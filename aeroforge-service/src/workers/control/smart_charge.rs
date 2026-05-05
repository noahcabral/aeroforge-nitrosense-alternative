use std::{io, os::windows::process::CommandExt, process::Command};

use serde::Deserialize;

use super::models::{AppliedSmartChargeSnapshot, ApplySmartChargeRequest};
use crate::workers::unix_timestamp;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatteryControlApplyOutput {
    health_status: u8,
}

pub fn apply_smart_charging(
    request: ApplySmartChargeRequest,
) -> Result<AppliedSmartChargeSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let requested_health_status = if request.enabled { 1u8 } else { 0u8 };
    let output = apply_battery_control_health_status(requested_health_status)?;
    let battery_healthy = if output.health_status == 1 { 0 } else { 1 };
    let detail = if request.enabled {
        format!(
            "Applied optimized charging through AeroForgeService direct BatteryControl WMI. Health limiter status {} keeps the 80% ceiling active.",
            output.health_status
        )
    } else {
        format!(
            "Applied full battery charging through AeroForgeService direct BatteryControl WMI. Health limiter status {} allows full charge.",
            output.health_status
        )
    };

    Ok(AppliedSmartChargeSnapshot {
        enabled: request.enabled,
        health_status: output.health_status,
        battery_healthy,
        applied_at_unix: unix_timestamp(),
        detail,
    })
}

fn apply_battery_control_health_status(
    requested_health_status: u8,
) -> Result<BatteryControlApplyOutput, Box<dyn std::error::Error + Send + Sync>> {
    let script = r#"
$status = [byte]$args[0]
$battery = Get-CimInstance -Namespace root\wmi -ClassName BatteryControl -ErrorAction Stop | Select-Object -First 1
if (-not $battery) { throw 'BatteryControl instance was not found.' }

function Read-HealthStatus {
  param($Battery)
  $get = Invoke-CimMethod -InputObject $Battery -MethodName GetBatteryHealthControlStatus -Arguments @{
    uBatteryNo = [byte]1
    uFunctionQuery = [byte]1
    uReserved = ([byte[]](0,0))
  } -ErrorAction Stop
  return $get
}

$attempts = @(
  @{
    Name = 'array-status'
    Arguments = @{
      uBatteryNo = [byte]1
      uFunctionMask = [byte]1
      uFunctionStatus = ([byte[]]($status,0,0,0,0))
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  },
  @{
    Name = 'scalar-status'
    Arguments = @{
      uBatteryNo = [byte]1
      uFunctionMask = [byte]1
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  }
)

$errors = New-Object System.Collections.Generic.List[string]
foreach ($attempt in $attempts) {
  try {
    $set = Invoke-CimMethod -InputObject $battery -MethodName SetBatteryHealthControl -Arguments $attempt.Arguments -ErrorAction Stop
    Start-Sleep -Milliseconds 250
    $get = Read-HealthStatus -Battery $battery
    $health = [int]$get.uFunctionStatus[0]
    if ($health -eq [int]$status) {
      [ordered]@{
        requestedHealthStatus = [int]$status
        healthStatus = $health
        setAttempt = $attempt.Name
        functionList = [int]$get.uFunctionList
        functionStatus = @($get.uFunctionStatus)
        getReturn = @($get.uReturn)
        setReturn = @($set.uReturn)
        setReservedOut = @($set.uReservedOut)
      } | ConvertTo-Json -Compress
      exit 0
    }
    $errors.Add(('{0}: readback returned {1} after requesting {2}' -f $attempt.Name, $health, [int]$status))
  } catch {
    $errors.Add(('{0}: {1}' -f $attempt.Name, $_.Exception.Message))
  }
}

throw ('BatteryControl direct apply failed. ' + ($errors -join ' | '))
"#;

    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
            &requested_health_status.to_string(),
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("PowerShell exited with status {}", output.status)
        };
        return Err(
            io::Error::other(format!("BatteryControl service apply failed: {detail}")).into(),
        );
    }

    let parsed = serde_json::from_slice::<BatteryControlApplyOutput>(&output.stdout)?;
    if parsed.health_status != requested_health_status {
        return Err(io::Error::other(format!(
            "BatteryControl returned healthStatus {} after requesting {}.",
            parsed.health_status, requested_health_status
        ))
        .into());
    }

    Ok(parsed)
}
