use crate::paths::ServicePaths;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

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

    let mut last_error: Option<String> = None;
    for attempt in 0..6 {
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
                last_error = Some(format!("control snapshot could not be read: {error}"));
            }
        }

        if attempt < 5 {
            thread::sleep(Duration::from_millis(50));
        }
    }

    Err(last_error
        .unwrap_or_else(|| "control snapshot could not be loaded".into())
        .into())
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
    snapshot.custom_base_profile = applied.custom_base_profile.clone();
    snapshot.processor_state_control_enabled = applied.processor_state_control_enabled;
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
    snapshot.current_cpu_fan_speed_percent = applied.cpu_speed_percent;
    snapshot.current_gpu_fan_speed_percent = applied.gpu_speed_percent;
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

pub fn persist_telemetry_settings(
    paths: &ServicePaths,
    nvidia_telemetry_enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.nvidia_telemetry_enabled = nvidia_telemetry_enabled;
    persist_snapshot(paths, &snapshot)
}

pub fn persist_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_error = Some(detail.into());
    snapshot.last_apply_detail =
        format!("The most recent power-profile apply attempt failed. {detail}");
    snapshot.processor_state_drift_detected = false;
    persist_snapshot(paths, &snapshot)
}

pub fn persist_gpu_tuning_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_gpu_tuning_error = Some(detail.into());
    snapshot.last_gpu_tuning_detail =
        format!("The most recent GPU tuning apply attempt failed. {detail}");
    persist_snapshot(paths, &snapshot)
}

pub fn persist_fan_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.last_fan_error = Some(detail.into());
    snapshot.last_fan_apply_detail =
        format!("The most recent fan-control apply attempt failed. {detail}");
    persist_snapshot(paths, &snapshot)
}

pub fn persist_boot_logo_apply_error(
    paths: &ServicePaths,
    detail: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut snapshot = load_snapshot(paths)?;
    snapshot.boot_logo_apply_supported = true;
    snapshot.last_boot_logo_error = Some(detail.into());
    snapshot.last_boot_logo_apply_detail = format!(
        "The most recent boot-logo apply attempt failed before AeroForge completed the EFI write. {detail}"
    );
    persist_snapshot(paths, &snapshot)
}

fn persist_snapshot(
    paths: &ServicePaths,
    snapshot: &ControlSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = paths.worker_snapshot("control");
    write_snapshot_atomically(&path, &serde_json::to_string_pretty(snapshot)?)?;
    Ok(())
}

fn write_snapshot_atomically(
    path: &Path,
    content: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let temp_path = temp_snapshot_path(path);
    {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
    }

    replace_file(&temp_path, path)?;
    Ok(())
}

fn json_without_bom(raw: &str) -> &str {
    raw.trim_start_matches('\u{feff}')
}

fn temp_snapshot_path(path: &Path) -> PathBuf {
    let mut temp_path = path.to_path_buf();
    let extension = format!(
        "tmp.{}.{}",
        std::process::id(),
        crate::workers::unix_timestamp()
    );
    temp_path.set_extension(extension);
    temp_path
}

#[cfg(windows)]
fn replace_file(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let source_w = wide(source);
    let target_w = wide(target);
    let moved = unsafe {
        MoveFileExW(
            source_w.as_ptr(),
            target_w.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if moved == 0 {
        let error = std::io::Error::last_os_error();
        let _ = fs::remove_file(source);
        return Err(error.into());
    }

    Ok(())
}

#[cfg(not(windows))]
fn replace_file(
    source: &Path,
    target: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    fs::rename(source, target)?;
    Ok(())
}
