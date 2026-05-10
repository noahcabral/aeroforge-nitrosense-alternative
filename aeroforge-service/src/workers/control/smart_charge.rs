use std::{io, os::windows::process::CommandExt, process::Command};

use serde::Deserialize;

use super::models::{AppliedSmartChargeSnapshot, ApplySmartChargeRequest};
use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatteryControlApplyOutput {
    health_status: u8,
    #[serde(default)]
    set_attempt: Option<String>,
    #[serde(default)]
    matched_status_index: Option<usize>,
}

pub fn apply_smart_charging(
    paths: &ServicePaths,
    request: ApplySmartChargeRequest,
) -> Result<AppliedSmartChargeSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let requested_health_status = if request.enabled { 1u8 } else { 0u8 };
    log_battery_control_snapshot(paths, "before-apply", requested_health_status);
    let output = match apply_battery_control_health_status(requested_health_status) {
        Ok(output) => output,
        Err(error) => {
            log_battery_control_snapshot(paths, "after-failed-apply", requested_health_status);
            return Err(error);
        }
    };
    log_battery_control_snapshot(paths, "after-apply", requested_health_status);
    let battery_healthy = if output.health_status == 1 { 0 } else { 1 };
    let detail = if request.enabled {
        format!(
            "Applied optimized charging through AeroForgeService direct BatteryControl WMI. Health limiter status {} keeps the 80% ceiling active. {}",
            output.health_status,
            battery_control_attempt_detail(&output)
        )
    } else {
        format!(
            "Applied full battery charging through AeroForgeService direct BatteryControl WMI. Health limiter status {} allows full charge. {}",
            output.health_status,
            battery_control_attempt_detail(&output)
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

fn battery_control_attempt_detail(output: &BatteryControlApplyOutput) -> String {
    match (&output.set_attempt, output.matched_status_index) {
        (Some(attempt), Some(index)) => {
            format!("Matched BatteryControl status byte {index} with {attempt}.")
        }
        (Some(attempt), None) => format!("Matched BatteryControl readback with {attempt}."),
        _ => "Matched BatteryControl readback.".into(),
    }
}

fn log_battery_control_snapshot(paths: &ServicePaths, phase: &str, requested_health_status: u8) {
    let log_path = paths.component_log("control-smart-charge");
    match read_battery_control_snapshot(requested_health_status) {
        Ok(snapshot) => {
            let _ = write_log_line(
                &log_path,
                "INFO",
                &format!("BatteryControl raw snapshot {phase}: {snapshot}"),
            );
        }
        Err(error) => {
            let _ = write_log_line(
                &log_path,
                "WARN",
                &format!("BatteryControl raw snapshot {phase} failed: {error}"),
            );
        }
    }
}

fn read_battery_control_snapshot(
    requested_health_status: u8,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let script = r#"
$requested = [int]$args[0]
$result = [ordered]@{
  requestedHealthStatus = $requested
  isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  classes = @()
  instances = @()
  statusReads = @()
  errors = @()
}

try {
  $classes = @(Get-CimClass -Namespace root\wmi -ClassName BatteryControl -ErrorAction Stop)
  $result.classes = @($classes | ForEach-Object {
    [ordered]@{
      name = $_.CimClassName
      methods = @($_.CimClassMethods | ForEach-Object {
        [ordered]@{
          name = $_.Name
          parameters = @($_.Parameters | ForEach-Object {
            [ordered]@{
              name = $_.Name
              type = $_.CimType.ToString()
              qualifiers = @($_.Qualifiers | ForEach-Object { $_.Name })
            }
          })
        }
      })
      properties = @($_.CimClassProperties | ForEach-Object {
        [ordered]@{
          name = $_.Name
          type = $_.CimType.ToString()
          qualifiers = @($_.Qualifiers | ForEach-Object { $_.Name })
        }
      })
    }
  })
} catch {
  $result.errors += ('Get-CimClass BatteryControl: ' + $_.Exception.Message)
}

$instances = @()
try {
  $instances = @(Get-CimInstance -Namespace root\wmi -ClassName BatteryControl -ErrorAction Stop)
  $index = 0
  $result.instances = @($instances | ForEach-Object {
    $props = @{}
    foreach ($property in $_.CimInstanceProperties) {
      $props[$property.Name] = $property.Value
    }
    [ordered]@{
      index = $index++
      properties = $props
    }
  })
} catch {
  $result.errors += ('Get-CimInstance BatteryControl: ' + $_.Exception.Message)
}

if ($instances.Count -gt 0) {
  $battery = $instances | Select-Object -First 1
  foreach ($batteryNo in @(0,1,2,3)) {
    foreach ($query in @(0,1,2,3)) {
      try {
        $get = Invoke-CimMethod -InputObject $battery -MethodName GetBatteryHealthControlStatus -Arguments @{
          uBatteryNo = [byte]$batteryNo
          uFunctionQuery = [byte]$query
          uReserved = ([byte[]](0,0))
        } -ErrorAction Stop
        $result.statusReads += [ordered]@{
          batteryNo = $batteryNo
          functionQuery = $query
          functionList = [int]$get.uFunctionList
          functionStatus = @($get.uFunctionStatus | ForEach-Object { [int]$_ })
          'return' = @($get.uReturn | ForEach-Object { [int]$_ })
        }
      } catch {
        $result.statusReads += [ordered]@{
          batteryNo = $batteryNo
          functionQuery = $query
          error = $_.Exception.Message
        }
      }
    }
  }
}

$result | ConvertTo-Json -Depth 8 -Compress
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
        return Err(io::Error::other(detail).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

function New-StatusBytes {
  param([int]$First, [int]$Second)
  return ([byte[]]@([byte]$First,[byte]$Second,0,0,0))
}

function Test-DesiredStatus {
  param($GetResult, [int]$Requested)
  $statuses = @($GetResult.uFunctionStatus | ForEach-Object { [int]$_ })
  if ($statuses.Count -lt 2) {
    return [ordered]@{ ok = $false; health = $null; index = $null }
  }

  if ($Requested -eq 0) {
    if ($statuses[0] -eq 0 -and $statuses[1] -eq 0) {
      return [ordered]@{ ok = $true; health = 0; index = 0 }
    }
    return [ordered]@{ ok = $false; health = [Math]::Max($statuses[0], $statuses[1]); index = $null }
  }

  foreach ($index in @(0,1)) {
    if ($statuses[$index] -eq $Requested) {
      return [ordered]@{ ok = $true; health = $Requested; index = $index }
    }
  }

  return [ordered]@{ ok = $false; health = [Math]::Max($statuses[0], $statuses[1]); index = $null }
}

$attempts = @(
  @{
    Name = 'legacy-byte0-array'
    Arguments = @{
      uBatteryNo = [byte]1
      uFunctionMask = [byte]1
      uFunctionStatus = (New-StatusBytes -First $status -Second 0)
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  },
  @{
    Name = 'legacy-byte0-scalar'
    Arguments = @{
      uBatteryNo = [byte]1
      uFunctionMask = [byte]1
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  },
  @{
    Name = 'battery-health-byte1-array'
    Arguments = @{
      uBatteryNo = [byte]1
      uFunctionMask = [byte]2
      uFunctionStatus = (New-StatusBytes -First 0 -Second $status)
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  },
  @{
    Name = 'combined-byte0-byte1-array'
    Arguments = @{
      uBatteryNo = [byte]1
      uFunctionMask = [byte]3
      uFunctionStatus = (New-StatusBytes -First $status -Second $status)
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
    $match = Test-DesiredStatus -GetResult $get -Requested ([int]$status)
    $health = if ($null -ne $match.health) { [int]$match.health } else { -1 }
    if ($match.ok) {
      [ordered]@{
        requestedHealthStatus = [int]$status
        healthStatus = $health
        setAttempt = $attempt.Name
        matchedStatusIndex = $match.index
        functionList = [int]$get.uFunctionList
        functionStatus = @($get.uFunctionStatus)
        getReturn = @($get.uReturn)
        setReturn = @($set.uReturn)
        setReservedOut = @($set.uReservedOut)
      } | ConvertTo-Json -Compress
      exit 0
    }
    $errors.Add(('{0}: readback returned {1} with statuses [{2}] after requesting {3}' -f $attempt.Name, $health, (@($get.uFunctionStatus) -join ','), [int]$status))
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
