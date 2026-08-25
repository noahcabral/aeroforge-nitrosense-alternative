param(
  [string]$Label = '',
  [int]$Seconds = 45,
  [int]$IntervalMs = 1000,
  [string]$OutputRoot = (Join-Path $env:USERPROFILE 'Desktop'),
  [switch]$NoOpen,
  [switch]$Quiet
)

$ErrorActionPreference = 'Stop'

if ($Seconds -lt 1) {
  throw 'Seconds must be at least 1.'
}

if ($IntervalMs -lt 250) {
  throw 'IntervalMs must be at least 250.'
}

$stateDir = Join-Path $env:ProgramData 'AeroForge\Service\state'
if (-not (Test-Path -LiteralPath $stateDir)) {
  throw "AeroForge service state folder was not found at $stateDir. Install or start AeroForgeService first."
}

function Get-SafeLabel {
  param([string]$Value)

  $safe = ($Value -replace '[^a-zA-Z0-9._-]+', '-').Trim('-')
  if ([string]::IsNullOrWhiteSpace($safe)) {
    return ''
  }
  return "-$safe"
}

function Read-JsonFile {
  param([string]$Path)

  if (-not (Test-Path -LiteralPath $Path)) {
    return $null
  }

  for ($attempt = 0; $attempt -lt 3; $attempt++) {
    try {
      $raw = Get-Content -Raw -LiteralPath $Path
      if ([string]::IsNullOrWhiteSpace($raw)) {
        return $null
      }
      return $raw | ConvertFrom-Json
    } catch {
      if ($attempt -eq 2) {
        return $null
      }
      Start-Sleep -Milliseconds 75
    }
  }
}

function Get-JsonValue {
  param(
    $Object,
    [string]$Path,
    $Default = $null
  )

  if ($null -eq $Object) {
    return $Default
  }

  $cursor = $Object
  foreach ($part in ($Path -split '\.')) {
    if ($null -eq $cursor) {
      return $Default
    }

    $property = $cursor.PSObject.Properties[$part]
    if ($null -eq $property) {
      return $Default
    }

    $cursor = $property.Value
  }

  if ($null -eq $cursor) {
    return $Default
  }

  return $cursor
}

function Trim-Text {
  param(
    [object]$Value,
    [int]$MaxLength = 240
  )

  if ($null -eq $Value) {
    return $null
  }

  $text = [string]$Value
  if ($text.Length -le $MaxLength) {
    return $text
  }

  return $text.Substring(0, $MaxLength) + '...'
}

function Get-FirstValue {
  param(
    [object[]]$Values
  )

  foreach ($value in $Values) {
    if ($null -ne $value) {
      return $value
    }
  }
  return $null
}

function New-Stats {
  param(
    [object[]]$Rows,
    [string]$Property
  )

  $values = @(
    foreach ($row in $Rows) {
      $value = $row.PSObject.Properties[$Property].Value
      if ($null -ne $value -and $value -ne '') {
        [double]$value
      }
    }
  )

  if ($values.Count -eq 0) {
    return [pscustomobject]@{
      count = 0
      min = $null
      avg = $null
      max = $null
    }
  }

  $measure = $values | Measure-Object -Minimum -Maximum -Average
  return [pscustomobject]@{
    count = $values.Count
    min = [math]::Round($measure.Minimum, 3)
    avg = [math]::Round($measure.Average, 3)
    max = [math]::Round($measure.Maximum, 3)
  }
}

$timestamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$runDir = Join-Path $OutputRoot ("AeroForge-TDP-Trace-$timestamp$(Get-SafeLabel -Value $Label)")
$rawDir = Join-Path $runDir 'raw-state'
New-Item -ItemType Directory -Force -Path $runDir, $rawDir | Out-Null

$csvPath = Join-Path $runDir 'samples.csv'
$jsonlPath = Join-Path $runDir 'samples.jsonl'
$summaryPath = Join-Path $runDir 'summary.json'
$notesPath = Join-Path $runDir 'README.txt'

$notes = @"
AeroForge CPU TDP trace
Started: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')
Label: $Label
Duration: $Seconds seconds
Interval: $IntervalMs ms

This script is read-only. It samples AeroForge service state files and does not apply power modes, fan modes, or firmware writes.

Recommended passes:
1. Set AeroForge to Quiet, run: .\scripts\Trace-AeroForgeCpuTdp.ps1 -Label Quiet
2. Set AeroForge to Balanced, run: .\scripts\Trace-AeroForgeCpuTdp.ps1 -Label Balanced
3. Set AeroForge to Turbo, run: .\scripts\Trace-AeroForgeCpuTdp.ps1 -Label Turbo
4. Set AeroForge to Max fan or the fan state under test, run with a matching label.

If cpuPackagePowerW, cpuPl1W, and cpuPl2W are blank, the installed AeroForgeService does not expose the new RAPL readback yet.
"@
Set-Content -LiteralPath $notesPath -Value $notes -Encoding UTF8

$telemetryPath = Join-Path $stateDir 'telemetry.json'
$lowlevelPath = Join-Path $stateDir 'lowlevel.json'
$controlPath = Join-Path $stateDir 'control.json'
$supervisorPath = Join-Path $stateDir 'supervisor.json'
$capabilitiesPath = Join-Path $stateDir 'capabilities.json'

if (-not $Quiet) {
  Write-Host "Writing trace to: $runDir"
  Write-Host 'Switch AeroForge to the target mode before or during this capture.'
}

$records = New-Object System.Collections.Generic.List[object]
$started = Get-Date
$stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$sampleIndex = 0

while ($stopwatch.Elapsed.TotalSeconds -lt $Seconds) {
  $telemetry = Read-JsonFile -Path $telemetryPath
  $lowlevel = Read-JsonFile -Path $lowlevelPath
  $control = Read-JsonFile -Path $controlPath
  $supervisor = Read-JsonFile -Path $supervisorPath

  $cpuPackagePowerW = Get-FirstValue @(
    (Get-JsonValue $telemetry 'cpuPackagePowerW'),
    (Get-JsonValue $lowlevel 'packagePowerW')
  )
  $cpuPl1W = Get-FirstValue @(
    (Get-JsonValue $telemetry 'cpuPl1W'),
    (Get-JsonValue $lowlevel 'packagePl1W')
  )
  $cpuPl2W = Get-FirstValue @(
    (Get-JsonValue $telemetry 'cpuPl2W'),
    (Get-JsonValue $lowlevel 'packagePl2W')
  )
  $cpuPl1Enabled = Get-FirstValue @(
    (Get-JsonValue $telemetry 'cpuPl1Enabled'),
    (Get-JsonValue $lowlevel 'packagePl1Enabled')
  )
  $cpuPl2Enabled = Get-FirstValue @(
    (Get-JsonValue $telemetry 'cpuPl2Enabled'),
    (Get-JsonValue $lowlevel 'packagePl2Enabled')
  )
  $cpuPowerLimitLocked = Get-FirstValue @(
    (Get-JsonValue $telemetry 'cpuPowerLimitLocked'),
    (Get-JsonValue $lowlevel 'packagePowerLimitLocked')
  )

  $record = [pscustomobject]@{
    timestamp = (Get-Date).ToString('o')
    elapsedSec = [math]::Round($stopwatch.Elapsed.TotalSeconds, 3)
    sampleIndex = $sampleIndex
    label = $Label
    activePowerProfile = Get-JsonValue $control 'activePowerProfile'
    activeFanProfile = Get-JsonValue $control 'activeFanProfile'
    customBaseProfile = Get-JsonValue $control 'customBaseProfile'
    processorMinPercent = Get-JsonValue $control 'processorState.minPercent'
    processorMaxPercent = Get-JsonValue $control 'processorState.maxPercent'
    readbackAcMinPercent = Get-JsonValue $control 'processorStateReadback.ac.minPercent'
    readbackAcMaxPercent = Get-JsonValue $control 'processorStateReadback.ac.maxPercent'
    processorStateDriftDetected = Get-JsonValue $control 'processorStateDriftDetected'
    lastApplyDetail = Trim-Text (Get-JsonValue $control 'lastApplyDetail')
    lastFanApplyDetail = Trim-Text (Get-JsonValue $control 'lastFanApplyDetail')
    serviceWorkerCount = Get-JsonValue $supervisor 'workerCount'
    serviceUpdatedAtUnix = Get-JsonValue $supervisor 'updatedAtUnix'
    cpuPackagePowerW = $cpuPackagePowerW
    cpuPl1W = $cpuPl1W
    cpuPl1Enabled = $cpuPl1Enabled
    cpuPl2W = $cpuPl2W
    cpuPl2Enabled = $cpuPl2Enabled
    cpuPowerLimitLocked = $cpuPowerLimitLocked
    cpuTempC = Get-JsonValue $telemetry 'cpuTempC'
    cpuTempAverageC = Get-JsonValue $telemetry 'cpuTempAverageC'
    cpuTempLowestCoreC = Get-JsonValue $telemetry 'cpuTempLowestCoreC'
    cpuTempHighestCoreC = Get-JsonValue $telemetry 'cpuTempHighestCoreC'
    lowlevelAvailable = Get-JsonValue $lowlevel 'available'
    lowlevelTransport = Get-JsonValue $lowlevel 'transport'
    lowlevelPackageTempC = Get-JsonValue $lowlevel 'packageTempC'
    lowlevelAverageCoreTempC = Get-JsonValue $lowlevel 'averageCoreTempC'
    lowlevelDetail = Trim-Text (Get-JsonValue $lowlevel 'detail')
    cpuClockMhz = Get-JsonValue $telemetry 'cpuClockMhz'
    cpuUsagePercent = Get-JsonValue $telemetry 'cpuUsagePercent'
    cpuFanRpm = Get-JsonValue $telemetry 'cpuFanRpm'
    gpuPowerDrawW = Get-JsonValue $telemetry 'gpuPowerDrawW'
    gpuPowerLimitW = Get-JsonValue $telemetry 'gpuPowerLimitW'
    gpuPowerDefaultLimitW = Get-JsonValue $telemetry 'gpuPowerDefaultLimitW'
    gpuPowerMinLimitW = Get-JsonValue $telemetry 'gpuPowerMinLimitW'
    gpuPowerMaxLimitW = Get-JsonValue $telemetry 'gpuPowerMaxLimitW'
    gpuTempC = Get-JsonValue $telemetry 'gpuTempC'
    gpuUsagePercent = Get-JsonValue $telemetry 'gpuUsagePercent'
    gpuClockMhz = Get-JsonValue $telemetry 'gpuClockMhz'
    gpuFanRpm = Get-JsonValue $telemetry 'gpuFanRpm'
    systemTempC = Get-JsonValue $telemetry 'systemTempC'
    batteryPercent = Get-JsonValue $telemetry 'batteryPercent'
    acPluggedIn = Get-JsonValue $telemetry 'acPluggedIn'
    telemetryHeartbeat = Get-JsonValue $telemetry 'heartbeat'
  }

  $records.Add($record) | Out-Null

  if ($sampleIndex -eq 0) {
    $record | Export-Csv -LiteralPath $csvPath -NoTypeInformation
  } else {
    $record | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Append
  }

  Add-Content -LiteralPath $jsonlPath -Value ($record | ConvertTo-Json -Depth 6 -Compress)

  if (-not $Quiet) {
    $cpuWatts = if ($null -eq $record.cpuPackagePowerW) { 'n/a' } else { "{0:N1}W" -f [double]$record.cpuPackagePowerW }
    $pl1 = if ($null -eq $record.cpuPl1W) { 'n/a' } else { "{0:N1}W" -f [double]$record.cpuPl1W }
    $pl2 = if ($null -eq $record.cpuPl2W) { 'n/a' } else { "{0:N1}W" -f [double]$record.cpuPl2W }
    $gpuLimit = if ($null -eq $record.gpuPowerLimitW) { 'n/a' } else { "{0:N1}W" -f [double]$record.gpuPowerLimitW }
    Write-Host ("[{0,3}s] mode={1}/{2} cpu={3} pl1={4} pl2={5} gpuLimit={6} fans={7}/{8} rpm" -f `
      [int]$stopwatch.Elapsed.TotalSeconds,
      $record.activePowerProfile,
      $record.activeFanProfile,
      $cpuWatts,
      $pl1,
      $pl2,
      $gpuLimit,
      $record.cpuFanRpm,
      $record.gpuFanRpm)
  }

  $sampleIndex++
  Start-Sleep -Milliseconds $IntervalMs
}

$summary = [pscustomobject]@{
  startedAt = $started.ToString('o')
  endedAt = (Get-Date).ToString('o')
  label = $Label
  durationSeconds = $Seconds
  intervalMs = $IntervalMs
  outputDirectory = $runDir
  sampleCount = $records.Count
  activePowerProfiles = @($records | Select-Object -ExpandProperty activePowerProfile -Unique)
  activeFanProfiles = @($records | Select-Object -ExpandProperty activeFanProfile -Unique)
  cpuPackagePowerW = New-Stats -Rows $records -Property 'cpuPackagePowerW'
  cpuPl1W = New-Stats -Rows $records -Property 'cpuPl1W'
  cpuPl2W = New-Stats -Rows $records -Property 'cpuPl2W'
  cpuClockMhz = New-Stats -Rows $records -Property 'cpuClockMhz'
  cpuUsagePercent = New-Stats -Rows $records -Property 'cpuUsagePercent'
  cpuFanRpm = New-Stats -Rows $records -Property 'cpuFanRpm'
  gpuPowerDrawW = New-Stats -Rows $records -Property 'gpuPowerDrawW'
  gpuPowerLimitW = New-Stats -Rows $records -Property 'gpuPowerLimitW'
  gpuFanRpm = New-Stats -Rows $records -Property 'gpuFanRpm'
  notes = @(
    if (($records | Where-Object { $null -ne $_.cpuPl1W -or $null -ne $_.cpuPl2W -or $null -ne $_.cpuPackagePowerW }).Count -eq 0) {
      'CPU RAPL fields were blank for the whole run. The installed AeroForgeService probably has not been rebuilt or installed with the new readback fields yet, or this CPU does not expose this Intel RAPL path.'
    }
    if (($records | Where-Object { $null -ne $_.gpuPowerLimitW }).Count -eq 0) {
      'GPU power-limit fields were blank for the whole run.'
    }
  )
}

$summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $summaryPath -Encoding UTF8

foreach ($path in @($telemetryPath, $lowlevelPath, $controlPath, $supervisorPath, $capabilitiesPath)) {
  if (Test-Path -LiteralPath $path) {
    Copy-Item -LiteralPath $path -Destination (Join-Path $rawDir (Split-Path -Leaf $path)) -Force
  }
}

if (-not $Quiet) {
  Write-Host ''
  Write-Host "Trace complete: $runDir"
  Write-Host "Samples: $csvPath"
  Write-Host "Summary: $summaryPath"
}

if (-not $NoOpen) {
  Start-Process explorer.exe -ArgumentList "`"$runDir`""
}

Write-Output $runDir
