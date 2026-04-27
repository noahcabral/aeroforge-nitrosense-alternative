use crate::paths::ServicePaths;

use super::models::{
    AppliedBootLogoSnapshot, AppliedFanControlSnapshot, AppliedGpuTuningSnapshot,
    AppliedPowerProfileSnapshot, ControlSnapshot,
};

pub fn load_snapshot(
    paths: &ServicePaths,
) -> Result<ControlSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let path = paths.worker_snapshot("control");
    if !path.exists() {
        return Ok(ControlSnapshot::default_snapshot("control-worker"));
    }

    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str::<ControlSnapshot>(&raw)?)
}

pub fn persist_default_snapshot(
    paths: &ServicePaths,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = paths.worker_snapshot("control");
    if path.exists() {
        return Ok(());
    }

    persist_snapshot(paths, &ControlSnapshot::default_snapshot("control-worker"))
}

pub fn persist_apply_success(
    paths: &ServicePaths,
    applied: &AppliedPowerProfileSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.active_power_profile = Some(applied.profile_id.clone());
    snapshot.processor_state = Some(applied.processor_state.clone());
    snapshot.processor_state_readback = Some(applied.readback.clone());
    snapshot.processor_state_drift_detected = applied.drift_detected;
    snapshot.last_applied_at_unix = Some(applied.applied_at_unix);
    snapshot.last_apply_detail = applied.detail.clone();
    snapshot.last_error = None;
    persist_snapshot(paths, &snapshot)
}

pub fn persist_gpu_tuning_apply_success(
    paths: &ServicePaths,
    applied: &AppliedGpuTuningSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.gpu_tuning_apply_supported = true;
    snapshot.active_gpu_tuning = Some(applied.tuning.clone());
    snapshot.last_gpu_tuning_applied_at_unix = Some(applied.applied_at_unix);
    snapshot.last_gpu_tuning_detail = applied.detail.clone();
    snapshot.last_gpu_tuning_error = None;
    persist_snapshot(paths, &snapshot)
}

pub fn persist_fan_apply_success(
    paths: &ServicePaths,
    applied: &AppliedFanControlSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.fan_apply_supported = true;
    snapshot.fan_curve_apply_supported = true;
    snapshot.active_fan_profile = Some(applied.profile_id.clone());
    if let Some(curves) = applied.curves.clone() {
        snapshot.active_fan_curves = Some(curves);
    }
    snapshot.last_fan_applied_at_unix = Some(applied.applied_at_unix);
    snapshot.last_fan_apply_detail = applied.detail.clone();
    snapshot.last_fan_error = None;
    snapshot.last_fan_readback = applied.readback.clone();
    persist_snapshot(paths, &snapshot)
}

pub fn persist_boot_logo_apply_success(
    paths: &ServicePaths,
    applied: &AppliedBootLogoSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.boot_logo_apply_supported = true;
    snapshot.last_boot_logo_applied_at_unix = Some(applied.applied_at_unix);
    snapshot.last_boot_logo_apply_detail = applied.detail.clone();
    snapshot.last_boot_logo_error = None;
    snapshot.last_boot_logo_readback = applied.readback.clone();
    persist_snapshot(paths, &snapshot)
}

pub fn persist_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_error = Some(detail.into());
    snapshot.last_apply_detail = "The most recent power-profile apply attempt failed.".into();
    snapshot.processor_state_drift_detected = false;
    persist_snapshot(paths, &snapshot)
}

pub fn persist_gpu_tuning_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_gpu_tuning_error = Some(detail.into());
    snapshot.last_gpu_tuning_detail = "The most recent GPU tuning apply attempt failed.".into();
    persist_snapshot(paths, &snapshot)
}

pub fn persist_fan_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_fan_error = Some(detail.into());
    snapshot.last_fan_apply_detail = "The most recent fan-control apply attempt failed.".into();
    persist_snapshot(paths, &snapshot)
}

pub fn persist_boot_logo_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_boot_logo_error = Some(detail.into());
    snapshot.last_boot_logo_apply_detail =
        "The most recent boot-logo apply attempt failed.".into();
    persist_snapshot(paths, &snapshot)
}

fn persist_snapshot(
    paths: &ServicePaths,
    snapshot: &ControlSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    std::fs::write(
        paths.worker_snapshot("control"),
        serde_json::to_string_pretty(snapshot)?,
    )?;
    Ok(())
}
