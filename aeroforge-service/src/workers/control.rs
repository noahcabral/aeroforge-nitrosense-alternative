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

type DynError = Box<dyn std::error::Error + Send + Sync>;

const WORKER_NAME: &str = "control-worker";
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const ADAPTIVE_FAN_REFRESH_INTERVAL_SECS: u64 = 5;
const ADAPTIVE_FAN_TARGET_CHANGE_THRESHOLD_PERCENT: u8 = 3;
const ADAPTIVE_FAN_FORCE_REFRESH_SECS: u64 = 60;
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
    record_apply(
        paths,
        "control-power",
        power::apply_power_profile(paths, request),
        state::persist_apply_success,
        state::persist_apply_error,
    )
}

pub fn apply_gpu_tuning(
    paths: &ServicePaths,
    request: ApplyGpuTuningRequest,
) -> Result<AppliedGpuTuningSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    record_apply(
        paths,
        "control-gpu-tuning",
        gpu_tuning::apply_gpu_tuning(paths, request),
        state::persist_gpu_tuning_apply_success,
        state::persist_gpu_tuning_apply_error,
    )
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
        return record_fan_apply(
            paths,
            fan::apply_quiet_auto_fan_policy(paths, &snapshot, false),
        );
    }

    record_fan_apply(paths, fan::apply_fan_profile(paths, request))
}

pub fn apply_boot_logo(
    paths: &ServicePaths,
    request: ApplyBootLogoRequest,
) -> Result<AppliedBootLogoSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    record_apply(
        paths,
        "control-boot-logo",
        boot_logo::apply_boot_logo(paths, request),
        state::persist_boot_logo_apply_success,
        state::persist_boot_logo_apply_error,
    )
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
    record_fan_apply(paths, fan::apply_custom_fan_curves(paths, request))
}

fn record_fan_apply(
    paths: &ServicePaths,
    result: Result<AppliedFanControlSnapshot, DynError>,
) -> Result<AppliedFanControlSnapshot, DynError> {
    record_apply(
        paths,
        "control-fan",
        result,
        state::persist_fan_apply_success,
        state::persist_fan_apply_error,
    )
}

/// Every request-driven apply records its outcome the same way: the success
/// snapshot is persisted, a failure is logged and persisted as the last error.
fn record_apply<T>(
    paths: &ServicePaths,
    component: &str,
    result: Result<T, DynError>,
    persist_success: fn(&ServicePaths, &T) -> Result<(), DynError>,
    persist_error: fn(&ServicePaths, &str) -> Result<(), DynError>,
) -> Result<T, DynError> {
    match result {
        Ok(applied) => {
            persist_success(paths, &applied)?;
            Ok(applied)
        }
        Err(error) => {
            let detail = error.to_string();
            let _ = write_log_line(&paths.component_log(component), "ERROR", &detail);
            let _ = persist_error(paths, &detail);
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
        settle_seconds: fan::FAN_SPEED_CALIBRATION_SETTLE_SECS,
        points: Vec::new(),
        last_error: None,
    };
    // Until the worker thread owns the flag, any early failure must release it or every
    // later fan apply is rejected as "calibration is running" until the service restarts.
    let release_on_error = |result: Result<(), Box<dyn std::error::Error + Send + Sync>>| {
        if result.is_err() {
            FAN_CALIBRATION_RUNNING.store(false, Ordering::SeqCst);
        }
        result
    };
    release_on_error(state::persist_fan_speed_calibration(
        paths,
        calibration.clone(),
    ))?;

    let job_paths = paths.clone();
    let spawned = thread::Builder::new()
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
        });
    if let Err(error) = spawned {
        FAN_CALIBRATION_RUNNING.store(false, Ordering::SeqCst);
        // The queued snapshot already says running; leave a terminal one so the UI does
        // not show a phantom calibration until the user cancels it.
        let mut failed = calibration;
        failed.running = false;
        failed.updated_at_unix = Some(unix_timestamp());
        failed.completed_at_unix = Some(unix_timestamp());
        failed.status = format!("Fan speed calibration failed. {error}");
        failed.last_error = Some(error.to_string());
        let _ = state::persist_fan_speed_calibration(paths, failed);
        return Err(error.into());
    }

    Ok(calibration)
}

pub fn cancel_fan_speed_calibration(
    paths: &ServicePaths,
) -> Result<FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if !FAN_CALIBRATION_RUNNING.load(Ordering::SeqCst) {
        return clear_stale_fan_speed_calibration(paths);
    }

    FAN_CALIBRATION_CANCEL_REQUESTED.store(true, Ordering::SeqCst);

    let mut snapshot = state::load_snapshot(paths)?;
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

/// A service that died mid-calibration leaves `running = true` in the snapshot,
/// which the UI shows as "Running" with no thread behind it.
fn clear_stale_fan_speed_calibration(
    paths: &ServicePaths,
) -> Result<FanSpeedCalibrationSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let mut calibration = state::load_snapshot(paths)?.fan_speed_calibration;
    if calibration.running {
        let now = unix_timestamp();
        calibration.running = false;
        calibration.current_percent = None;
        calibration.updated_at_unix = Some(now);
        calibration.completed_at_unix = Some(now);
        calibration.status =
            "Fan speed calibration was not running; cleared stale running state.".into();
        state::persist_fan_speed_calibration(paths, calibration.clone())?;
    }
    Ok(calibration)
}

fn run(
    paths: ServicePaths,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    event_tx: WorkerEventSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    state::persist_default_snapshot(&paths)?;
    if let Err(error) = clear_stale_fan_speed_calibration(&paths) {
        let _ = write_log_line(
            &paths.component_log("control-fan"),
            "WARN",
            &format!("Could not clear stale fan calibration state: {error}"),
        );
    }
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

    if !fan_profile_refreshable(
        initial_snapshot.active_fan_profile.as_ref(),
        initial_snapshot.active_fan_curves.is_some(),
    ) {
        return Ok(());
    }

    if !adaptive_fan_refresh_due(initial_snapshot.last_fan_applied_at_unix) {
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

    let Some(profile_id) = snapshot.active_fan_profile.clone() else {
        return Ok(());
    };
    let curves = match profile_id {
        FanProfileId::Custom => {
            let Some(curves) = snapshot.active_fan_curves.clone() else {
                return Ok(());
            };
            curves
        }
        FanProfileId::Cool => fan::cool_fan_curve_set(),
        _ => return Ok(()),
    };

    let targets = fan::resolve_curve_backed_targets(paths, profile_id, curves);
    if !adaptive_fan_target_refresh_needed(
        snapshot.last_fan_applied_at_unix,
        snapshot.current_cpu_fan_speed_percent,
        snapshot.current_gpu_fan_speed_percent,
        Some(targets.cpu_speed_percent),
        Some(targets.gpu_speed_percent),
        unix_timestamp(),
    ) {
        return Ok(());
    }

    let apply_result = fan::apply_curve_backed_targets(paths, targets, true);

    match apply_result {
        Ok(applied) => state::persist_fan_apply_success(paths, &applied)?,
        Err(error) => {
            let detail = format!("Periodic adaptive fan curve refresh failed: {error}");
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

fn fan_profile_refreshable(profile_id: Option<&FanProfileId>, has_saved_curves: bool) -> bool {
    match profile_id {
        Some(FanProfileId::Custom) => has_saved_curves,
        Some(FanProfileId::Cool) => true,
        _ => false,
    }
}

fn adaptive_fan_refresh_due(last_applied_at_unix: Option<u64>) -> bool {
    fan_refresh_due(last_applied_at_unix, ADAPTIVE_FAN_REFRESH_INTERVAL_SECS)
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

fn adaptive_fan_target_refresh_needed(
    last_applied_at_unix: Option<u64>,
    current_cpu_speed_percent: Option<u8>,
    current_gpu_speed_percent: Option<u8>,
    next_cpu_speed_percent: Option<u8>,
    next_gpu_speed_percent: Option<u8>,
    now_unix: u64,
) -> bool {
    let Some(last_applied_at_unix) = last_applied_at_unix else {
        return true;
    };

    let age_secs = now_unix.saturating_sub(last_applied_at_unix);
    if age_secs < ADAPTIVE_FAN_REFRESH_INTERVAL_SECS {
        return false;
    }

    if age_secs >= ADAPTIVE_FAN_FORCE_REFRESH_SECS {
        return true;
    }

    fan_target_changed(current_cpu_speed_percent, next_cpu_speed_percent)
        || fan_target_changed(current_gpu_speed_percent, next_gpu_speed_percent)
}

fn fan_target_changed(current: Option<u8>, next: Option<u8>) -> bool {
    match (current, next) {
        (Some(current), Some(next)) => {
            current.abs_diff(next) >= ADAPTIVE_FAN_TARGET_CHANGE_THRESHOLD_PERCENT
        }
        (None, None) => false,
        _ => true,
    }
}

pub fn restore_startup_state(
    paths: &ServicePaths,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let snapshot = state::load_snapshot(paths)?;
    let power_profile = snapshot
        .active_power_profile
        .clone()
        .unwrap_or(models::PowerProfileId::Balanced);
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

#[cfg(test)]
mod tests {
    use super::{
        adaptive_fan_target_refresh_needed, clear_stale_fan_speed_calibration,
        fan_profile_refreshable, start_fan_speed_calibration, state, FanProfileId,
        FanSpeedCalibrationSnapshot, FAN_CALIBRATION_RUNNING,
    };
    use crate::paths::ServicePaths;
    use std::sync::atomic::Ordering;

    #[test]
    fn calibration_start_releases_running_flag_when_persist_fails() {
        // state_dir nested under a regular file cannot be created, so the queued snapshot
        // persist fails before the worker thread exists.
        let temp = tempfile::tempdir().expect("tempdir");
        let blocker = temp.path().join("blocker");
        std::fs::write(&blocker, b"").expect("blocker file");
        let paths = ServicePaths {
            logs_dir: temp.path().join("logs"),
            state_dir: blocker.join("state"),
        };

        assert!(start_fan_speed_calibration(&paths).is_err());
        assert!(!FAN_CALIBRATION_RUNNING.load(Ordering::SeqCst));
    }

    #[test]
    fn stale_running_calibration_is_cleared_when_no_thread_owns_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = ServicePaths {
            logs_dir: temp.path().join("logs"),
            state_dir: temp.path().join("state"),
        };
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        state::persist_default_snapshot(&paths).expect("default snapshot");
        state::persist_fan_speed_calibration(
            &paths,
            FanSpeedCalibrationSnapshot {
                running: true,
                current_percent: Some(40),
                ..FanSpeedCalibrationSnapshot::default()
            },
        )
        .expect("stale snapshot");

        let cleared = clear_stale_fan_speed_calibration(&paths).expect("clear");
        assert!(!cleared.running);
        assert_eq!(cleared.current_percent, None);
        assert!(cleared.completed_at_unix.is_some());
        let reloaded = state::load_snapshot(&paths).expect("reload");
        assert!(!reloaded.fan_speed_calibration.running);
    }

    #[test]
    fn adaptive_fan_refresh_includes_custom_and_cool() {
        assert!(fan_profile_refreshable(Some(&FanProfileId::Custom), true));
        assert!(!fan_profile_refreshable(Some(&FanProfileId::Custom), false));
        assert!(fan_profile_refreshable(Some(&FanProfileId::Cool), false));
        assert!(!fan_profile_refreshable(Some(&FanProfileId::Auto), true));
        assert!(!fan_profile_refreshable(Some(&FanProfileId::Max), true));
        assert!(!fan_profile_refreshable(None, true));
    }

    #[test]
    fn adaptive_fan_target_refresh_skips_small_changes_until_force_age() {
        assert!(!adaptive_fan_target_refresh_needed(
            Some(100),
            Some(35),
            Some(36),
            Some(37),
            Some(37),
            105,
        ));
    }

    #[test]
    fn adaptive_fan_target_refresh_runs_for_missing_changed_or_old_targets() {
        assert!(adaptive_fan_target_refresh_needed(
            None,
            Some(35),
            Some(36),
            Some(35),
            Some(36),
            100,
        ));
        assert!(adaptive_fan_target_refresh_needed(
            Some(100),
            Some(35),
            Some(36),
            Some(39),
            Some(36),
            105,
        ));
        assert!(adaptive_fan_target_refresh_needed(
            Some(100),
            Some(35),
            Some(36),
            Some(36),
            Some(37),
            161,
        ));
    }
}
