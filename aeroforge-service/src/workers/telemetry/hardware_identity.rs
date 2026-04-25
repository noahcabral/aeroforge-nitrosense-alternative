use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::HardwareIdentitySnapshot,
};
use crate::paths::ServicePaths;

static HARDWARE_IDENTITY_CACHE: OnceLock<Arc<Mutex<HardwareIdentityCache>>> = OnceLock::new();

struct HardwareIdentityCache {
    last_refresh: Option<Instant>,
    snapshot: HardwareIdentitySnapshot,
    refresh_in_flight: bool,
    last_error: Option<String>,
}

impl RefreshState for HardwareIdentityCache {
    fn last_refresh(&self) -> Option<Instant> {
        self.last_refresh
    }

    fn set_last_refresh(&mut self, value: Option<Instant>) {
        self.last_refresh = value;
    }

    fn refresh_in_flight(&self) -> bool {
        self.refresh_in_flight
    }

    fn set_refresh_in_flight(&mut self, value: bool) {
        self.refresh_in_flight = value;
    }

    fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn set_last_error(&mut self, value: Option<String>) {
        self.last_error = value;
    }
}

pub fn read_hardware_identity(paths: &ServicePaths) -> HardwareIdentitySnapshot {
    let cache = HARDWARE_IDENTITY_CACHE
        .get_or_init(|| {
            Arc::new(Mutex::new(HardwareIdentityCache {
                last_refresh: None,
                snapshot: HardwareIdentitySnapshot::default(),
                refresh_in_flight: false,
                last_error: None,
            }))
        })
        .clone();

    refresh_cached_value(
        paths,
        "telemetry-identity",
        &cache,
        std::time::Duration::from_secs(15),
        |state| state.last_refresh().is_none(),
        query_hardware_identity_snapshot,
        |state, result| {
            if let Ok(value) = result {
                state.snapshot = value.clone();
            }
        },
        |state| state.snapshot.clone(),
    )
}

fn query_hardware_identity_snapshot(
) -> Result<HardwareIdentitySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let script = r#"
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1 Name, Manufacturer
$gpus = Get-CimInstance Win32_VideoController | Where-Object { $_.Name -and $_.PNPDeviceID -notmatch '^ROOT\\' }
$gpu = $gpus | Where-Object { $_.Name -match 'NVIDIA|GeForce|RTX|GTX|AMD Radeon|Radeon' } | Select-Object -First 1
if (-not $gpu) {
  $gpu = $gpus | Where-Object { $_.Name -notmatch '^Intel\(R\)' -and $_.AdapterCompatibility -notmatch '^Intel' } | Select-Object -First 1
}
if (-not $gpu) {
  $gpu = $gpus | Select-Object -First 1
}
$system = Get-CimInstance Win32_ComputerSystem | Select-Object -First 1 Manufacturer, Model
[ordered]@{
  cpuName = $cpu.Name
  cpuBrand = $cpu.Manufacturer
  gpuName = $gpu.Name
  gpuBrand = $gpu.AdapterCompatibility
  systemVendor = $system.Manufacturer
  systemModel = $system.Model
} | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("hardware identity query failed: {stderr}").into());
    }

    let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout)?;
    Ok(HardwareIdentitySnapshot {
        cpu_name: parsed
            .get("cpuName")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        cpu_brand: parsed
            .get("cpuBrand")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        gpu_name: parsed
            .get("gpuName")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        gpu_brand: parsed
            .get("gpuBrand")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        system_vendor: parsed
            .get("systemVendor")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        system_model: parsed
            .get("systemModel")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    })
}
