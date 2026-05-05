pub(crate) mod acer_wmi;
mod boot_logo;
mod fan;
mod gpu_tuning;
mod models;
mod nvapi_whisper;
mod nvml;
mod power;
mod smart_charge;
mod state;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::{run_periodic_worker, WorkerEventSender, WorkerRegistration},
};

pub use models::{
    AppliedBootLogoSnapshot, AppliedFanControlSnapshot, AppliedGpuTuningSnapshot,
    AppliedPowerProfileSnapshot, AppliedSmartChargeSnapshot, ApplyBootLogoRequest,
    ApplyCustomFanCurvesRequest, ApplyFanProfileRequest, ApplyGpuTuningRequest,
    ApplyPowerProfileRequest, ApplySmartChargeRequest, FanProfileId,
};

const WORKER_NAME: &str = "control-worker";
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

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
    if matches!(request.profile_id, FanProfileId::Custom) {
        let curves = state::load_snapshot(paths)?
            .active_fan_curves
            .ok_or_else(|| {
                "Custom fan mode requires a saved curve before it can be applied.".to_string()
            })?;

        return apply_custom_fan_curves(paths, ApplyCustomFanCurvesRequest { curves });
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
    match smart_charge::apply_smart_charging(request) {
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

pub fn apply_custom_fan_curves(
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
    restore_startup_state(&paths)?;

    run_periodic_worker(
        WORKER_NAME,
        SAMPLE_INTERVAL,
        paths,
        stop_flag,
        event_tx,
        tick,
    )
}

fn tick(paths: &ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state::persist_default_snapshot(paths)?;

    let snapshot = state::load_snapshot(paths)?;
    if !matches!(snapshot.active_fan_profile, Some(FanProfileId::Custom)) {
        return Ok(());
    }

    let Some(curves) = snapshot.active_fan_curves else {
        return Ok(());
    };

    match fan::apply_custom_fan_curves(paths, ApplyCustomFanCurvesRequest { curves }) {
        Ok(applied) => state::persist_fan_apply_success(paths, &applied)?,
        Err(error) => {
            let detail = format!("Periodic custom fan curve refresh failed: {error}");
            let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
            state::persist_fan_apply_error(paths, &detail)?;
        }
    }

    Ok(())
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

    write_log_line(
        &paths.component_log("control-power"),
        "INFO",
        &format!(
            "Restoring startup power profile {:?} with processor state min {} / max {}.",
            power_profile, processor_state.min_percent, processor_state.max_percent
        ),
    )?;

    match apply_power_profile(
        paths,
        ApplyPowerProfileRequest {
            profile_id: power_profile,
            processor_state,
            custom_base_profile: snapshot.custom_base_profile.clone(),
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
                match apply_custom_fan_curves(paths, ApplyCustomFanCurvesRequest { curves }) {
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
