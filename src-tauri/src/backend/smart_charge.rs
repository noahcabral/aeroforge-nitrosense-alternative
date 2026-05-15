use std::{
    fs, io,
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use native_tls::TlsConnector as NativeTlsConnector;
use serde_json::{json, Map, Value};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async_tls_with_config, tungstenite::client::IntoClientRequest, tungstenite::Message,
    Connector, MaybeTlsStream, WebSocketStream,
};

type DynError = Box<dyn std::error::Error + Send + Sync>;

const CARE_CENTER_URL: &str = "wss://127.0.0.1:4343/";
const CARE_CENTER_AUTH_DATA: &str = "OO24T5iPpUKxfxjPZW4ddo3uGeIJXC+ZbJPzCNGMBenvnZ7J9xQjpykAdgAL1a9HocmZsOHxisitF9B0t7msCayFnVDZA79rADTRjUNWQaLEQA6wSpLrLo/Fu/gHH7BM";
const SETTINGS_PATH: &str = r"C:\ProgramData\Acer\CC\settings.json";
const CARE_CENTER_TIMEOUT: Duration = Duration::from_secs(5);
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const BATTERY_CONTROL_RESULT_PREFIX: &str = "AEROFORGE_BATTERY_CONTROL_RESULT:";

pub struct SmartChargeApplyPayload {
    pub enabled: bool,
    pub battery_healthy: u8,
    pub applied_at_unix: u64,
    pub detail: String,
}

pub async fn sync_saved_state(enabled: bool) -> Result<SmartChargeApplyPayload, DynError> {
    apply_smart_charging(enabled).await
}

pub async fn apply_smart_charging(enabled: bool) -> Result<SmartChargeApplyPayload, DynError> {
    match apply_battery_control_direct(enabled) {
        Ok(payload) => return Ok(payload),
        Err(direct_error) => {
            let fallback = apply_care_center_smart_charging(enabled).await;
            match fallback {
                Ok(mut payload) => {
                    payload.detail = format!(
                        "Direct BatteryControl path was unavailable: {direct_error}. {}",
                        payload.detail
                    );
                    return Ok(payload);
                }
                Err(fallback_error) => {
                    return Err(io::Error::other(format!(
                        "Direct BatteryControl path failed: {direct_error}. Acer Care Center fallback failed: {fallback_error}"
                    ))
                    .into());
                }
            }
        }
    }
}

fn apply_battery_control_direct(enabled: bool) -> Result<SmartChargeApplyPayload, DynError> {
    let requested_health_status = if enabled { 1u8 } else { 0u8 };
    let script = r#"
$status = [byte]$args[0]
$battery = Get-CimInstance -Namespace root\wmi -ClassName BatteryControl -ErrorAction Stop | Select-Object -First 1
if (-not $battery) { throw 'BatteryControl instance was not found.' }

function Emit-AeroForgeResult {
  param($Payload)
  Write-Output ('AEROFORGE_BATTERY_CONTROL_RESULT:' + ($Payload | ConvertTo-Json -Compress -Depth 8))
  exit 0
}

function Read-HealthStatus {
  param($Battery, [int]$BatteryNo, [int]$FunctionQuery)
  Invoke-CimMethod -InputObject $Battery -MethodName GetBatteryHealthControlStatus -Arguments @{
    uBatteryNo = [byte]$BatteryNo
    uFunctionQuery = [byte]$FunctionQuery
    uReserved = ([byte[]](0,0))
  } -ErrorAction Stop
}

function Get-StatusIndexForMask {
  param([int]$FunctionMask)
  if (([int]$FunctionMask -band 1) -ne 0) { return 0 }
  if (([int]$FunctionMask -band 2) -ne 0) { return 1 }
  if (([int]$FunctionMask -band 4) -ne 0) { return 2 }
  return $null
}

function Find-DesiredStatus {
  param($Battery, [int]$Requested, [int]$FunctionMask, [int]$PreferredBatteryNo)
  $reads = New-Object System.Collections.Generic.List[object]
  $batteryNumbers = @($PreferredBatteryNo, 1, 0, 2, 3) | Select-Object -Unique
  foreach ($batteryNo in $batteryNumbers) {
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

  $targetIndex = Get-StatusIndexForMask -FunctionMask $FunctionMask
  foreach ($read in $reads) {
    if (-not $read.Contains('functionStatus')) { continue }
    if ([int]$read.batteryNo -ne $PreferredBatteryNo) { continue }
    $statuses = @($read.functionStatus)
    if ($null -ne $targetIndex -and $statuses.Count -gt $targetIndex -and (([int]$read.functionList -band $FunctionMask) -ne 0) -and $statuses[$targetIndex] -eq $Requested) {
      return [ordered]@{ ok = $true; health = $Requested; index = $targetIndex; read = $read }
    }
  }

  foreach ($read in $reads) {
    if (-not $read.Contains('functionStatus')) { continue }
    if ([int]$read.batteryNo -ne $PreferredBatteryNo) { continue }
    $statuses = @($read.functionStatus)
    if ($null -ne $targetIndex -and $statuses.Count -gt $targetIndex -and $statuses[$targetIndex] -eq $Requested) {
      return [ordered]@{ ok = $true; health = $Requested; index = $targetIndex; read = $read }
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
  Invoke-CimMethod -InputObject $Battery -MethodName GetBatteryFunctionData -Arguments @{
    uFunctionMask = [byte]$FunctionMask
    uReservedIn = ([byte[]](0,0,0,0,0))
  } -ErrorAction Stop
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
    Name = ('battery{0}-legacy-byte0-scalar' -f $BatteryNo)
    BatteryNo = $BatteryNo
    FunctionMask = 1
    Arguments = @{
      uBatteryNo = [byte]$BatteryNo
      uFunctionMask = [byte]1
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  })
  $Attempts.Add(@{
    Name = ('battery{0}-health-byte1-scalar' -f $BatteryNo)
    BatteryNo = $BatteryNo
    FunctionMask = 2
    Arguments = @{
      uBatteryNo = [byte]$BatteryNo
      uFunctionMask = [byte]2
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  })
  $Attempts.Add(@{
    Name = ('battery{0}-combined-byte0-byte1-scalar' -f $BatteryNo)
    BatteryNo = $BatteryNo
    FunctionMask = 3
    Arguments = @{
      uBatteryNo = [byte]$BatteryNo
      uFunctionMask = [byte]3
      uFunctionStatus = $status
      uReservedIn = ([byte[]](0,0,0,0,0))
    }
  })
}

function Add-BatteryFunctionDataAttempts {
  param([System.Collections.Generic.List[object]]$Attempts)
  foreach ($mask in @(1,2,3,0,4,5,7)) {
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
    $match = Find-DesiredStatus -Battery $battery -Requested ([int]$status) -FunctionMask ([int]$attempt.FunctionMask) -PreferredBatteryNo ([int]$attempt.BatteryNo)
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
        matchedFunctionMask = [int]$attempt.FunctionMask
        functionList = [int]$read.functionList
        functionStatus = @($read.functionStatus)
        getReturn = @($read.getReturn)
        setReturn = @($set.uReturn)
        setReservedOut = @($set.uReservedOut)
      })
    }
    $read = $match.read
    $readDetail = if ($read -and $read.Contains('functionStatus')) {
      ('batteryNo {0} query {1} mask {2} statuses [{3}]' -f $read.batteryNo, $read.functionQuery, [int]$attempt.FunctionMask, (@($read.functionStatus) -join ','))
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
            io::Error::other(format!("BatteryControl direct apply failed: {detail}")).into(),
        );
    }

    let parsed = parse_battery_control_result_value(&output.stdout)?;
    let verified_health_status = parsed
        .get("healthStatus")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .ok_or_else(|| {
            io::Error::other(format!(
                "BatteryControl response did not include healthStatus: {parsed}"
            ))
        })?;
    if verified_health_status != requested_health_status {
        return Err(io::Error::other(format!(
            "BatteryControl returned healthStatus {} after requesting {}.",
            verified_health_status, requested_health_status
        ))
        .into());
    }

    let care_center_battery_healthy_semantic = if verified_health_status == 1 { 0 } else { 1 };
    let detail = if enabled {
        format!(
            "Applied optimized charging through direct BatteryControl WMI. Health limiter status {} keeps the 80% ceiling active. {}",
            verified_health_status,
            battery_control_attempt_detail(&parsed)
        )
    } else {
        format!(
            "Applied full battery charging through direct BatteryControl WMI. Health limiter status {} allows full charge. {}",
            verified_health_status,
            battery_control_attempt_detail(&parsed)
        )
    };

    Ok(SmartChargeApplyPayload {
        enabled,
        battery_healthy: care_center_battery_healthy_semantic,
        applied_at_unix: now_unix(),
        detail,
    })
}

fn battery_control_attempt_detail(output: &Value) -> String {
    let attempt = output
        .get("setAttempt")
        .and_then(Value::as_str)
        .unwrap_or("BatteryControl attempt");
    let mode = output.get("mode").and_then(Value::as_str);
    if mode == Some("battery-function-data") {
        let mask = output
            .get("matchedFunctionMask")
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".into());
        let status = output
            .get("bacStatus")
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .unwrap_or_else(|| "unknown".into());
        return format!(
            "Matched BatteryControl function-data mask {mask} BAC status {status} with {attempt}."
        );
    }

    let index = output
        .get("matchedStatusIndex")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into());
    let battery_no = output
        .get("matchedBatteryNo")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into());
    let query = output
        .get("matchedFunctionQuery")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("Matched BatteryControl battery {battery_no} query {query} status byte {index} with {attempt}.")
}

fn parse_battery_control_result_value(bytes: &[u8]) -> Result<Value, DynError> {
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

    Ok(serde_json::from_str::<Value>(payload)?)
}

async fn apply_care_center_smart_charging(
    enabled: bool,
) -> Result<SmartChargeApplyPayload, DynError> {
    let requested_battery_healthy = if enabled { 0 } else { 1 };
    let mut client =
        with_timeout("connect to Acer Care Center", CareCenterClient::connect()).await?;

    with_timeout(
        "set Acer Care Center BatteryHealthy",
        client.set_battery_healthy(requested_battery_healthy),
    )
    .await?;
    let verified_battery_healthy = with_timeout(
        "read Acer Care Center BatteryHealthy",
        client.get_battery_healthy(),
    )
    .await?;
    if verified_battery_healthy != requested_battery_healthy {
        return Err(io::Error::other(format!(
            "Care Center returned BatteryHealthy {} after requesting {}.",
            verified_battery_healthy, requested_battery_healthy
        ))
        .into());
    }

    let boundary = with_timeout(
        "read Acer Care Center BatteryBoundary",
        client.get_battery_boundary(),
    )
    .await
    .ok();
    persist_battery_healthy(verified_battery_healthy)?;

    let detail = match (enabled, boundary) {
        (true, Some(boundary)) => format!(
            "Applied Acer Care Center optimized charging (BatteryHealthy=0). BatteryBoundary reports {}-{} with the 80% ceiling active.",
            boundary.lower_bound, boundary.upper_bound
        ),
        (true, None) => {
            "Applied Acer Care Center optimized charging (BatteryHealthy=0) with the 80% ceiling active."
                .into()
        }
        (false, Some(boundary)) => format!(
            "Applied full battery charging through Acer Care Center (BatteryHealthy=1). BatteryBoundary still reports {}-{}, but BatteryHealthy is the real full-charge gate on this SKU.",
            boundary.lower_bound, boundary.upper_bound
        ),
        (false, None) => {
            "Applied full battery charging through Acer Care Center (BatteryHealthy=1)."
                .into()
        }
    };

    Ok(SmartChargeApplyPayload {
        enabled,
        battery_healthy: verified_battery_healthy,
        applied_at_unix: now_unix(),
        detail,
    })
}

async fn with_timeout<F, T>(label: &str, future: F) -> Result<T, DynError>
where
    F: std::future::Future<Output = Result<T, DynError>>,
{
    match tokio::time::timeout(CARE_CENTER_TIMEOUT, future).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::other(format!("Timed out while trying to {label}.")).into()),
    }
}

struct CareCenterClient {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl CareCenterClient {
    async fn connect() -> Result<Self, DynError> {
        let mut request = CARE_CENTER_URL.into_client_request()?;
        request
            .headers_mut()
            .insert("Origin", "https://127.0.0.1".parse()?);

        let tls = NativeTlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true)
            .build()?;
        let connector = Connector::NativeTls(tls);
        let (mut socket, _) =
            connect_async_tls_with_config(request, None, false, Some(connector)).await?;

        socket
            .send(Message::Text(
                json!({
                    "PacketType": 1,
                    "Version": 1,
                    "Data": CARE_CENTER_AUTH_DATA,
                })
                .to_string(),
            ))
            .await?;
        let auth_reply = recv_json(&mut socket).await?;
        let packet_type = auth_reply
            .get("PacketType")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if packet_type != 1 {
            return Err(io::Error::other(format!(
                "Unexpected Care Center auth response: {auth_reply}"
            ))
            .into());
        }

        socket
            .send(Message::Text(
                json!({
                    "PacketType": 2,
                    "Version": 1,
                    "Session": "af-function-query",
                    "Command": "FunctionQuery",
                })
                .to_string(),
            ))
            .await?;
        let function_query = recv_json(&mut socket).await?;
        let battery_healthy_supported = function_query
            .get("Data")
            .and_then(Value::as_array)
            .map(|items| items.iter().any(|entry| entry == "BatteryHealthy"))
            .unwrap_or(false);
        if !battery_healthy_supported {
            return Err(io::Error::other(
                "Acer Care Center does not report BatteryHealthy support on this machine.",
            )
            .into());
        }

        Ok(Self { socket })
    }

    async fn set_battery_healthy(&mut self, value: u8) -> Result<(), DynError> {
        let response = self
            .send_command(json!({
                "PacketType": 2,
                "Version": 1,
                "Session": format!("af-bh-set-{value}"),
                "Command": "BatteryHealthy",
                "Action": "Set",
                "Param1": value,
            }))
            .await?;

        let packet_type = response
            .get("PacketType")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if packet_type != 3 {
            return Err(io::Error::other(format!(
                "Unexpected BatteryHealthy set response: {response}"
            ))
            .into());
        }
        Ok(())
    }

    async fn get_battery_healthy(&mut self) -> Result<u8, DynError> {
        let response = self
            .send_command(json!({
                "PacketType": 2,
                "Version": 1,
                "Session": "af-bh-get",
                "Command": "BatteryHealthy",
                "Action": "Get",
            }))
            .await?;
        response
            .get("Result")
            .and_then(Value::as_object)
            .and_then(|result| result.get("Value"))
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryHealthy get response did not contain Value: {response}"
                ))
                .into()
            })
    }

    async fn get_battery_boundary(&mut self) -> Result<BatteryBoundary, DynError> {
        let response = self
            .send_command(json!({
                "PacketType": 2,
                "Version": 1,
                "Session": "af-boundary-get",
                "Command": "BatteryBoundary",
                "Action": "Get",
            }))
            .await?;
        let result = response
            .get("Result")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryBoundary response missing Result object: {response}"
                ))
            })?;
        let upper_bound = result
            .get("UpperBound")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryBoundary response missing UpperBound: {response}"
                ))
            })?;
        let lower_bound = result
            .get("LowerBound")
            .and_then(Value::as_u64)
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| {
                io::Error::other(format!(
                    "BatteryBoundary response missing LowerBound: {response}"
                ))
            })?;

        Ok(BatteryBoundary {
            upper_bound,
            lower_bound,
        })
    }

    async fn send_command(&mut self, payload: Value) -> Result<Value, DynError> {
        self.socket.send(Message::Text(payload.to_string())).await?;
        recv_json(&mut self.socket).await
    }
}

struct BatteryBoundary {
    upper_bound: u8,
    lower_bound: u8,
}

async fn recv_json(
    socket: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> Result<Value, DynError> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Binary(bytes) => return Ok(serde_json::from_slice(&bytes)?),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
            Message::Pong(_) => {}
            Message::Frame(_) => {}
            Message::Close(frame) => {
                return Err(io::Error::other(format!(
                    "Care Center websocket closed before a JSON reply arrived: {frame:?}"
                ))
                .into())
            }
        }
    }

    Err(io::Error::other("Care Center websocket ended before a JSON reply arrived.").into())
}

fn persist_battery_healthy(battery_healthy: u8) -> Result<(), DynError> {
    let path = Path::new(SETTINGS_PATH);
    let mut root = if path.exists() {
        serde_json::from_str::<Value>(&fs::read_to_string(path)?)?
    } else {
        Value::Object(Map::new())
    };

    let object = root
        .as_object_mut()
        .ok_or_else(|| io::Error::other("Care Center settings.json was not a JSON object."))?;
    object.insert("BatteryHealthy".into(), Value::from(battery_healthy));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, serde_json::to_string_pretty(&root)?)?;
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
