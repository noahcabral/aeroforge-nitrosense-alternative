use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::FirmwareSensorSnapshot,
};
use crate::paths::ServicePaths;

static FIRMWARE_SENSOR_CACHE: OnceLock<Arc<Mutex<FirmwareSensorCache>>> = OnceLock::new();

struct FirmwareSensorCache {
    last_refresh: Option<Instant>,
    snapshot: FirmwareSensorSnapshot,
    refresh_in_flight: bool,
    last_error: Option<String>,
}

impl RefreshState for FirmwareSensorCache {
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

pub fn read_firmware_sensors(paths: &ServicePaths) -> FirmwareSensorSnapshot {
    let cache = FIRMWARE_SENSOR_CACHE
        .get_or_init(|| {
            Arc::new(Mutex::new(FirmwareSensorCache {
                last_refresh: None,
                snapshot: FirmwareSensorSnapshot::default(),
                refresh_in_flight: false,
                last_error: None,
            }))
        })
        .clone();

    refresh_cached_value(
        paths,
        "telemetry-firmware",
        &cache,
        std::time::Duration::from_secs(2),
        |state| state.last_refresh().is_none(),
        query_firmware_sensor_snapshot,
        |state, result| {
            if let Ok(value) = result {
                state.snapshot = *value;
            }
        },
        |state| state.snapshot,
    )
}

fn query_firmware_sensor_snapshot(
) -> Result<FirmwareSensorSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let script = r#"
$thermalZone = Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Select-Object -First 1
$result = [ordered]@{
  thermalZoneTempC = $null
  cpuFanRpm = $null
  gpuFanRpm = $null
}
if ($thermalZone -and $thermalZone.CurrentTemperature) {
  $result.thermalZoneTempC = [math]::Round(($thermalZone.CurrentTemperature / 10) - 273.15)
}
$result | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("firmware telemetry query failed: {stderr}").into());
    }

    let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout)?;
    Ok(FirmwareSensorSnapshot {
        thermal_zone_temp_c: parsed
            .get("thermalZoneTempC")
            .and_then(|value| value.as_u64())
            .map(|value| value as u8),
        cpu_fan_rpm: parsed
            .get("cpuFanRpm")
            .and_then(|value| value.as_u64())
            .map(|value| value as u16),
        gpu_fan_rpm: parsed
            .get("gpuFanRpm")
            .and_then(|value| value.as_u64())
            .map(|value| value as u16),
        has_acer_firmware: false,
        has_thermal_zone: parsed.get("thermalZoneTempC").is_some(),
    })
}
