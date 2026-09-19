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
        std::time::Duration::from_secs(60 * 30),
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
$gpu = $gpus | Where-Object { $_.Name -match 'NVIDIA|GeForce|RTX|GTX' -or $_.AdapterCompatibility -match 'NVIDIA' } | Select-Object -First 1
if (-not $gpu) {
  $gpu = $gpus | Where-Object { $_.Name -match 'AMD Radeon|Radeon' -or $_.AdapterCompatibility -match 'AMD|Advanced Micro Devices' } | Select-Object -First 1
}
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

    let output = crate::paths::system32_command("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return query_registry_hardware_identity_snapshot().map_err(|fallback_error| {
            Box::<dyn std::error::Error + Send + Sync>::from(format!(
                "hardware identity query failed: {stderr}; registry fallback failed: {fallback_error}"
            ))
        });
    }

    let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout)?;
    let field = |key: &str| {
        parsed
            .get(key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    };
    Ok(HardwareIdentitySnapshot {
        cpu_name: field("cpuName"),
        cpu_brand: field("cpuBrand"),
        gpu_name: field("gpuName"),
        gpu_brand: field("gpuBrand"),
        system_vendor: field("systemVendor"),
        system_model: field("systemModel"),
    })
}

fn query_registry_hardware_identity_snapshot(
) -> Result<HardwareIdentitySnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let output = crate::paths::system32_command("reg")
        .args(["query", r"HKLM\HARDWARE\DESCRIPTION\System\BIOS"])
        .output()?;

    if !output.status.success() {
        return Err(format!("reg.exe exited with status {}", output.status).into());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(HardwareIdentitySnapshot {
        cpu_name: None,
        cpu_brand: None,
        gpu_name: None,
        gpu_brand: None,
        system_vendor: parse_reg_value(&raw, "SystemManufacturer"),
        system_model: parse_reg_value(&raw, "SystemProductName"),
    })
}

fn parse_reg_value(raw: &str, name: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with(name) {
            return None;
        }
        let mut parts = trimmed.split_whitespace();
        parts.next()?;
        parts.next()?;
        let value = parts.collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::parse_reg_value;

    #[test]
    fn parses_registry_hardware_identity() {
        let raw = r#"
    SystemManufacturer    REG_SZ    Acer
    SystemProductName    REG_SZ    Nitro ANV15-51
    BIOSVersion    REG_SZ    V1.60
"#;

        assert_eq!(
            parse_reg_value(raw, "SystemManufacturer").as_deref(),
            Some("Acer")
        );
        assert_eq!(
            parse_reg_value(raw, "SystemProductName").as_deref(),
            Some("Nitro ANV15-51")
        );
        assert_eq!(
            parse_reg_value(raw, "BIOSVersion").as_deref(),
            Some("V1.60")
        );
    }
}
