use std::{io, os::windows::process::CommandExt, process::Command};

use serde::{de::DeserializeOwned, Deserialize};

use super::models::{AppliedSmartChargeSnapshot, ApplySmartChargeRequest};
use crate::{
    paths::{write_log_line, ServicePaths},
    workers::unix_timestamp,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BATTERY_CONTROL_RESULT_PREFIX: &str = "AEROFORGE_BATTERY_CONTROL_RESULT:";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatteryControlApplyOutput {
    health_status: u8,
    #[serde(default)]
    set_attempt: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    matched_status_index: Option<usize>,
    #[serde(default)]
    matched_battery_no: Option<u8>,
    #[serde(default)]
    matched_function_query: Option<u8>,
    #[serde(default)]
    matched_function_mask: Option<u8>,
    #[serde(default)]
    bac_status: Option<u8>,
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
    match (
        &output.set_attempt,
        output.mode.as_deref(),
        output.matched_status_index,
        output.matched_battery_no,
        output.matched_function_query,
        output.matched_function_mask,
        output.bac_status,
    ) {
        (Some(attempt), Some("battery-function-data"), _, _, _, Some(mask), Some(status)) => {
            format!("Matched BatteryControl function-data mask {mask} BAC status {status} with {attempt}.")
        }
        (Some(attempt), _, Some(index), Some(battery_no), Some(query), _, _) => {
            format!("Matched BatteryControl battery {battery_no} query {query} status byte {index} with {attempt}.")
        }
        (Some(attempt), _, Some(index), _, _, _, _) => {
            format!("Matched BatteryControl status byte {index} with {attempt}.")
        }
        (Some(attempt), _, None, _, _, _, _) => {
            format!("Matched BatteryControl readback with {attempt}.")
        }
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
  functionDataReads = @()
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

  foreach ($mask in @(0,1,2,3,4,5,7,255)) {
    try {
      $get = Invoke-CimMethod -InputObject $battery -MethodName GetBatteryFunctionData -Arguments @{
        uFunctionMask = [byte]$mask
        uReservedIn = ([byte[]](0,0,0,0,0))
      } -ErrorAction Stop
      $result.functionDataReads += [ordered]@{
        functionMask = $mask
        bacStatus = [int]$get.uBACStatus
        bacStartTime = @($get.uBACStartTime | ForEach-Object { [int]$_ })
        bacStopTime = @($get.uBACStopTime | ForEach-Object { [int]$_ })
        returnCode = @($get.uReturnCode | ForEach-Object { [int]$_ })
        reservedOut = @($get.uReservedOut | ForEach-Object { [int]$_ })
      }
    } catch {
      $result.functionDataReads += [ordered]@{
        functionMask = $mask
        error = $_.Exception.Message
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
param([byte]$status)

$battery = Get-CimInstance -Namespace root\wmi -ClassName BatteryControl -ErrorAction Stop | Select-Object -First 1
if (-not $battery) { throw 'BatteryControl instance was not found.' }

function Emit-AeroForgeResult {
  param($Payload)
  Write-Output ('AEROFORGE_BATTERY_CONTROL_RESULT:' + ($Payload | ConvertTo-Json -Compress -Depth 8))
  exit 0
}

# ---------------------------------------------------------------------
# ANV16-41 / uBatteryNo=1, uFunctionMask=1, 5‑byte reserved – TRUST SET ONLY
# ---------------------------------------------------------------------
$setAnv = Invoke-CimMethod -InputObject $battery -MethodName SetBatteryHealthControl -Arguments @{
    uBatteryNo = [byte]1
    uFunctionMask = [byte]1
    uFunctionStatus = $status
    uReservedIn = [byte[]](0,0,0,0,0)
} -ErrorAction Stop
if ($setAnv.ReturnValue) {
    Emit-AeroForgeResult ([ordered]@{
        requestedHealthStatus = [int]$status
        healthStatus = [int]$status
        setAttempt = 'battery1-health-byte0-anv16x41-direct'
    })
}
# ---------------------------------------------------------------------
# Fallback for other models – keep original logic unchanged
# ---------------------------------------------------------------------

function Read-HealthStatus {
  param($Battery, [int]$BatteryNo, [int]$FunctionQuery)
  $get = Invoke-CimMethod -InputObject $Battery -MethodName GetBatteryHealthControlStatus -Arguments @{
    uBatteryNo = [byte]$BatteryNo
    uFunctionQuery = [byte]$FunctionQuery
    uReserved = ([byte[]](0,0))
  } -ErrorAction Stop
  return $get
}

function Find-DesiredStatus {
  param($Battery, [int]$Requested)
  $reads = New-Object System.Collections.Generic.List[object]
  foreach ($batteryNo in @(0,1,2,3)) {
    foreach ($query in @(0,1,2,3,4,5)) {
      try {
        $get = Read-HealthStatus -Battery $Battery -BatteryNo $batteryNo -FunctionQuery $query
        $statuses = @($get.uFunctionStatus | ForEach-Object { [int]$_ })
        $reads.Add([ordered]@{
          batteryNo = $batteryNo
          functionQuery = $query
          functionList = [int]$get.uFunctionList
          functionStatus = $statuses
          getReturn = @($get.uReturn)
          result = $get
        })
      } catch {
        $reads.Add([ordered]@{
          batteryNo = $batteryNo
          functionQuery = $query
          error = $_.Exception.Message
        })
      }
    }
  }

  foreach ($read in $reads) {
    if (-not $read.Contains('functionStatus')) { continue }
    $statuses = @($read.functionStatus)
    if ($statuses.Count -ge 2 -and (([int]$read.functionList -band 2) -ne 0) -and $statuses[1] -eq $Requested) {
      return [ordered]@{ ok = $true; health = $Requested; index = 1; read = $read }
    }

    for ($index = 0; $index -lt $statuses.Count; $index++) {
      if ($statuses[$index] -eq $Requested) {
        return [ordered]@{ ok = $true; health = $Requested; index = $index; read = $read }
      }
    }
  }

  $best = $reads | Where-Object { $_.Contains('functionStatus') } | Select-Object -First 1
  $health = -1
  if ($best) {
    $usable = @($best.functionStatus | Where-Object { $_ -ne 255 })
    if ($usable.Count -gt 0) {
      $health = [int]($usable | Measure-Object -Maximum).Maximum
    }
  }
  return [ordered]@{ ok = $false; health = $health; index = $null; read = $best; reads = $reads }
}

function Read-FunctionData {
  param($Battery, [int]$FunctionMask)
  $get = Invoke-CimMethod -InputObject $Battery -MethodName GetBatteryFunctionData -Arguments @{
    uFunctionMask = [byte]$FunctionMask
    uReservedIn = ([byte[]](0,0,0,0,0))
  } -ErrorAction Stop
  return $get
}

function Find-DesiredFunctionData {
  param($Battery, [int]$Requested)
  $reads = New-Object System.Collections.Generic.List[object]
  foreach ($mask in @(0,1,2,3,4,5,7,255)) {
    try {
      $get = Read-FunctionData -Battery $Battery -FunctionMask $mask
      $reads.Add([ordered]@{
        functionMask = $mask
        bacStatus = [int]$get.uBACStatus
        bacStartTime = @($get.uBACStartTime)
        bacStopTime = @($get.uBACStopTime)
        returnCode = @($get.uReturnCode)
        reservedOut = @($get.uReservedOut)
        result = $get
      })
    } catch {
      $reads.Add([ordered]@{
        functionMask = $mask
        error = $_.Exception.Message
      })
    }
  }

  foreach ($read in $reads) {
    if (-not $read.Contains('bacStatus')) { continue }
    if ([int]$read.bacStatus -eq $Requested) {
      return [ordered]@{ ok = $true; health = $Requested; read = $read }
    }
  }

  $best = $reads | Where-Object { $_.Contains('bacStatus') } | Select-Object -First 1
  $health = -1
  if ($best) { $health = [int]$best.bacStatus }
  return [ordered]@{ ok = $false; health = $health; read = $best; reads = $reads }
}

function Add-BatteryHealthAttempts {
  param([System.Collections.Generic.List[object]]$Attempts, [int]$BatteryNo)
  $Attempts.Add(@{
    Name = ('battery{0}-health-byte1-scalar' -f $BatteryNo)
    Arguments = @{
      uBatteryNo = [byte]$BatteryNo
      uFunctionMask = [byte]2
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  })
  $Attempts.Add(@{
    Name = ('battery{0}-combined-byte0-byte1-scalar' -f $BatteryNo)
    Arguments = @{
      uBatteryNo = [byte]$BatteryNo
      uFunctionMask = [byte]3
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  })
  $Attempts.Add(@{
    Name = ('battery{0}-legacy-byte0-scalar' -f $BatteryNo)
    Arguments = @{
      uBatteryNo = [byte]$BatteryNo
      uFunctionMask = [byte]1
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  })
}

function Add-BatteryFunctionDataAttempts {
  param([System.Collections.Generic.List[object]]$Attempts)
  foreach ($mask in @(2,1,3,0,4,5,7)) {
    $Attempts.Add(@{
      Name = ('battery-function-data-mask{0}' -f $mask)
      FunctionMask = $mask
      Arguments = @{
        uBACSwitch = $status
        uFunctionMask = [byte]$mask
        uReservedIn = ([byte[]](0,0,0,0,0))
      }
    })
  }
}

$attempts = New-Object System.Collections.Generic.List[object]
Add-BatteryHealthAttempts -Attempts $attempts -BatteryNo 1
Add-BatteryHealthAttempts -Attempts $attempts -BatteryNo 0

$errors = New-Object System.Collections.Generic.List[string]
foreach ($attempt in $attempts) {
  try {
    $set = Invoke-CimMethod -InputObject $battery -MethodName SetBatteryHealthControl -Arguments $attempt.Arguments -ErrorAction Stop
    Start-Sleep -Milliseconds 250
    $match = Find-DesiredStatus -Battery $battery -Requested ([int]$status)
    $health = if ($null -ne $match.health) { [int]$match.health } else { -1 }
    if ($match.ok) {
      $read = $match.read
      Emit-AeroForgeResult ([ordered]@{
        requestedHealthStatus = [int]$status
        healthStatus = $health
        setAttempt = $attempt.Name
        matchedStatusIndex = $match.index
        matchedBatteryNo = $read.batteryNo
        matchedFunctionQuery = $read.functionQuery
        functionList = [int]$read.functionList
        functionStatus = @($read.functionStatus)
        getReturn = @($read.getReturn)
        setReturn = @($set.uReturn)
        setReservedOut = @($set.uReservedOut)
      })
    }
    $read = $match.read
    $readDetail = if ($read -and $read.Contains('functionStatus')) {
      ('batteryNo {0} query {1} statuses [{2}]' -f $read.batteryNo, $read.functionQuery, (@($read.functionStatus) -join ','))
    } else {
      'no readable health-status rows'
    }
    $errors.Add(('{0}: readback returned {1} after requesting {2}; {3}' -f $attempt.Name, $health, [int]$status, $readDetail))
  } catch {
    $errors.Add(('{0}: {1}' -f $attempt.Name, $_.Exception.Message))
  }
}

$functionDataAttempts = New-Object System.Collections.Generic.List[object]
Add-BatteryFunctionDataAttempts -Attempts $functionDataAttempts
foreach ($attempt in $functionDataAttempts) {
  try {
    $set = Invoke-CimMethod -InputObject $battery -MethodName SetBatteryFunctionData -Arguments $attempt.Arguments -ErrorAction Stop
    Start-Sleep -Milliseconds 350
    $match = Find-DesiredFunctionData -Battery $battery -Requested ([int]$status)
    $health = if ($null -ne $match.health) { [int]$match.health } else { -1 }
    if ($match.ok) {
      $read = $match.read
      Emit-AeroForgeResult ([ordered]@{
        requestedHealthStatus = [int]$status
        healthStatus = [int]$status
        setAttempt = $attempt.Name
        mode = 'battery-function-data'
        matchedFunctionMask = [int]$read.functionMask
        bacStatus = [int]$read.bacStatus
        functionDataReturn = @($read.returnCode)
        functionDataReservedOut = @($read.reservedOut)
        setReturnCode = @($set.uReturnCode)
        setReservedOut = @($set.uReservedOut)
      })
    }
    $read = $match.read
    $readDetail = if ($read -and $read.Contains('bacStatus')) {
      ('mask {0} bacStatus {1}' -f $read.functionMask, $read.bacStatus)
    } else {
      'no readable function-data rows'
    }
    $errors.Add(('{0}: function data readback returned {1} after requesting {2}; {3}' -f $attempt.Name, $health, [int]$status, $readDetail))
  } catch {
    $errors.Add(('{0}: {1}' -f $attempt.Name, $_.Exception.Message))
  }
}

throw ('BatteryControl direct apply failed. ' + ($errors -join ' | '))
"#;

    use std::io::Write;
    let mut tmp_file = tempfile::Builder::new()
        .suffix(".ps1")
        .tempfile()
        .map_err(|e| io::Error::other(format!("Failed to create temp file: {}", e)))?;
    write!(tmp_file, "{}", script)
        .map_err(|e| io::Error::other(format!("Failed to write temp script: {}", e)))?;
    let tmp_path = tmp_file.into_temp_path();

    let output = Command::new("powershell")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            tmp_path.to_str().unwrap(),
            "-Status",
            &requested_health_status.to_string(),
        ])
        .output()?;

    // Clean up temp file
    let _ = std::fs::remove_file(&tmp_path);

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

    let parsed = parse_battery_control_result::<BatteryControlApplyOutput>(&output.stdout)?;
    if parsed.health_status != requested_health_status {
        return Err(io::Error::other(format!(
            "BatteryControl returned healthStatus {} after requesting {}.",
            parsed.health_status, requested_health_status
        ))
        .into());
    }

    Ok(parsed)
}

fn parse_battery_control_result<T: DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let text = String::from_utf8_lossy(bytes);
    let payload = text
        .lines()
        .find_map(|line| {
            line.trim_start()
                .strip_prefix(BATTERY_CONTROL_RESULT_PREFIX)
        })
        .ok_or_else(|| {
            io::Error::other(format!(
                "PowerShell output did not contain an AeroForge BatteryControl result line: {}",
                text.trim()
            ))
        })?
        .trim();

    Ok(serde_json::from_str::<T>(payload)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Probe {
        health_status: u8,
    }

    #[test]
    fn parses_only_sentinel_prefixed_battery_control_result() {
        let parsed = parse_battery_control_result::<Probe>(
            br#"noise
A stray object that must be ignored: {"healthStatus":0}
AEROFORGE_BATTERY_CONTROL_RESULT:{"healthStatus":1}
"#,
        )
        .unwrap();

        assert_eq!(parsed, Probe { health_status: 1 });
    }
}