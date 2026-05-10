use std::{os::windows::process::CommandExt, path::Path, process::Command};

use crate::workers::control::acer_wmi;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn nvml_present() -> bool {
    Path::new(r"C:\Windows\System32\nvml.dll").exists()
}

pub fn acer_gaming_wmi_present() -> bool {
    acer_wmi::read_gaming_sys_info(0).is_ok()
        || acer_wmi::read_gaming_misc_setting(acer_wmi::MISC_SETTING_SUPPORTED_PROFILES).is_ok()
        || acer_wmi::read_firmware_sensor_snapshot().is_ok()
}

pub fn acer_fan_telemetry_present() -> bool {
    acer_wmi::read_firmware_sensor_snapshot()
        .map(|snapshot| snapshot.cpu_fan_rpm.is_some() || snapshot.gpu_fan_rpm.is_some())
        .unwrap_or(false)
}

#[derive(Default, Clone, Copy)]
pub struct BatteryControlProbe {
    pub class_present: bool,
    pub instance_present: bool,
    pub health_status_readable: bool,
}

pub fn battery_control_probe() -> BatteryControlProbe {
    let script = r#"
$class = Get-CimClass -Namespace root\wmi -ClassName BatteryControl -ErrorAction SilentlyContinue
$instance = Get-CimInstance -Namespace root\wmi -ClassName BatteryControl -ErrorAction SilentlyContinue | Select-Object -First 1
$healthReadable = $false
if ($instance) {
  try {
    Invoke-CimMethod -InputObject $instance -MethodName GetBatteryHealthControlStatus -Arguments @{
      uBatteryNo = [byte]1
      uFunctionQuery = [byte]1
      uReserved = ([byte[]](0,0))
    } -ErrorAction Stop | Out-Null
    $healthReadable = $true
  } catch {
    $healthReadable = $false
  }
}
[ordered]@{
  classPresent = [bool]$class
  instancePresent = [bool]$instance
  healthStatusReadable = [bool]$healthReadable
} | ConvertTo-Json -Compress
"#;

    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output();

    let Ok(output) = output else {
        return BatteryControlProbe::default();
    };

    if !output.status.success() {
        return BatteryControlProbe::default();
    }

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return BatteryControlProbe::default();
    };

    BatteryControlProbe {
        class_present: value
            .get("classPresent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        instance_present: value
            .get("instancePresent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        health_status_readable: value
            .get("healthStatusReadable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}
