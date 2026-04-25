use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySnapshot {
    pub cpu_temp_c: u8,
    pub cpu_temp_average_c: Option<u8>,
    pub cpu_temp_lowest_core_c: Option<u8>,
    pub cpu_temp_highest_core_c: Option<u8>,
    pub gpu_temp_c: u8,
    pub system_temp_c: u8,
    pub cpu_usage_percent: u8,
    pub gpu_usage_percent: u8,
    pub gpu_memory_usage_percent: Option<u8>,
    pub cpu_name: Option<String>,
    pub cpu_brand: Option<String>,
    pub gpu_name: Option<String>,
    pub gpu_brand: Option<String>,
    pub system_vendor: Option<String>,
    pub system_model: Option<String>,
    pub cpu_clock_mhz: u16,
    pub gpu_clock_mhz: u16,
    pub cpu_fan_rpm: u16,
    pub gpu_fan_rpm: u16,
    pub battery_percent: u8,
    pub battery_life_remaining_sec: Option<u32>,
    pub ac_plugged_in: bool,
    pub heartbeat: u64,
}

#[derive(Default, Clone, Copy)]
pub struct PowerSnapshot {
    pub battery_percent: u8,
    pub battery_life_remaining_sec: Option<u32>,
    pub ac_plugged_in: bool,
}

#[derive(Default, Clone, Copy)]
pub struct GpuSnapshot {
    pub usage_percent: Option<u8>,
    pub memory_usage_percent: Option<u8>,
    pub temp_c: Option<u8>,
    pub clock_mhz: Option<u16>,
}

#[derive(Default, Clone, Copy)]
pub struct CpuThermalSnapshot {
    pub average_temp_c: Option<u8>,
    pub lowest_core_temp_c: Option<u8>,
    pub highest_core_temp_c: Option<u8>,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LowLevelSnapshot {
    pub available: bool,
    pub package_temp_c: Option<u8>,
    pub average_core_temp_c: Option<u8>,
    pub lowest_core_temp_c: Option<u8>,
    pub highest_core_temp_c: Option<u8>,
}

#[derive(Default, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareSensorSnapshot {
    pub thermal_zone_temp_c: Option<u8>,
    pub cpu_fan_rpm: Option<u16>,
    pub gpu_fan_rpm: Option<u16>,
    pub has_acer_firmware: bool,
    pub has_thermal_zone: bool,
}

#[derive(Default, Clone, Copy)]
pub struct AcerHidStatusSnapshot {
    pub cpu_fan_rpm: Option<u16>,
    pub gpu_fan_rpm: Option<u16>,
    pub cpu_temp_c: Option<u8>,
    pub system_temp_c: Option<u8>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareIdentitySnapshot {
    pub cpu_name: Option<String>,
    pub cpu_brand: Option<String>,
    pub gpu_name: Option<String>,
    pub gpu_brand: Option<String>,
    pub system_vendor: Option<String>,
    pub system_model: Option<String>,
}
