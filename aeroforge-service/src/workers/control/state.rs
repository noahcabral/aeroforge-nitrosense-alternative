use crate::paths::ServicePaths;
use crate::DynError;
use std::{fs, sync::Mutex, thread, time::Duration};

use super::models::{
    AppliedBootLogoSnapshot, AppliedFanControlSnapshot, AppliedGpuTuningSnapshot,
    AppliedPowerProfileSnapshot, ControlSnapshot, FanSpeedCalibrationSnapshot,
};

pub fn load_snapshot(paths: &ServicePaths) -> Result<ControlSnapshot, DynError> {
    let path = paths.worker_snapshot("control");
    if !path.exists() {
        return Ok(ControlSnapshot::default_snapshot("control-worker"));
    }

    let mut last_error: Option<String> = None;
    let mut content_bad = false;
    for attempt in 0..6 {
        content_bad = true;
        match fs::read_to_string(&path) {
            Ok(raw) if raw.trim().is_empty() => {
                last_error = Some("control snapshot was empty".into());
            }
            Ok(raw) => match serde_json::from_str::<ControlSnapshot>(json_without_bom(&raw)) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => {
                    last_error = Some(format!("control snapshot JSON was unreadable: {error}"));
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ControlSnapshot::default_snapshot("control-worker"));
            }
            Err(error) => {
                content_bad = false;
                last_error = Some(format!("control snapshot could not be read: {error}"));
            }
        }

        if attempt < 5 {
            thread::sleep(Duration::from_millis(50));
        }
    }

    // Writes are atomic, so content still bad after retries is real corruption.
    // Set it aside and start from defaults instead of failing every later write.
    if content_bad && fs::rename(&path, path.with_extension("json.corrupt")).is_ok() {
        let _ = crate::paths::write_log_line(
            &paths.component_log("control-worker"),
            "WARN",
            &format!(
                "{}; moved it to control.json.corrupt and reset to defaults.",
                last_error
                    .as_deref()
                    .unwrap_or("control snapshot was unreadable")
            ),
        );
        return Ok(ControlSnapshot::default_snapshot("control-worker"));
    }

    Err(last_error
        .unwrap_or_else(|| "control snapshot could not be loaded".into())
        .into())
}

pub fn persist_default_snapshot(paths: &ServicePaths) -> Result<(), DynError> {
    let path = paths.worker_snapshot("control");
    if path.exists() {
        return Ok(());
    }

    persist_snapshot(paths, &ControlSnapshot::default_snapshot("control-worker"))
}

pub fn persist_apply_success(
    paths: &ServicePaths,
    applied: &AppliedPowerProfileSnapshot,
) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.active_power_profile = Some(applied.profile_id.clone());
        snapshot.processor_state = Some(applied.processor_state.clone());
        snapshot.custom_base_profile = applied.custom_base_profile.clone();
        snapshot.processor_state_control_enabled = applied.processor_state_control_enabled;
        snapshot.processor_state_readback = Some(applied.readback.clone());
        snapshot.processor_state_drift_detected = applied.drift_detected;
        snapshot.last_applied_at_unix = Some(applied.applied_at_unix);
        snapshot.last_apply_detail = applied.detail.clone();
        snapshot.last_error = None;
        snapshot.last_power_layer_statuses = applied.layer_statuses.clone();
    })
}

pub fn persist_gpu_tuning_apply_success(
    paths: &ServicePaths,
    applied: &AppliedGpuTuningSnapshot,
) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.gpu_tuning_apply_supported = true;
        snapshot.active_gpu_tuning = Some(applied.tuning.clone());
        snapshot.last_gpu_tuning_applied_at_unix = Some(applied.applied_at_unix);
        snapshot.last_gpu_tuning_detail = applied.detail.clone();
        snapshot.last_gpu_tuning_error = None;
    })
}

pub fn persist_fan_apply_success(
    paths: &ServicePaths,
    applied: &AppliedFanControlSnapshot,
) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.fan_apply_supported = true;
        snapshot.fan_curve_apply_supported = true;
        snapshot.active_fan_profile = Some(applied.profile_id.clone());
        if let Some(curves) = applied.curves.clone() {
            snapshot.active_fan_curves = Some(curves);
        }
        snapshot.current_cpu_fan_speed_percent = applied.cpu_speed_percent;
        snapshot.current_gpu_fan_speed_percent = applied.gpu_speed_percent;
        snapshot.last_fan_applied_at_unix = Some(applied.applied_at_unix);
        snapshot.last_fan_apply_detail = applied.detail.clone();
        snapshot.last_fan_error = None;
        snapshot.last_fan_layer_statuses = applied.layer_statuses.clone();
        snapshot.last_fan_readback = applied.readback.clone();
        if let Some(calibration) = applied.quiet_auto_fan_calibration.clone() {
            snapshot.quiet_auto_fan_calibration = calibration;
        }
        snapshot.quiet_auto_thermal_warning = applied.quiet_auto_thermal_warning.clone();
    })
}

pub fn persist_boot_logo_apply_success(
    paths: &ServicePaths,
    applied: &AppliedBootLogoSnapshot,
) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.boot_logo_apply_supported = true;
        snapshot.last_boot_logo_applied_at_unix = Some(applied.applied_at_unix);
        snapshot.last_boot_logo_apply_detail = applied.detail.clone();
        snapshot.last_boot_logo_error = None;
        snapshot.last_boot_logo_readback = applied.readback.clone();
    })
}

pub fn persist_telemetry_settings(
    paths: &ServicePaths,
    nvidia_telemetry_enabled: bool,
) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.nvidia_telemetry_enabled = nvidia_telemetry_enabled;
    })
}

pub fn persist_fan_speed_calibration(
    paths: &ServicePaths,
    calibration: FanSpeedCalibrationSnapshot,
) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.fan_speed_calibration = calibration;
    })
}

pub fn persist_apply_error(paths: &ServicePaths, detail: &str) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.last_error = Some(detail.into());
        snapshot.last_apply_detail =
            format!("The most recent power-profile apply attempt failed. {detail}");
        snapshot.last_power_layer_statuses = vec![super::models::ControlLayerStatus::failed(
            "power-apply",
            "Power Apply",
            detail,
        )];
        snapshot.processor_state_drift_detected = false;
    })
}

pub fn persist_gpu_tuning_apply_error(paths: &ServicePaths, detail: &str) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.last_gpu_tuning_error = Some(detail.into());
        snapshot.last_gpu_tuning_detail =
            format!("The most recent GPU tuning apply attempt failed. {detail}");
    })
}

pub fn persist_fan_apply_error(paths: &ServicePaths, detail: &str) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.fan_apply_supported = false;
        snapshot.fan_curve_apply_supported = false;
        snapshot.last_fan_error = Some(detail.into());
        snapshot.last_fan_apply_detail =
            format!("The most recent fan-control apply attempt failed. {detail}");
        snapshot.last_fan_layer_statuses = vec![super::models::ControlLayerStatus::failed(
            "fan-apply",
            "Fan Apply",
            detail,
        )];
    })
}

pub fn persist_boot_logo_apply_error(paths: &ServicePaths, detail: &str) -> Result<(), DynError> {
    update_snapshot(paths, |snapshot| {
        snapshot.boot_logo_apply_supported = true;
        snapshot.last_boot_logo_error = Some(detail.into());
        snapshot.last_boot_logo_apply_detail = format!(
            "The most recent boot-logo apply attempt failed before AeroForge completed the EFI write. {detail}"
        );
    })
}

// IPC clients are handled on their own threads, so a power apply and a fan
// apply can land together; the lock keeps each read-modify-write whole so
// neither overwrites the other's fields with stale ones.
static SNAPSHOT_LOCK: Mutex<()> = Mutex::new(());

fn update_snapshot(
    paths: &ServicePaths,
    mutate: impl FnOnce(&mut ControlSnapshot),
) -> Result<(), DynError> {
    let _guard = SNAPSHOT_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut snapshot = load_snapshot(paths)?;
    mutate(&mut snapshot);
    persist_snapshot(paths, &snapshot)
}

fn persist_snapshot(paths: &ServicePaths, snapshot: &ControlSnapshot) -> Result<(), DynError> {
    crate::paths::write_text_atomically(
        &paths.worker_snapshot("control"),
        &serde_json::to_string_pretty(snapshot)?,
    )
}

fn json_without_bom(raw: &str) -> &str {
    raw.trim_start_matches('\u{feff}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workers::control::models::PowerProfileId;

    #[test]
    fn corrupt_snapshot_is_quarantined_and_defaults_load() {
        let temp = tempfile::tempdir().expect("tempdir");
        let paths = ServicePaths {
            logs_dir: temp.path().join("logs"),
            state_dir: temp.path().join("state"),
        };
        fs::create_dir_all(&paths.state_dir).expect("state dir");
        let path = paths.worker_snapshot("control");
        fs::write(&path, "{ not json").expect("corrupt");

        let loaded = load_snapshot(&paths).expect("defaults");
        assert!(matches!(
            loaded.active_power_profile,
            Some(PowerProfileId::Balanced)
        ));
        assert!(!path.exists());
        assert!(path.with_extension("json.corrupt").exists());
        persist_default_snapshot(&paths).expect("fresh default");
        assert!(load_snapshot(&paths).is_ok());
    }
}
