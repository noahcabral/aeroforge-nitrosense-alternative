mod acer_hid;
pub(crate) mod acer_wmi;
mod boot_logo;
mod fan;
mod gpu_tuning;
mod models;
mod nvapi_whisper;
mod nvidia_power;
mod nvml;
mod power;
mod rapl_power;
mod smart_charge;
mod state;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::{run_periodic_worker, unix_timestamp, WorkerEventSender, WorkerRegistration},
};
use std::sync::{Mutex, OnceLock};

pub use models::{
    AppliedBootLogoSnapshot, AppliedFanControlSnapshot, AppliedGpuTuningSnapshot,
    AppliedPowerProfileSnapshot, AppliedSmartChargeSnapshot, AppliedTelemetrySettingsSnapshot,
    ApplyBootLogoRequest, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest,
    ApplyGpuTuningRequest, ApplyPowerProfileRequest, ApplySmartChargeRequest,
    ApplyTelemetrySettingsRequest, FanProfileId,
};

const WORKER_NAME: &str = "control-worker";
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const CUSTOM_FAN_REFRESH_INTERVAL_SECS: u64 = 1;
static FAN_APPLY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn registration() -> WorkerRegistration {
    WorkerRegistration::new(WORKER_NAME, run)
}

pub fn apply_power_profile(
    paths: &ServicePaths,
    request: ApplyPowerProfileRequest,
) -> Result<AppliedPowerProfileSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    match power::apply_power_profile(paths, request) {
        Ok(applied) => {
            state::persist_apply_success(paths, &applied)?;
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(&paths.component_log("control-power"), "ERROR", &detail);
            let _ = state::persist_apply_error(paths, &detail);
            Err(error)
        }
    }
}

pub fn apply_gpu_tuning(
    paths: &ServicePaths,
    request: ApplyGpuTuningRequest,
) -> Result<AppliedGpuTuningSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    match gpu_tuning::apply_gpu_tuning(paths, request) {
        Ok(applied) => {
            state::persist_gpu_tuning_apply_success(paths, &applied)?;
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(&paths.component_log("control-gpu-tuning"), "ERROR", &detail);
            let _ = state::persist_gpu_tuning_apply_error(paths, &detail);
            Err(error)
        }
    }
}

pub fn apply_fan_profile(
    paths: &ServicePaths,
    request: ApplyFanProfileRequest,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let _fan_apply_guard = FAN_APPLY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Fan apply lock was poisoned.")?;

    if matches!(request.profile_id, FanProfileId::Custom) {
        let curves = state::load_snapshot(paths)?
            .active_fan_curves
            .ok_or_else(|| {
                "Custom fan mode requires a saved curve before it can be applied.".to_string()
            })?;

        return apply_custom_fan_curves_unlocked(
            paths,
            ApplyCustomFanCurvesRequest {
                curves,
                quiet_success_log: false,
            },
        );
    }

    match fan::apply_fan_profile(paths, request) {
        Ok(applied) => {
            state::persist_fan_apply_success(paths, &applied)?;
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
            let _ = state::persist_fan_apply_error(paths, &detail);
            Err(error)
        }
    }
}

pub fn apply_boot_logo(
    paths: &ServicePaths,
    request: ApplyBootLogoRequest,
) -> Result<AppliedBootLogoSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    match boot_logo::apply_boot_logo(paths, request) {
        Ok(applied) => {
            state::persist_boot_logo_apply_success(paths, &applied)?;
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(&paths.component_log("control-boot-logo"), "ERROR", &detail);
            let _ = state::persist_boot_logo_apply_error(paths, &detail);
            Err(error)
        }
    }
}

pub fn apply_smart_charging(
    paths: &ServicePaths,
    request: ApplySmartChargeRequest,
) -> Result<AppliedSmartChargeSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    match smart_charge::apply_smart_charging(paths, request) {
        Ok(applied) => {
            let _ = write_log_line(
                &paths.component_log("control-smart-charge"),
                "INFO",
                &applied.detail,
            );
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(
                &paths.component_log("control-smart-charge"),
                "ERROR",
                &detail,
            );
            Err(error)
        }
    }
}

pub fn apply_telemetry_settings(
    paths: &ServicePaths,
    request: ApplyTelemetrySettingsRequest,
) -> Result<AppliedTelemetrySettingsSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    state::persist_telemetry_settings(paths, request.nvidia_telemetry_enabled)?;

    let detail = if request.nvidia_telemetry_enabled {
        "NVIDIA telemetry polling enabled. AeroForge may read dGPU clocks, power, and limits when Windows reports active dGPU memory."
    } else {
        "NVIDIA telemetry polling disabled. AeroForge will skip NVML and nvidia-smi reads so the dGPU can idle."
    }
    .to_string();

    write_log_line(&paths.component_log("control-telemetry"), "INFO", &detail)?;

    Ok(AppliedTelemetrySettingsSnapshot {
        nvidia_telemetry_enabled: request.nvidia_telemetry_enabled,
        detail,
    })
}

pub fn apply_custom_fan_curves(
    paths: &ServicePaths,
    request: ApplyCustomFanCurvesRequest,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let _fan_apply_guard = FAN_APPLY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Fan apply lock was poisoned.")?;

    apply_custom_fan_curves_unlocked(paths, request)
}

fn apply_custom_fan_curves_unlocked(
    paths: &ServicePaths,
    request: ApplyCustomFanCurvesRequest,
) -> Result<AppliedFanControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    match fan::apply_custom_fan_curves(paths, request) {
        Ok(applied) => {
            state::persist_fan_apply_success(paths, &applied)?;
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
            let _ = state::persist_fan_apply_error(paths, &detail);
            Err(error)
        }
    }
}

fn run(
    paths: ServicePaths,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state::persist_default_snapshot(&paths)?;
    if let Err(error) = restore_startup_state(&paths) {
        let _ = write_log_line(
            &paths.component_log("control-worker"),
            "ERROR",
            &format!("Startup restore failed and will be retried by the periodic worker: {error}"),
        );
    }

    run_periodic_worker(
        WORKER_NAME,
        SAMPLE_INTERVAL,
        paths,
        stop_flag,
        event_tx,
        tick,
    )
}

// made by faxcon
fn tick(paths: &ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state::persist_default_snapshot(paths)?;

    // INITIAL READ: Check if acquiring the lock is worth it
    let initial_snapshot = match state::load_snapshot(paths) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = write_log_line(
                &paths.component_log("control-worker"),
                "ERROR",
                &format!(
                    "Control snapshot was temporarily unavailable; next tick will retry: {error}"
                ),
            );
            return Ok(());
        }
    };

    // Quick exit checks - before acquiring lock
    if !matches!(
        initial_snapshot.active_fan_profile,
        Some(FanProfileId::Custom)
    ) {
        return Ok(());
    }

    if initial_snapshot.active_fan_curves.is_none() {
        return Ok(());
    }

    if !custom_fan_refresh_due(initial_snapshot.last_fan_applied_at_unix) {
        return Ok(());
    }

    // Acquire lock
    let _fan_apply_guard = FAN_APPLY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Fan apply lock was poisoned.")?;

    // *** CRITICAL FIX: Re-read after acquiring lock ***
    let snapshot = match state::load_snapshot(paths) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = write_log_line(
                &paths.component_log("control-worker"),
                "ERROR",
                &format!("Control snapshot unavailable after lock acquisition: {error}"),
            );
            return Ok(());
        }
    };

    // Re-check - mode may have changed.
    if !matches!(snapshot.active_fan_profile, Some(FanProfileId::Custom)) {
        // Switched to Auto, exit
        return Ok(());
    }

    let Some(curves) = snapshot.active_fan_curves else {
        return Ok(());
    };

    // Now safe - apply with fresh state
    match fan::apply_custom_fan_curves(
        paths,
        ApplyCustomFanCurvesRequest {
            curves,
            quiet_success_log: true,
        },
    ) {
        Ok(applied) => state::persist_fan_apply_success(paths, &applied)?,
        Err(error) => {
            let detail = format!("Periodic custom fan curve refresh failed: {error}");
            let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
            state::persist_fan_apply_error(paths, &detail)?;
        }
    }

    Ok(())
}

fn custom_fan_refresh_due(last_applied_at_unix: Option<u64>) -> bool {
    let Some(last_applied_at_unix) = last_applied_at_unix else {
        return true;
    };

    unix_timestamp().saturating_sub(last_applied_at_unix) >= CUSTOM_FAN_REFRESH_INTERVAL_SECS
}

fn restore_startup_state(
    paths: &ServicePaths,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = state::load_snapshot(paths)?;
    let power_profile = snapshot
        .active_power_profile
        .clone()
        .unwrap_or(models::PowerProfileId::Turbo);
    let processor_state = snapshot
        .processor_state
        .clone()
        .unwrap_or_else(|| default_processor_state_for_profile(&power_profile));
    let processor_state_control_enabled = snapshot.processor_state_control_enabled;

    write_log_line(
        &paths.component_log("control-power"),
        "INFO",
        &format!(
            "Restoring startup power profile {:?} with processor state min {} / max {} (processor state writes {}).",
            power_profile,
            processor_state.min_percent,
            processor_state.max_percent,
            if processor_state_control_enabled {
                "enabled"
            } else {
                "disabled"
            }
        ),
    )?;

    match apply_power_profile(
        paths,
        ApplyPowerProfileRequest {
            profile_id: power_profile,
            processor_state,
            custom_base_profile: snapshot.custom_base_profile.clone(),
            processor_state_control_enabled,
        },
    ) {
        Ok(applied) => {
            let _ = write_log_line(
                &paths.component_log("control-power"),
                "INFO",
                &format!("Startup power restore succeeded: {}", applied.detail),
            );
        }
        Err(error) => {
            let detail = format!("Startup power restore failed: {error}");
            let _ = write_log_line(&paths.component_log("control-power"), "ERROR", &detail);
        }
    }

    let fan_profile = snapshot
        .active_fan_profile
        .clone()
        .unwrap_or(FanProfileId::Auto);

    match fan_profile {
        FanProfileId::Custom => {
            if let Some(curves) = snapshot.active_fan_curves.clone() {
                match apply_custom_fan_curves(
                    paths,
                    ApplyCustomFanCurvesRequest {
                        curves,
                        quiet_success_log: false,
                    },
                ) {
                    Ok(applied) => {
                        let _ = write_log_line(
                            &paths.component_log("control-fan"),
                            "INFO",
                            &format!("Startup custom fan restore succeeded: {}", applied.detail),
                        );
                    }
                    Err(error) => {
                        let detail = format!("Startup custom fan restore failed: {error}");
                        let _ =
                            write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
                    }
                }
            } else {
                let _ = write_log_line(
                    &paths.component_log("control-fan"),
                    "WARN",
                    "Startup fan restore skipped: Custom was active but no saved curve was present.",
                );
            }
        }
        _ => match apply_fan_profile(
            paths,
            ApplyFanProfileRequest {
                profile_id: fan_profile,
            },
        ) {
            Ok(applied) => {
                let _ = write_log_line(
                    &paths.component_log("control-fan"),
                    "INFO",
                    &format!("Startup fan restore succeeded: {}", applied.detail),
                );
            }
            Err(error) => {
                let detail = format!("Startup fan restore failed: {error}");
                let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
            }
        },
    }

    Ok(())
}

fn default_processor_state_for_profile(
    profile_id: &models::PowerProfileId,
) -> models::ProcessorStateSettings {
    match profile_id {
        models::PowerProfileId::BatteryGuard => models::ProcessorStateSettings {
            min_percent: 5,
            max_percent: 45,
        },
        models::PowerProfileId::Balanced => models::ProcessorStateSettings {
            min_percent: 35,
            max_percent: 88,
        },
        models::PowerProfileId::Performance | models::PowerProfileId::Turbo => {
            models::ProcessorStateSettings {
                min_percent: 100,
                max_percent: 100,
            }
        }
        models::PowerProfileId::Custom => models::ProcessorStateSettings {
            min_percent: 35,
            max_percent: 88,
        },
    }
}
