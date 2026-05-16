use std::{
    os::windows::process::CommandExt,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use super::{
    cache::{refresh_cached_value, RefreshState},
    models::FirmwareSensorSnapshot,
};
use crate::paths::ServicePaths;
use crate::workers::control::acer_wmi;

static FIRMWARE_SENSOR_CACHE: OnceLock<Arc<Mutex<FirmwareSensorCache>>> = OnceLock::new();
static THERMAL_ZONE_CACHE: OnceLock<Arc<Mutex<ThermalZoneCache>>> = OnceLock::new();
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct ThermalZoneCache {
    last_refresh: Option<Instant>,
    value: Option<u8>,
    refresh_in_flight: bool,
    last_error: Option<String>,
}

impl RefreshState for ThermalZoneCache {
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

fn read_thermal_zone_temp_c(paths: &ServicePaths) -> Option<u8> {
    let cache = THERMAL_ZONE_CACHE
        .get_or_init(|| {
            Arc::new(Mutex::new(ThermalZoneCache {
                last_refresh: None,
                value: None,
                refresh_in_flight: false,
                last_error: None,
            }))
        })
        .clone();

    refresh_cached_value(
        paths,
        "telemetry-thermal-zone",
        &cache,
        std::time::Duration::from_secs(30),
        |state| state.last_refresh().is_none(),
        || query_windows_thermal_zone_temp_c(),
        |state, result| {
            if let Ok(value) = result {
                state.value = *value;
            }
        },
        |state| state.value,
    )
}

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

    let mut snapshot = refresh_cached_value(
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
    );

    // Thermal zone is on its own 30s cache to avoid spawning PowerShell every 2s.
    let thermal_zone = read_thermal_zone_temp_c(paths);
    snapshot.thermal_zone_temp_c = thermal_zone;
    snapshot.has_thermal_zone = thermal_zone.is_some();
    snapshot
}

fn query_firmware_sensor_snapshot(
) -> Result<FirmwareSensorSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    if let Ok(acer) = acer_wmi::read_firmware_sensor_snapshot() {
        return Ok(FirmwareSensorSnapshot {
            thermal_zone_temp_c: None,
            cpu_temp_c: to_u8(acer.cpu_temp_c),
            gpu_temp_c: to_u8(acer.gpu_temp_c),
            system_temp_c: to_u8(acer.system_temp_c),
            cpu_fan_rpm: acer.cpu_fan_rpm,
            gpu_fan_rpm: acer.gpu_fan_rpm,
            supported_sensor_mask: acer.supported_sensors,
            acer_battery_status_raw: acer.battery_status,
            has_acer_firmware: true,
            has_thermal_zone: false,
        });
    }

    Ok(FirmwareSensorSnapshot {
        thermal_zone_temp_c: None,
        cpu_temp_c: None,
        gpu_temp_c: None,
        system_temp_c: None,
        cpu_fan_rpm: None,
        gpu_fan_rpm: None,
        supported_sensor_mask: None,
        acer_battery_status_raw: None,
        has_acer_firmware: false,
        has_thermal_zone: false,
    })
}

fn query_windows_thermal_zone_temp_c(
) -> Result<Option<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let script = r#"
$thermalZone = Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction SilentlyContinue | Select-Object -First 1
$result = [ordered]@{
  thermalZoneTempC = $null
}
if ($thermalZone -and $thermalZone.CurrentTemperature) {
  $result.thermalZoneTempC = [math]::Round(($thermalZone.CurrentTemperature / 10) - 273.15)
}
$result | ConvertTo-Json -Compress
"#;

    let output = std::process::Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-NoProfile", "-Command", script])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("firmware telemetry query failed: {stderr}").into());
    }

    let parsed = serde_json::from_slice::<serde_json::Value>(&output.stdout)?;
    Ok(parsed
        .get("thermalZoneTempC")
        .and_then(|value| value.as_u64())
        .and_then(|value| u8::try_from(value).ok()))
}

fn to_u8(value: Option<u16>) -> Option<u8> {
    value.and_then(|value| u8::try_from(value).ok())
}
