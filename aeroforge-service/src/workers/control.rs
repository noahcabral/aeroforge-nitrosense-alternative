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
use std::time::Instant;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    thread,
};

pub use models::{
    AppliedBootLogoSnapshot, AppliedFanControlSnapshot, AppliedGpuTuningSnapshot,
    AppliedPowerProfileSnapshot, AppliedSmartChargeSnapshot, AppliedTelemetrySettingsSnapshot,
    ApplyBootLogoRequest, ApplyCustomFanCurvesRequest, ApplyFanProfileRequest,
    ApplyGpuTuningRequest, ApplyPowerProfileRequest, ApplySmartChargeRequest,
    ApplyTelemetrySettingsRequest, FanProfileId, FanSpeedCalibrationSnapshot,
};

const WORKER_NAME: &str = "control-worker";
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const CUSTOM_FAN_REFRESH_INTERVAL_SECS: u64 = 1;
const QUIET_AUTO_REFRESH_INTERVAL_SECS: u64 = 5;
const STARTUP_RECONCILE_CHECKPOINTS_SECS: [u64; 3] = [15, 45, 120];
static FAN_APPLY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static STARTUP_RECONCILE_STATE: OnceLock<Mutex<StartupReconcileState>> = OnceLock::new();
static FAN_CALIBRATION_RUNNING: AtomicBool = AtomicBool::new(false);
static FAN_CALIBRATION_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

struct StartupReconcileState {
    started_at: Instant,
    next_checkpoint_index: usize,
}

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
    if FAN_CALIBRATION_RUNNING.load(Ordering::SeqCst) {
        return Err(
            "Fan speed calibration is running. Wait for it to finish before changing fan modes."
                .into(),
        );
    }

    let _fan_apply_guard = FAN_APPLY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Fan apply lock was poisoned.")?;

    let snapshot = state::load_snapshot(paths)?;

    if matches!(request.profile_id, FanProfileId::Custom) {
        let curves = snapshot.active_fan_curves.ok_or_else(|| {
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

    if matches!(request.profile_id, FanProfileId::Auto)
        && matches!(
            snapshot.active_power_profile,
            Some(models::PowerProfileId::BatteryGuard)
        )
    {
        match fan::apply_quiet_auto_fan_policy(paths, &snapshot, false) {
            Ok(applied) => {
                state::persist_fan_apply_success(paths, &applied)?;
                return Ok(applied);
            }
            Err(error) => {
                let detail = error.to_string();
                let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
                let _ = state::persist_fan_apply_error(paths, &detail);
                return Err(error);
            }
        }
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
    if FAN_CALIBRATION_RUNNING.load(Ordering::SeqCst) {
        return Err(
            "Fan speed calibration is running. Wait for it to finish before changing fan curves."
                .into(),
        );
    }

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

pub fn start_fan_speed_calibration(
    paths: &ServicePaths,
) -> Result<FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if FAN_CALIBRATION_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(state::load_snapshot(paths)?.fan_speed_calibration);
    }

    FAN_CALIBRATION_CANCEL_REQUESTED.store(false, Ordering::SeqCst);

    let now = unix_timestamp();
    let calibration = FanSpeedCalibrationSnapshot {
        running: true,
        status: "Fan speed calibration queued. Testing every 5% with a 20 second settle per step."
            .into(),
        started_at_unix: Some(now),
        updated_at_unix: Some(now),
        completed_at_unix: None,
        current_percent: None,
        settle_seconds: 20,
        points: Vec::new(),
        last_error: None,
    };
    state::persist_fan_speed_calibration(paths, calibration.clone())?;

    let job_paths = paths.clone();
    thread::Builder::new()
        .name("aeroforge-fan-calibration".into())
        .spawn(move || {
            let result = (|| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                let _fan_apply_guard = FAN_APPLY_LOCK
                    .get_or_init(|| Mutex::new(()))
                    .lock()
                    .map_err(|_| "Fan apply lock was poisoned.")?;
                let completed = fan::run_fan_speed_calibration(&job_paths, &|| {
                    FAN_CALIBRATION_CANCEL_REQUESTED.load(Ordering::SeqCst)
                })?;
                state::persist_fan_speed_calibration(&job_paths, completed)?;
                Ok(())
            })();

            if let Err(error) = result {
                let detail = error.to_string();
                let _ = write_log_line(
                    &job_paths.component_log("control-fan"),
                    "ERROR",
                    &format!("Fan speed calibration failed: {detail}"),
                );
                let mut snapshot = state::load_snapshot(&job_paths)
                    .unwrap_or_else(|_| models::ControlSnapshot::default_snapshot(WORKER_NAME));
                snapshot.fan_speed_calibration.running = false;
                snapshot.fan_speed_calibration.current_percent = None;
                snapshot.fan_speed_calibration.updated_at_unix = Some(unix_timestamp());
                snapshot.fan_speed_calibration.completed_at_unix = Some(unix_timestamp());
                snapshot.fan_speed_calibration.status =
                    format!("Fan speed calibration failed. {detail}");
                snapshot.fan_speed_calibration.last_error = Some(detail);
                let _ = state::persist_fan_speed_calibration(
                    &job_paths,
                    snapshot.fan_speed_calibration,
                );
            }

            FAN_CALIBRATION_RUNNING.store(false, Ordering::SeqCst);
            FAN_CALIBRATION_CANCEL_REQUESTED.store(false, Ordering::SeqCst);
        })?;

    Ok(calibration)
}

pub fn cancel_fan_speed_calibration(
    paths: &ServicePaths,
) -> Result<FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = state::load_snapshot(paths)?;
    if !FAN_CALIBRATION_RUNNING.load(Ordering::SeqCst) {
        if snapshot.fan_speed_calibration.running {
            let now = unix_timestamp();
            snapshot.fan_speed_calibration.running = false;
            snapshot.fan_speed_calibration.current_percent = None;
            snapshot.fan_speed_calibration.updated_at_unix = Some(now);
            snapshot.fan_speed_calibration.completed_at_unix = Some(now);
            snapshot.fan_speed_calibration.status =
                "Fan speed calibration was not running; cleared stale running state.".into();
            state::persist_fan_speed_calibration(paths, snapshot.fan_speed_calibration.clone())?;
        }
        return Ok(snapshot.fan_speed_calibration);
    }

    FAN_CALIBRATION_CANCEL_REQUESTED.store(true, Ordering::SeqCst);

    let now = unix_timestamp();
    snapshot.fan_speed_calibration.updated_at_unix = Some(now);
    snapshot.fan_speed_calibration.status =
        "Fan speed calibration cancel requested. Restoring the previous fan mode.".into();
    snapshot.fan_speed_calibration.last_error = None;
    state::persist_fan_speed_calibration(paths, snapshot.fan_speed_calibration.clone())?;

    write_log_line(
        &paths.component_log("control-fan"),
        "INFO",
        "Fan speed calibration cancel requested by UI.",
    )?;

    Ok(snapshot.fan_speed_calibration)
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

fn tick(paths: &ServicePaths) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state::persist_default_snapshot(paths)?;

    if FAN_CALIBRATION_RUNNING.load(Ordering::SeqCst) {
        return Ok(());
    }

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

    if run_due_startup_reconcile(paths)? {
        return Ok(());
    }

    if matches!(
        initial_snapshot.active_power_profile,
        Some(models::PowerProfileId::BatteryGuard)
    ) && matches!(
        initial_snapshot.active_fan_profile,
        Some(FanProfileId::Auto)
    ) {
        if !quiet_auto_refresh_due(initial_snapshot.last_fan_applied_at_unix) {
            return Ok(());
        }

        let _fan_apply_guard = FAN_APPLY_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| "Fan apply lock was poisoned.")?;

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

        if !matches!(
            snapshot.active_power_profile,
            Some(models::PowerProfileId::BatteryGuard)
        ) || !matches!(snapshot.active_fan_profile, Some(FanProfileId::Auto))
        {
            return Ok(());
        }

        match fan::apply_quiet_auto_fan_policy(paths, &snapshot, true) {
            Ok(applied) => state::persist_fan_apply_success(paths, &applied)?,
            Err(error) => {
                let detail = format!("Periodic Quiet Auto fan policy refresh failed: {error}");
                let _ = write_log_line(&paths.component_log("control-fan"), "ERROR", &detail);
                state::persist_fan_apply_error(paths, &detail)?;
            }
        }

        return Ok(());
    }

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

    let _fan_apply_guard = FAN_APPLY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Fan apply lock was poisoned.")?;

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

    if !matches!(snapshot.active_fan_profile, Some(FanProfileId::Custom)) {
        return Ok(());
    }

    let Some(curves) = snapshot.active_fan_curves else {
        return Ok(());
    };

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

fn run_due_startup_reconcile(
    paths: &ServicePaths,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let mut state = STARTUP_RECONCILE_STATE
        .get_or_init(|| {
            Mutex::new(StartupReconcileState {
                started_at: Instant::now(),
                next_checkpoint_index: 0,
            })
        })
        .lock()
        .map_err(|_| "Startup reconcile lock was poisoned.")?;

    let Some(next_checkpoint_secs) = STARTUP_RECONCILE_CHECKPOINTS_SECS
        .get(state.next_checkpoint_index)
        .copied()
    else {
        return Ok(false);
    };

    if state.started_at.elapsed().as_secs() < next_checkpoint_secs {
        return Ok(false);
    }

    state.next_checkpoint_index += 1;
    drop(state);

    write_log_line(
        &paths.component_log("control-worker"),
        "INFO",
        &format!(
            "Running post-boot restore reconcile at +{next_checkpoint_secs}s to counter delayed firmware or vendor-service overrides."
        ),
    )?;

    if let Err(error) = restore_startup_state(paths) {
        let _ = write_log_line(
            &paths.component_log("control-worker"),
            "ERROR",
            &format!("Post-boot restore reconcile failed: {error}"),
        );
    }

    Ok(true)
}

fn custom_fan_refresh_due(last_applied_at_unix: Option<u64>) -> bool {
    fan_refresh_due(last_applied_at_unix, CUSTOM_FAN_REFRESH_INTERVAL_SECS)
}

fn quiet_auto_refresh_due(last_applied_at_unix: Option<u64>) -> bool {
    fan_refresh_due(last_applied_at_unix, QUIET_AUTO_REFRESH_INTERVAL_SECS)
}

fn fan_refresh_due(last_applied_at_unix: Option<u64>, interval_secs: u64) -> bool {
    let Some(last_applied_at_unix) = last_applied_at_unix else {
        return true;
    };

    unix_timestamp().saturating_sub(last_applied_at_unix) >= interval_secs
}

pub fn restore_startup_state(
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
    if let Some(tuning) = snapshot.active_gpu_tuning {
        match apply_gpu_tuning(
            paths,
            ApplyGpuTuningRequest { tuning },
        ) {
            Ok(applied) => {
                let _ = write_log_line(
                    &paths.component_log("control-gpu-tuning"),
                    "INFO",
                    &format!("Startup GPU OC applied: {}", applied.detail),
                );
            }
            Err(error) => {
                let detail = format!("Startup GPU OC failed to apply: {error}");
                let _ = write_log_line(&paths.component_log("control-gpu-tuning"), "ERROR", &detail);
            }
        }
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
