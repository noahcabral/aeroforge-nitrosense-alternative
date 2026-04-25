mod acer_wmi;
mod fan;
mod gpu_tuning;
mod models;
mod nvapi_whisper;
mod nvml;
mod power;
mod state;

use crate::{
    paths::{write_log_line, ServicePaths},
    workers::{run_periodic_worker, WorkerEventSender, WorkerRegistration},
};

pub use models::{
    AppliedFanControlSnapshot, AppliedGpuTuningSnapshot, AppliedPowerProfileSnapshot,
    ApplyCustomFanCurvesRequest, ApplyFanProfileRequest, ApplyGpuTuningRequest,
    ApplyPowerProfileRequest, FanProfileId,
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
