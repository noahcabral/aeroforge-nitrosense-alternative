@echo off
setlocal
title AeroForge Debug Collector

set "AFD_SOURCE=%~f0"
set "AFD_PAYLOAD=%TEMP%\AeroForge-Debug-%RANDOM%%RANDOM%.ps1"
set "AFD_NOPAUSE=0"

for %%A in (%*) do (
  if /I "%%~A"=="-NoPause" set "AFD_NOPAUSE=1"
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "$source=$env:AFD_SOURCE; $out=$env:AFD_PAYLOAD; $raw=[IO.File]::ReadAllText($source); $marker=':: POWERSHELL_PAYLOAD'; $idx=$raw.LastIndexOf($marker); if($idx -lt 0){Write-Error 'PowerShell payload marker missing.'; exit 10}; $payload=$raw.Substring($idx + $marker.Length); $encoding=New-Object System.Text.UTF8Encoding($false); [IO.File]::WriteAllText($out,$payload,$encoding)"
if errorlevel 1 (
  echo Failed to prepare the AeroForge debug collector.
  if not "%AFD_NOPAUSE%"=="1" pause
  exit /b 1
)

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%AFD_PAYLOAD%" %*
set "AFD_EXIT=%ERRORLEVEL%"
del "%AFD_PAYLOAD%" >nul 2>nul
if not "%AFD_EXIT%"=="0" (
  echo.
  echo AeroForge debug collector failed with exit code %AFD_EXIT%.
  echo Send a screenshot of this window along with any AeroForge-Debug folder or ZIP that was created.
  if not "%AFD_NOPAUSE%"=="1" pause
)
exit /b %AFD_EXIT%

:: POWERSHELL_PAYLOAD
param(
  [switch]$NoPause,
  [switch]$NoElevate,
  [int]$SampleSeconds = 0,
  [int]$SampleIntervalSeconds = 3,
  [string]$OutputRoot = ""
)

$ErrorActionPreference = "Continue"
$script:CommandIndex = 0
$script:TranscriptStarted = $false

trap {
  $message = $_.Exception.Message
  Write-Host ""
  Write-Host "AeroForge debug collector stopped because of an unexpected error."
  Write-Host $message
  if (-not [string]::IsNullOrWhiteSpace($script:MasterLog)) {
    try {
      Add-Content -LiteralPath $script:MasterLog -Value ("[{0}] Unexpected collector failure: {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $message) -Encoding UTF8
    } catch {
    }
  }
  if (-not $NoPause) {
    Read-Host "Press Enter to close"
  }
  exit 1
}

function Get-TimeStamp {
  Get-Date -Format "yyyyMMdd-HHmmss"
}

function Redact-Text {
  param([AllowNull()][string]$Text)

  if ($null -eq $Text) {
    return ""
  }

  $redacted = $Text
  $redacted = $redacted -replace 'github_pat_[A-Za-z0-9_]+', '[REDACTED_GITHUB_PAT]'
  $redacted = $redacted -replace 'gh[pousr]_[A-Za-z0-9_]+', '[REDACTED_GITHUB_TOKEN]'
  $redacted = $redacted -replace '(?i)(authorization\s*[:=]\s*(bearer|token)\s+)[^\s''"]+', '$1[REDACTED]'
  $redacted = $redacted -replace '(?i)(password\s*[:=]\s*)[^\s''"]+', '$1[REDACTED]'
  $redacted = $redacted -replace '(?i)(secret\s*[:=]\s*)[^\s''"]+', '$1[REDACTED]'
  return $redacted
}

function Write-LogLine {
  param([string]$Message)
  $line = "[{0}] {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $Message
  Write-Host $line
  Add-Content -LiteralPath $script:MasterLog -Value $line -Encoding UTF8
}

function Write-TextFile {
  param(
    [string]$Path,
    [AllowNull()][string]$Text
  )

  $parent = Split-Path -Parent $Path
  if ($parent -and -not (Test-Path -LiteralPath $parent)) {
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
  }

  Set-Content -LiteralPath $Path -Value (Redact-Text $Text) -Encoding UTF8
}

function Invoke-DiagCommand {
  param(
    [string]$Name,
    [scriptblock]$ScriptBlock
  )

  $script:CommandIndex++
  $safeName = ($Name -replace '[^A-Za-z0-9_.-]+', '_').Trim('_')
  $path = Join-Path $script:CommandsDir ("{0:000}-{1}.txt" -f $script:CommandIndex, $safeName)
  Write-LogLine "Collecting $Name"

  $header = @(
    "AeroForge debug collector command: $Name",
    "Timestamp: $(Get-Date -Format o)",
    "Admin: $script:IsAdmin",
    ""
  ) -join [Environment]::NewLine

  try {
    $output = & $ScriptBlock *>&1 | Out-String -Width 4096
    Write-TextFile -Path $path -Text ($header + $output)
  } catch {
    $message = $header + "Collector command failed: $($_.Exception.Message)"
    Write-TextFile -Path $path -Text $message
  }
}

function Test-IsAdmin {
  try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  } catch {
    return $false
  }
}

function Start-ElevatedCollectorIfNeeded {
  if ($script:IsAdmin -or $NoElevate) {
    return $false
  }

  if ([string]::IsNullOrWhiteSpace($env:AFD_SOURCE) -or -not (Test-Path -LiteralPath $env:AFD_SOURCE)) {
    return $false
  }

  $argumentList = New-Object System.Collections.Generic.List[string]
  $argumentList.Add("-NoElevate")
  if ($NoPause) {
    $argumentList.Add("-NoPause")
  }
  if ($SampleSeconds -gt 0) {
    $argumentList.Add("-SampleSeconds")
    $argumentList.Add([string]$SampleSeconds)
  }
  if ($SampleIntervalSeconds -ne 3) {
    $argumentList.Add("-SampleIntervalSeconds")
    $argumentList.Add([string]$SampleIntervalSeconds)
  }
  if (-not [string]::IsNullOrWhiteSpace($OutputRoot)) {
    $argumentList.Add("-OutputRoot")
    $argumentList.Add($OutputRoot)
  }

  try {
    $quotedArgs = New-Object System.Collections.Generic.List[string]
    $quotedArgs.Add(('"{0}"' -f $env:AFD_SOURCE))
    foreach ($argument in $argumentList) {
      $quotedArgs.Add(('"{0}"' -f ($argument -replace '"', '\"')))
    }
    $cmdArguments = "/d /c " + ($quotedArgs -join " ")
    Write-Host "AeroForge debug collector is requesting administrator permission."
    Write-Host "Keep this window open; it will wait for the elevated collector to finish."
    $process = Start-Process -FilePath $env:ComSpec -ArgumentList $cmdArguments -Verb RunAs -WorkingDirectory (Split-Path -Parent $env:AFD_SOURCE) -Wait -PassThru
    if ($process.ExitCode -ne 0) {
      Write-Host "Elevated collector exited with code $($process.ExitCode)."
      if (-not $NoPause) {
        Read-Host "Press Enter to close"
      }
      exit $process.ExitCode
    }
    Write-Host "Elevated AeroForge debug collection finished."
    return $true
  } catch {
    Write-Host "Could not relaunch elevated: $($_.Exception.Message)"
    Write-Host "Continuing without elevation; direct Acer WMI instance probes may be incomplete."
    return $false
  }
}

function Get-RegistryInstalledApps {
  $roots = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
  )

  foreach ($root in $roots) {
    Get-ItemProperty -Path $root -ErrorAction SilentlyContinue |
      Where-Object {
        $_.DisplayName -and
        ($_.DisplayName -match 'AeroForge|Nitro|Predator|Acer|Quick Access|QuickAccess|WebView|NVIDIA|Microsoft Edge WebView')
      } |
      Select-Object DisplayName, DisplayVersion, Publisher, InstallDate, InstallLocation, UninstallString, QuietUninstallString, PSPath
  }
}

function Invoke-NamedPipeJsonRequest {
  param(
    [string]$Kind,
    [int]$TimeoutMs = 2500
  )

  $pipe = $null
  $writer = $null
  $reader = $null
  try {
    $pipe = New-Object System.IO.Pipes.NamedPipeClientStream(".", "AeroForgeService", [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::None)
    $pipe.Connect($TimeoutMs)
    $reader = New-Object System.IO.StreamReader($pipe, [System.Text.Encoding]::UTF8, $false, 4096, $true)
    $payload = (ConvertTo-Json @{ kind = $Kind } -Compress) + "`n"
    $bytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    $pipe.Write($bytes, 0, $bytes.Length)
    $pipe.Flush()
    $line = $reader.ReadLine()
    if ([string]::IsNullOrWhiteSpace($line)) {
      return "No response returned for $Kind."
    }
    return $line
  } catch {
    return "Pipe request $Kind failed: $($_.Exception.Message)"
  } finally {
    if ($reader) { $reader.Dispose() }
    if ($writer) { $writer.Dispose() }
    if ($pipe) { $pipe.Dispose() }
  }
}

function ConvertFrom-PipeJsonReply {
  param([AllowNull()][string]$Text)

  if ([string]::IsNullOrWhiteSpace($Text)) {
    return $null
  }

  try {
    $parsed = $Text | ConvertFrom-Json -ErrorAction Stop
    if ($parsed.kind -eq "ok" -and $null -ne $parsed.payload) {
      return $parsed.payload
    }
    return $parsed
  } catch {
    return $null
  }
}

function Get-AeroForgePipeProbe {
  foreach ($kind in @("getServiceStatus", "getCapabilities", "getControlSnapshot", "getTelemetrySnapshot")) {
    "===== $kind ====="
    $reply = Invoke-NamedPipeJsonRequest -Kind $kind
    try {
      $parsed = $reply | ConvertFrom-Json -ErrorAction Stop
      $parsed | ConvertTo-Json -Depth 10
    } catch {
      $reply
    }
    ""
  }
}

function Get-AcerDirectWmiReadOnlyProbe {
  if (-not $script:IsAdmin) {
    "NOTE: Collector is not elevated. Acer WMI classes may be visible while Acer WMI instances and method calls are hidden. Re-run without -NoElevate for the full hardware probe."
    ""
  }

  "===== ROOT\WMI Acer classes ====="
  Get-CimClass -Namespace root\wmi -ErrorAction SilentlyContinue |
    Where-Object { $_.CimClassName -match 'Acer|Gaming|BatteryControl|GenericMethod' } |
    Select-Object CimClassName, CimSuperClassName, CimClassMethods, CimClassProperties |
    Format-List

  "===== AcerGamingFunction read-only values ====="
  $gaming = Get-CimInstance -Namespace root\wmi -ClassName AcerGamingFunction -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($gaming) {
    $probeRows = foreach ($item in @(
      @{ name = "SupportedProfiles"; method = "GetGamingMiscSetting"; input = [uint32]0x0A },
      @{ name = "PlatformProfile"; method = "GetGamingMiscSetting"; input = [uint32]0x0B },
      @{ name = "BootAnimationSound"; method = "GetGamingMiscSetting"; input = [uint32]0x06 },
      @{ name = "SupportedSensors"; method = "GetGamingSysInfo"; input = [uint32]0x0000 },
      @{ name = "BatteryStatus"; method = "GetGamingSysInfo"; input = [uint32]0x0002 },
      @{ name = "CpuTemp"; method = "GetGamingSysInfo"; input = [uint32]0x0101 },
      @{ name = "CpuFan"; method = "GetGamingSysInfo"; input = [uint32]0x0201 },
      @{ name = "SystemTemp"; method = "GetGamingSysInfo"; input = [uint32]0x0301 },
      @{ name = "GpuFan"; method = "GetGamingSysInfo"; input = [uint32]0x0601 },
      @{ name = "GpuTemp"; method = "GetGamingSysInfo"; input = [uint32]0x0A01 },
      @{ name = "FanBehavior"; method = "GetGamingFanBehavior"; input = [uint32]0 },
      @{ name = "CpuFanSpeed"; method = "GetGamingFanSpeed"; input = [uint32]1 },
      @{ name = "GpuFanSpeed"; method = "GetGamingFanSpeed"; input = [uint32]4 }
    )) {
      try {
        $result = Invoke-CimMethod -InputObject $gaming -MethodName $item.method -Arguments @{ gmInput = $item.input } -ErrorAction Stop
        [pscustomobject]@{
          name = $item.name
          method = $item.method
          input = ("0x{0:X}" -f [uint32]$item.input)
          output = $result.gmOutput
          outputHex = ("0x{0:X}" -f [uint64]$result.gmOutput)
          decodedReading = ((([uint64]$result.gmOutput) -shr 8) -band 0xFFFF)
        }
      } catch {
        [pscustomobject]@{
          name = $item.name
          method = $item.method
          input = ("0x{0:X}" -f [uint32]$item.input)
          error = $_.Exception.Message
        }
      }
    }
    $probeRows | Format-Table -AutoSize -Wrap
    try {
      $supported = $probeRows | Where-Object { $_.name -eq 'SupportedProfiles' } | Select-Object -First 1
      $current = $probeRows | Where-Object { $_.name -eq 'PlatformProfile' } | Select-Object -First 1
      if ($supported -or $current) {
        ""
        "===== Acer platform-profile interpretation ====="
        [pscustomobject]@{
          supportedProfilesRaw = $supported.outputHex
          currentPlatformProfileRaw = $current.outputHex
          note = "Raw bytes are authoritative. AeroForge should treat these values as the AMD machine's actual AcerGamingFunction profile surface."
        } | Format-List
      }
    } catch {
    }
  } else {
    "AcerGamingFunction instance not found."
  }

  "===== BatteryControl read-only health status ====="
  $batteryControl = Get-CimInstance -Namespace root\wmi -ClassName BatteryControl -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($batteryControl) {
    try {
      Invoke-CimMethod -InputObject $batteryControl -MethodName GetBatteryHealthControlStatus -Arguments @{
        uBatteryNo = [byte]1
        uFunctionQuery = [byte]1
        uReserved = ([byte[]](0,0))
      } -ErrorAction Stop | Format-List *
    } catch {
      "BatteryControl health status read failed: $($_.Exception.Message)"
    }
  } else {
    "BatteryControl instance not found."
  }
}

function Write-DebugSummaryJson {
  $flags = New-Object System.Collections.Generic.List[string]
  $os = Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue | Select-Object -First 1 Caption, Version, BuildNumber, OSArchitecture
  $computer = Get-CimInstance Win32_ComputerSystem -ErrorAction SilentlyContinue | Select-Object -First 1 Manufacturer, Model, SystemType, TotalPhysicalMemory
  $processor = Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue | Select-Object -First 1 Name, Manufacturer, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, CurrentClockSpeed
  $service = Get-CimInstance Win32_Service -Filter "Name='AeroForgeService'" -ErrorAction SilentlyContinue | Select-Object -First 1 Name, State, StartMode, ProcessId, PathName
  $acerClasses = @(Get-CimClass -Namespace root\wmi -ErrorAction SilentlyContinue | Where-Object { $_.CimClassName -match 'AcerGamingFunction|BatteryControl|AcerGenericMethod' } | Select-Object -ExpandProperty CimClassName)
  $pipeStatus = Invoke-NamedPipeJsonRequest -Kind "getServiceStatus" -TimeoutMs 1500
  $pipeTelemetry = Invoke-NamedPipeJsonRequest -Kind "getTelemetrySnapshot" -TimeoutMs 1500
  $pipeControls = Invoke-NamedPipeJsonRequest -Kind "getControlSnapshot" -TimeoutMs 1500
  $pipeCapabilities = Invoke-NamedPipeJsonRequest -Kind "getCapabilities" -TimeoutMs 1500
  $pipeStatusJson = $null
  $pipeTelemetryJson = $null
  $pipeControlsJson = $null
  $pipeCapabilitiesJson = $null
  $pipeStatusPayload = $null
  $pipeTelemetryPayload = $null
  $pipeControlsPayload = $null
  $pipeCapabilitiesPayload = $null
  try { $pipeStatusJson = $pipeStatus | ConvertFrom-Json -ErrorAction Stop } catch {}
  try { $pipeTelemetryJson = $pipeTelemetry | ConvertFrom-Json -ErrorAction Stop } catch {}
  try { $pipeControlsJson = $pipeControls | ConvertFrom-Json -ErrorAction Stop } catch {}
  try { $pipeCapabilitiesJson = $pipeCapabilities | ConvertFrom-Json -ErrorAction Stop } catch {}
  $pipeStatusPayload = ConvertFrom-PipeJsonReply -Text $pipeStatus
  $pipeTelemetryPayload = ConvertFrom-PipeJsonReply -Text $pipeTelemetry
  $pipeControlsPayload = ConvertFrom-PipeJsonReply -Text $pipeControls
  $pipeCapabilitiesPayload = ConvertFrom-PipeJsonReply -Text $pipeCapabilities
  $powerScheme = (powercfg.exe /getactivescheme 2>&1 | Out-String).Trim()
  $fileFacts = @(Get-AeroForgeExecutableFacts -Roots $script:InstallRoots)
  $installerLogFacts = @(Get-InstallerLogFacts)

  $serviceBinary = Get-ServiceBinaryPath
  if ($service -and $serviceBinary -and -not (Test-Path -LiteralPath $serviceBinary)) {
    $flags.Add("AeroForgeService points at a missing executable: $serviceBinary")
  }

  $serviceFact = $fileFacts | Where-Object { $_.path -eq $serviceBinary -or $_.path -like "*\AeroForge\Service\bin\aeroforge-service.exe" } | Select-Object -First 1
  $controlVersions = @(
    $fileFacts |
      Where-Object { $_.name -eq "aeroforge-control.exe" -and -not [string]::IsNullOrWhiteSpace($_.productVersion) } |
      Select-Object -ExpandProperty productVersion -Unique
  )
  if (
    $serviceFact -and
    -not [string]::IsNullOrWhiteSpace($serviceFact.productVersion) -and
    $controlVersions.Count -gt 0 -and
    $controlVersions -notcontains $serviceFact.productVersion
  ) {
    $flags.Add("AeroForge app/service version mismatch: service $($serviceFact.productVersion), app versions $($controlVersions -join ', ').")
  }

  if ($processor -and $processor.Manufacturer -match 'AMD') {
    $flags.Add("AMD CPU detected; inspect cpu-and-amd-diagnostics and sampling output for frequency/power-state behavior.")
  }
  if (-not ($acerClasses -contains "AcerGamingFunction")) {
    $flags.Add("AcerGamingFunction WMI class missing.")
  }
  if (-not ($acerClasses -contains "BatteryControl")) {
    $flags.Add("BatteryControl WMI class missing.")
  }
  if (-not $script:IsAdmin) {
    $flags.Add("Collector did not run as administrator; direct Acer WMI instance probes may be incomplete.")
  }
  if (
    ($pipeStatusJson -and $pipeStatusJson.kind -eq 'error') -or
    (-not $pipeStatusJson -and ($pipeStatus -match 'failed|unavailable|error'))
  ) {
    $flags.Add("AeroForge service pipe probe failed or reported unavailable.")
  }
  if ($pipeTelemetryPayload) {
    if (($pipeTelemetryPayload.cpuFanRpm -as [int]) -eq 0 -and ($pipeTelemetryPayload.gpuFanRpm -as [int]) -eq 0) {
      $flags.Add("AeroForge telemetry reported no fan RPM values.")
    }
    if ($processor -and $processor.Manufacturer -match 'Intel' -and $null -eq $pipeTelemetryPayload.cpuPl1W -and $null -eq $pipeTelemetryPayload.cpuPl2W) {
      $flags.Add("Intel CPU detected but AeroForge telemetry did not report PL1/PL2 values.")
    }
  }
  if ($pipeControlsPayload) {
    if ($pipeControlsPayload.powerApplySupported -eq $false) {
      $flags.Add("AeroForge control snapshot reports power apply unsupported.")
    }
    if ($pipeControlsPayload.fanApplySupported -eq $false) {
      $flags.Add("AeroForge control snapshot reports fan apply unsupported.")
    }
    if ($pipeControlsPayload.fanCurveApplySupported -eq $false) {
      $flags.Add("AeroForge control snapshot reports custom fan curves unsupported.")
    }
  }

  $summary = [ordered]@{
    generatedAt = (Get-Date -Format o)
    admin = $script:IsAdmin
    os = $os
    computer = $computer
    processor = $processor
    aeroForgeService = $service
    acerWmiClasses = $acerClasses
    activePowerScheme = $powerScheme
    pipeStatus = $pipeStatus
    pipeTelemetry = $pipeTelemetry
    pipeControls = $pipeControls
    pipeCapabilities = $pipeCapabilities
    parsedPipeStatus = $pipeStatusJson
    parsedPipeTelemetry = $pipeTelemetryJson
    parsedPipeControls = $pipeControlsJson
    parsedPipeCapabilities = $pipeCapabilitiesJson
    pipeStatusPayload = $pipeStatusPayload
    pipeTelemetryPayload = $pipeTelemetryPayload
    pipeControlsPayload = $pipeControlsPayload
    pipeCapabilitiesPayload = $pipeCapabilitiesPayload
    aeroForgeExecutableFacts = $fileFacts
    installerLogs = $installerLogFacts
    flags = @($flags)
  }

  Write-TextFile -Path (Join-Path $script:BundleRoot "summary.json") -Text ($summary | ConvertTo-Json -Depth 8)
}

function Invoke-OptionalSamplingCapture {
  if ($SampleSeconds -le 0) {
    return
  }

  $interval = [Math]::Max(1, $SampleIntervalSeconds)
  $deadline = (Get-Date).AddSeconds($SampleSeconds)
  $rows = New-Object System.Collections.Generic.List[object]
  Write-LogLine "Starting optional $SampleSeconds second sampling capture at $interval second intervals."

  while ((Get-Date) -lt $deadline) {
    $cpuTotal = Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -eq "_Total" } |
      Select-Object -First 1 Name, ProcessorFrequency, PercentProcessorPerformance, PercentProcessorTime
    $pipeTelemetry = Invoke-NamedPipeJsonRequest -Kind "getTelemetrySnapshot" -TimeoutMs 1500
    $pipeControls = Invoke-NamedPipeJsonRequest -Kind "getControlSnapshot" -TimeoutMs 1500
    $acerSensors = $null
    $gaming = Get-CimInstance -Namespace root\wmi -ClassName AcerGamingFunction -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($gaming) {
      $sensorRows = @()
      foreach ($item in @(
        @{ name = "CpuTemp"; input = [uint32]0x0101 },
        @{ name = "CpuFan"; input = [uint32]0x0201 },
        @{ name = "SystemTemp"; input = [uint32]0x0301 },
        @{ name = "GpuFan"; input = [uint32]0x0601 },
        @{ name = "GpuTemp"; input = [uint32]0x0A01 }
      )) {
        try {
          $result = Invoke-CimMethod -InputObject $gaming -MethodName GetGamingSysInfo -Arguments @{ gmInput = $item.input } -ErrorAction Stop
          $sensorRows += [pscustomobject]@{
            name = $item.name
            output = $result.gmOutput
            reading = ((([uint64]$result.gmOutput) -shr 8) -band 0xFFFF)
          }
        } catch {
          $sensorRows += [pscustomobject]@{ name = $item.name; error = $_.Exception.Message }
        }
      }
      $acerSensors = $sensorRows
    }

    $rows.Add([pscustomobject]@{
      timestamp = (Get-Date -Format o)
      cpu = $cpuTotal
      acerSensors = $acerSensors
      pipeTelemetry = $pipeTelemetry
      pipeControls = $pipeControls
    })
    Start-Sleep -Seconds $interval
  }

  Write-TextFile -Path (Join-Path $script:BundleRoot "sampling.json") -Text ($rows | ConvertTo-Json -Depth 10)
  Write-LogLine "Optional sampling capture complete."
}

function ConvertTo-SafeRelativePath {
  param(
    [string]$BasePath,
    [string]$Path
  )

  $relative = $Path.Substring($BasePath.Length).TrimStart('\', '/')
  return ($relative -replace '[:*?"<>|]', '_')
}

function Copy-SafeTextTree {
  param(
    [string]$Source,
    [string]$Destination,
    [int64]$MaxBytes = 5242880
  )

  $manifest = @()
  if (-not (Test-Path -LiteralPath $Source)) {
    Write-TextFile -Path (Join-Path $Destination "_missing.txt") -Text "Source path not present: $Source"
    return
  }

  $base = (Get-Item -LiteralPath $Source).FullName
  New-Item -ItemType Directory -Force -Path $Destination | Out-Null

  Get-ChildItem -LiteralPath $Source -Recurse -Force -File -ErrorAction SilentlyContinue | ForEach-Object {
    $file = $_
    $extension = $file.Extension.ToLowerInvariant()
    $sensitiveName = $file.Name -match '(?i)(token|secret|credential|password|private[-_ ]?key)'
    $privacyHeavyPath = $file.FullName -match '(?i)\\(EBWebView|Cache|Code Cache|GPUCache|Local Storage|Session Storage|IndexedDB|Service Worker|blob_storage|DawnCache|GrShaderCache|ShaderCache)\\'
    $stagedUpdatePath = $file.FullName -match '(?i)\\updates\\(install|stage|staged|downloads?)\\'
    $textLike = $extension -in @(".json", ".jsonl", ".log", ".txt", ".csv", ".xml", ".yaml", ".yml", ".toml", ".ini", ".nfo", ".ps1", ".cmd", ".bat", ".nsh")

    if ($sensitiveName -or $privacyHeavyPath -or $stagedUpdatePath -or -not $textLike -or $file.Length -gt $MaxBytes) {
      $manifest += [pscustomobject]@{
        path = $file.FullName
        length = $file.Length
        copied = $false
        reason = if ($sensitiveName) { "sensitive filename" } elseif ($privacyHeavyPath) { "webview cache or browser storage" } elseif ($stagedUpdatePath) { "staged update payload" } elseif (-not $textLike) { "non-text extension" } else { "too large" }
      }
      return
    }

    try {
      $relative = ConvertTo-SafeRelativePath -BasePath $base -Path $file.FullName
      $target = Join-Path $Destination $relative
      $content = Get-Content -LiteralPath $file.FullName -Raw -ErrorAction Stop
      Write-TextFile -Path $target -Text $content
      $manifest += [pscustomobject]@{
        path = $file.FullName
        length = $file.Length
        copied = $true
        reason = "copied"
      }
    } catch {
      $manifest += [pscustomobject]@{
        path = $file.FullName
        length = $file.Length
        copied = $false
        reason = $_.Exception.Message
      }
    }
  }

  $manifestText = $manifest | ConvertTo-Json -Depth 4
  Write-TextFile -Path (Join-Path $Destination "_manifest.json") -Text $manifestText
}

function Get-FileInventory {
  param([string[]]$Roots)

  foreach ($root in $Roots | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -Unique) {
    "===== $root ====="
    Get-ChildItem -LiteralPath $root -Recurse -Force -File -ErrorAction SilentlyContinue |
      Where-Object { $_.Extension -in @(".exe", ".dll", ".json", ".log", ".ps1", ".cmd", ".nsh") } |
      ForEach-Object {
        $hash = $null
        if ($_.Extension -in @(".exe", ".dll", ".ps1", ".cmd")) {
          try {
            $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256 -ErrorAction Stop).Hash
          } catch {
            $hash = "hash failed: $($_.Exception.Message)"
          }
        }
        [pscustomobject]@{
          path = $_.FullName
          length = $_.Length
          modified = $_.LastWriteTimeUtc.ToString("o")
          sha256 = $hash
        }
      } | Format-Table -AutoSize -Wrap
    ""
  }
}

function Get-ServiceBinaryPath {
  try {
    $svc = Get-CimInstance Win32_Service -Filter "Name='AeroForgeService'" -ErrorAction Stop
    if (-not $svc -or -not $svc.PathName) {
      return $null
    }
    $pathName = $svc.PathName.Trim()
    if ($pathName.StartsWith('"')) {
      return ($pathName -replace '^"([^"]+)".*$', '$1')
    }
    return ($pathName -split '\s+', 2)[0]
  } catch {
    return $null
  }
}

function Get-ExecutableFact {
  param([string]$Path)

  if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path -LiteralPath $Path)) {
    return $null
  }

  try {
    $item = Get-Item -LiteralPath $Path -ErrorAction Stop
    $hash = $null
    try {
      $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256 -ErrorAction Stop).Hash
    } catch {
      $hash = "hash failed: $($_.Exception.Message)"
    }

    return [pscustomobject]@{
      name = $item.Name
      path = $item.FullName
      length = $item.Length
      modifiedUtc = $item.LastWriteTimeUtc.ToString("o")
      fileVersion = $item.VersionInfo.FileVersion
      productVersion = $item.VersionInfo.ProductVersion
      productName = $item.VersionInfo.ProductName
      companyName = $item.VersionInfo.CompanyName
      sha256 = $hash
    }
  } catch {
    return [pscustomobject]@{
      name = Split-Path -Leaf $Path
      path = $Path
      error = $_.Exception.Message
    }
  }
}

function Get-AeroForgeExecutableFacts {
  param([string[]]$Roots)

  $paths = New-Object System.Collections.Generic.List[string]
  foreach ($root in $Roots | Where-Object { $_ } | Select-Object -Unique) {
    if (Test-Path -LiteralPath $root) {
      Get-ChildItem -LiteralPath $root -Recurse -Force -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -in @("aeroforge-control.exe", "aeroforge-service.exe", "aeroforge-hotkey-helper.exe") } |
        ForEach-Object { $paths.Add($_.FullName) }
    }
  }

  $servicePath = Get-ServiceBinaryPath
  if ($servicePath) {
    $paths.Add($servicePath)
  }

  $programDataServiceBinary = Join-Path $env:ProgramData "AeroForge\Service\bin\aeroforge-service.exe"
  $paths.Add($programDataServiceBinary)

  foreach ($path in $paths | Where-Object { $_ } | Select-Object -Unique) {
    Get-ExecutableFact -Path $path
  }
}

function Get-AeroForgeShortcutTargets {
  $folders = @(
    [Environment]::GetFolderPath("Desktop"),
    [Environment]::GetFolderPath("CommonDesktopDirectory"),
    [Environment]::GetFolderPath("StartMenu"),
    [Environment]::GetFolderPath("CommonStartMenu"),
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"),
    (Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs")
  ) | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -Unique

  $shell = $null
  try {
    $shell = New-Object -ComObject WScript.Shell
  } catch {
    "WScript.Shell unavailable: $($_.Exception.Message)"
    return
  }

  foreach ($folder in $folders) {
    Get-ChildItem -LiteralPath $folder -Recurse -Force -Filter "*.lnk" -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -match 'AeroForge|Nitro|Acer|Predator' } |
      ForEach-Object {
        try {
          $shortcut = $shell.CreateShortcut($_.FullName)
          [pscustomobject]@{
            shortcut = $_.FullName
            target = $shortcut.TargetPath
            arguments = $shortcut.Arguments
            workingDirectory = $shortcut.WorkingDirectory
            iconLocation = $shortcut.IconLocation
            modifiedUtc = $_.LastWriteTimeUtc.ToString("o")
          }
        } catch {
          [pscustomobject]@{
            shortcut = $_.FullName
            error = $_.Exception.Message
          }
        }
      }
  }
}

function Get-InstallerLogFacts {
  $paths = @(
    (Join-Path $env:ProgramData "AeroForge\Service\logs\installer-service.log"),
    (Join-Path $env:TEMP "AeroForge\Service\logs\installer-service.log")
  )

  foreach ($path in $paths | Select-Object -Unique) {
    if (Test-Path -LiteralPath $path) {
      $item = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
      [pscustomobject]@{
        path = $path
        length = $item.Length
        modifiedUtc = $item.LastWriteTimeUtc.ToString("o")
      }
    } else {
      [pscustomobject]@{
        path = $path
        missing = $true
      }
    }
  }
}

function Get-RecentTextTail {
  param(
    [string]$Path,
    [int]$LineCount = 120
  )

  if (-not (Test-Path -LiteralPath $Path)) {
    "Missing: $Path"
    return
  }

  "===== $Path ====="
  Get-Item -LiteralPath $Path | Select-Object FullName, Length, LastWriteTimeUtc | Format-List
  ""
  Get-Content -LiteralPath $Path -Tail $LineCount -ErrorAction SilentlyContinue
  ""
}

function Get-RecentEventsMatching {
  param(
    [string]$LogName,
    [int]$Days = 7,
    [string]$Match = 'AeroForge|Acer|Nitro|Predator|NVIDIA|WebView|SideBySide|Application Error|Windows Error Reporting'
  )

  $start = (Get-Date).AddDays(-1 * $Days)
  Get-WinEvent -FilterHashtable @{ LogName = $LogName; StartTime = $start } -ErrorAction SilentlyContinue |
    Where-Object { ($_.ProviderName + " " + $_.Message) -match $Match } |
    Select-Object -First 200 TimeCreated, Id, LevelDisplayName, ProviderName, Message |
    Format-List
}

$stamp = Get-TimeStamp
$desktop = if (-not [string]::IsNullOrWhiteSpace($OutputRoot)) {
  $OutputRoot
} else {
  [Environment]::GetFolderPath("Desktop")
}
if ([string]::IsNullOrWhiteSpace($desktop)) {
  $desktop = $env:TEMP
}
if (-not (Test-Path -LiteralPath $desktop)) {
  New-Item -ItemType Directory -Force -Path $desktop | Out-Null
}

$script:BundleRoot = Join-Path $desktop "AeroForge-Debug-$stamp"
$script:CommandsDir = Join-Path $script:BundleRoot "commands"
$script:RuntimeDir = Join-Path $script:BundleRoot "runtime-files"
$script:MasterLog = Join-Path $script:BundleRoot "collector.log"
$script:IsAdmin = Test-IsAdmin

if (Start-ElevatedCollectorIfNeeded) {
  exit 0
}

New-Item -ItemType Directory -Force -Path $script:BundleRoot, $script:CommandsDir, $script:RuntimeDir | Out-Null

try {
  Start-Transcript -Path (Join-Path $script:BundleRoot "collector-transcript.txt") -Force | Out-Null
  $script:TranscriptStarted = $true
} catch {
  $script:TranscriptStarted = $false
}

Write-LogLine "AeroForge debug collector started."
Write-LogLine "Bundle root: $script:BundleRoot"
Write-LogLine "Running as admin: $script:IsAdmin"

$readme = @"
AeroForge Debug Bundle
======================

Generated: $(Get-Date -Format o)
Computer: $env:COMPUTERNAME
User: $env:USERNAME
Admin: $script:IsAdmin

Send the ZIP next to this folder to AeroForge support or the project maintainer.

Privacy notes:
- This collector is read-only. It does not change fan, power, firmware, EFI, display, or registry settings.
- It asks for administrator permission so read-only Acer WMI instance probes can see the same hardware surface as the AeroForge service.
- It intentionally skips images, ZIP/EXE installers, staged update packages, and filenames that look like tokens, passwords, credentials, private keys, or secrets.
- Text copied into the bundle is redacted for GitHub-token-like strings and common Authorization/password/secret fields.
- Review the ZIP before posting it publicly.

Most useful files:
- summary.json: one-page machine, service, app/service version, Acer WMI, telemetry, and failure-flag summary.
- commands\*.txt: system, service, WMI, pipe, hardware, TDP/PL, shortcut, event-log, and updater probes.
- sampling.json: optional timed telemetry capture when launched with -SampleSeconds 60.
- runtime-files\ProgramData-AeroForge-Service: AeroForge service logs and state snapshots.
- runtime-files\Temp-AeroForge-Service: fallback service-installer logs, if Windows wrote them under Temp.
- runtime-files\AppData-*: AeroForge app-owned state files, including performance.jsonl when present.
- collector.log and collector-transcript.txt: collector progress and errors.

Optional deeper capture:
- Run AeroForge-Debug-Collector.cmd -SampleSeconds 60 to capture CPU frequency, Acer sensor, and AeroForge pipe samples for intermittent AMD, power, or fan issues.
- Maintainers can use -OutputRoot "C:\Some\Temp\Folder" to write the bundle somewhere other than the Desktop.
"@
Write-TextFile -Path (Join-Path $script:BundleRoot "README.txt") -Text $readme

$cmdSource = $env:AFD_SOURCE
$cmdRoot = if ($cmdSource) { Split-Path -Parent $cmdSource } else { $PWD.Path }
$serviceBinary = Get-ServiceBinaryPath

$programFilesX86 = ${env:ProgramFiles(x86)}
$installRoots = @(
  (Join-Path $env:ProgramFiles "AeroForge Control"),
  $(if ($programFilesX86) { Join-Path $programFilesX86 "AeroForge Control" }),
  (Join-Path $env:LOCALAPPDATA "Programs\AeroForge Control"),
  $cmdRoot,
  $(if ($serviceBinary) { Split-Path -Parent $serviceBinary })
) | Where-Object { $_ }
$script:InstallRoots = $installRoots

Invoke-DiagCommand "collector context" {
  [pscustomobject]@{
    bundleRoot = $script:BundleRoot
    sourceCmd = $env:AFD_SOURCE
    sourceDir = $cmdRoot
    admin = $script:IsAdmin
    powershell = $PSVersionTable.PSVersion.ToString()
    executionPolicyProcess = Get-ExecutionPolicy -Scope Process
    executionPolicyCurrentUser = Get-ExecutionPolicy -Scope CurrentUser
    executionPolicyLocalMachine = Get-ExecutionPolicy -Scope LocalMachine
  } | Format-List
}

Invoke-DiagCommand "identity and privileges" {
  whoami /user
  whoami /groups
  whoami /priv
}

Invoke-DiagCommand "os and computer" {
  Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber, OSArchitecture, InstallDate, LastBootUpTime, LocalDateTime | Format-List
  Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer, Model, SystemType, TotalPhysicalMemory, UserName, Domain, PartOfDomain | Format-List
  Get-CimInstance Win32_BIOS | Select-Object Manufacturer, SMBIOSBIOSVersion, Version, ReleaseDate, SerialNumber | Format-List
  Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer, Product, Version, SerialNumber | Format-List
}

Invoke-DiagCommand "cpu and amd diagnostics" {
  "===== Processor identity ====="
  Get-CimInstance Win32_Processor -ErrorAction SilentlyContinue |
    Select-Object Name, Manufacturer, Description, Architecture, NumberOfCores, NumberOfLogicalProcessors, MaxClockSpeed, CurrentClockSpeed, L2CacheSize, L3CacheSize, ProcessorId, SocketDesignation |
    Format-List
  "===== Processor performance counters ====="
  Get-CimInstance Win32_PerfFormattedData_Counters_ProcessorInformation -ErrorAction SilentlyContinue |
    Select-Object Name, ProcessorFrequency, PercentProcessorPerformance, PercentProcessorTime, PercentPrivilegedTime, PercentUserTime |
    Sort-Object Name |
    Format-Table -AutoSize -Wrap
  "===== AMD/processor/PPM drivers ====="
  Get-CimInstance Win32_PnPSignedDriver -ErrorAction SilentlyContinue |
    Where-Object { ($_.DeviceName + " " + $_.Manufacturer + " " + $_.DriverProviderName + " " + $_.InfName) -match 'AMD|Ryzen|Processor|PPM|ACPI|Chipset' } |
    Select-Object DeviceName, Manufacturer, DriverProviderName, DriverVersion, DriverDate, InfName, DeviceID |
    Sort-Object DeviceName |
    Format-Table -AutoSize -Wrap
  "===== AMD/processor/PPM services ====="
  Get-CimInstance Win32_SystemDriver -ErrorAction SilentlyContinue |
    Where-Object { ($_.Name + " " + $_.DisplayName + " " + $_.PathName) -match 'amd|ryzen|ppm|processor|acpi' } |
    Select-Object Name, DisplayName, State, StartMode, PathName |
    Sort-Object Name |
    Format-Table -AutoSize -Wrap
  "===== Relevant service registry ====="
  reg.exe query "HKLM\SYSTEM\CurrentControlSet\Services\amdppm" /s
  reg.exe query "HKLM\SYSTEM\CurrentControlSet\Services\Processor" /s
  reg.exe query "HKLM\SYSTEM\CurrentControlSet\Services\intelppm" /s
}

Invoke-DiagCommand "installed apps filtered" {
  Get-RegistryInstalledApps | Sort-Object DisplayName, DisplayVersion | Format-List
}

Invoke-DiagCommand "appx packages filtered" {
  $appxPackages = if ($script:IsAdmin) {
    Get-AppxPackage -AllUsers -ErrorAction SilentlyContinue
  } else {
    "Collector is not elevated; collecting current-user AppX packages only. Re-run without -NoElevate for all-user AppX package inventory."
    Get-AppxPackage -ErrorAction SilentlyContinue
  }

  $appxPackages |
    Where-Object { ($_.Name + " " + $_.PackageFullName + " " + $_.Publisher) -match 'Acer|Nitro|Predator|Quick|AeroForge|WebView' } |
    Select-Object Name, PackageFullName, Publisher, InstallLocation, Status, SignatureKind |
    Format-List
}

Invoke-DiagCommand "aeroforge service sc" {
  sc.exe queryex AeroForgeService
  sc.exe qc AeroForgeService
  sc.exe sdshow AeroForgeService
}

Invoke-DiagCommand "aeroforge executable versions" {
  Get-AeroForgeExecutableFacts -Roots $installRoots | Format-List
}

Invoke-DiagCommand "aeroforge installer logs" {
  Get-InstallerLogFacts | Format-List
  ""
  Get-RecentTextTail -Path (Join-Path $env:ProgramData "AeroForge\Service\logs\installer-service.log")
  Get-RecentTextTail -Path (Join-Path $env:TEMP "AeroForge\Service\logs\installer-service.log")
}

Invoke-DiagCommand "aeroforge shortcuts and launch targets" {
  Get-AeroForgeShortcutTargets | Sort-Object shortcut | Format-List
}

Invoke-DiagCommand "aeroforge service state snapshots" {
  $stateRoot = Join-Path $env:ProgramData "AeroForge\Service\state"
  if (-not (Test-Path -LiteralPath $stateRoot)) {
    "Missing: $stateRoot"
    return
  }

  Get-ChildItem -LiteralPath $stateRoot -Recurse -Force -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension -in @(".json", ".jsonl", ".log") } |
    Sort-Object FullName |
    ForEach-Object {
      "===== $($_.FullName) ====="
      $_ | Select-Object FullName, Length, LastWriteTimeUtc | Format-List
      Get-Content -LiteralPath $_.FullName -Tail 160 -ErrorAction SilentlyContinue
      ""
    }
}

Invoke-DiagCommand "services filtered" {
  Get-CimInstance Win32_Service |
    Where-Object { ($_.Name + " " + $_.DisplayName + " " + $_.PathName) -match 'AeroForge|Acer|Nitro|Predator|Quick Access|QuickAccess|NVIDIA|NVDisplay|NVContainer|WebView|WinRing|OpenLibSys' } |
    Select-Object Name, DisplayName, State, Status, StartMode, StartName, ProcessId, PathName |
    Sort-Object Name |
    Format-List
}

Invoke-DiagCommand "processes filtered" {
  Get-Process -ErrorAction SilentlyContinue |
    Where-Object { $_.ProcessName -match 'aeroforge|webview|msedgewebview2|acer|nitro|predator|nvidia|nvcontainer|quick|winring|openlibsys' } |
    Select-Object ProcessName, Id, CPU, WorkingSet64, PrivateMemorySize64, StartTime, Path |
    Sort-Object ProcessName, Id |
    Format-Table -AutoSize -Wrap
}

Invoke-DiagCommand "process command lines filtered" {
  Get-CimInstance Win32_Process |
    Where-Object { ($_.Name + " " + $_.ExecutablePath + " " + $_.CommandLine) -match 'aeroforge|webview|msedgewebview2|acer|nitro|predator|nvidia|nvcontainer|quick|winring|openlibsys' } |
    Select-Object ProcessId, ParentProcessId, Name, ExecutablePath, CommandLine |
    Sort-Object Name, ProcessId |
    Format-List
}

Invoke-DiagCommand "named pipe read-only probe" {
  Get-AeroForgePipeProbe
}

Invoke-DiagCommand "aeroforge installed file inventory" {
  Get-FileInventory -Roots $installRoots
}

Invoke-DiagCommand "startup entries" {
  "===== Registry Run HKCU ====="
  reg.exe query "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
  "===== Registry Run HKLM ====="
  reg.exe query "HKLM\Software\Microsoft\Windows\CurrentVersion\Run"
  "===== Registry Run HKLM WOW6432Node ====="
  reg.exe query "HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"
  "===== Startup folders ====="
  Get-ChildItem -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup", "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Startup" -ErrorAction SilentlyContinue |
    Select-Object FullName, Length, LastWriteTime |
    Format-Table -AutoSize -Wrap
}

Invoke-DiagCommand "scheduled tasks filtered" {
  Get-ScheduledTask -ErrorAction SilentlyContinue |
    Where-Object { ($_.TaskName + " " + $_.TaskPath + " " + $_.Description) -match 'AeroForge|Acer|Nitro|Predator|Quick|NVIDIA' } |
    ForEach-Object {
      $info = $_ | Get-ScheduledTaskInfo -ErrorAction SilentlyContinue
      [pscustomobject]@{
        TaskName = $_.TaskName
        TaskPath = $_.TaskPath
        State = $_.State
        LastRunTime = $info.LastRunTime
        LastTaskResult = $info.LastTaskResult
        NextRunTime = $info.NextRunTime
        Actions = ($_.Actions | ForEach-Object { $_.Execute + " " + $_.Arguments }) -join " ; "
      }
    } | Format-List
}

Invoke-DiagCommand "windows power state" {
  powercfg.exe /getactivescheme
  powercfg.exe /a
  powercfg.exe /requests
  powercfg.exe /query SCHEME_CURRENT SUB_PROCESSOR
  Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Format-List *
}

Invoke-DiagCommand "display and gpu" {
  Get-CimInstance Win32_VideoController | Select-Object Name, AdapterCompatibility, DriverVersion, DriverDate, CurrentRefreshRate, CurrentHorizontalResolution, CurrentVerticalResolution, VideoModeDescription, Status | Format-List
  Get-CimInstance Win32_DesktopMonitor -ErrorAction SilentlyContinue | Select-Object Name, MonitorType, MonitorManufacturer, ScreenWidth, ScreenHeight, Status | Format-List
  Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID -ErrorAction SilentlyContinue | Format-List *
}

Invoke-DiagCommand "nvidia smi" {
  $nvidiaSmi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
  if ($nvidiaSmi) {
    & $nvidiaSmi.Source -q
  } else {
    "nvidia-smi.exe not found in PATH."
  }
}

Invoke-DiagCommand "nvidia power limits" {
  $nvidiaSmi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
  if ($nvidiaSmi) {
    "===== nvidia-smi -q -d POWER ====="
    & $nvidiaSmi.Source -q -d POWER
    ""
    "===== nvidia-smi query-gpu power ====="
    & $nvidiaSmi.Source --query-gpu=name,pci.bus_id,power.draw,power.limit,power.default_limit,power.min_limit,power.max_limit,clocks.current.graphics,clocks.current.memory,temperature.gpu,utilization.gpu --format=csv,noheader,nounits
  } else {
    "nvidia-smi.exe not found in PATH."
  }
}

Invoke-DiagCommand "acer wmi classes" {
  "===== Acer classes under ROOT\WMI ====="
  Get-CimClass -Namespace root\wmi -ErrorAction SilentlyContinue |
    Where-Object { $_.CimClassName -match 'Acer|Gaming|Nitro|Predator' } |
    Select-Object CimClassName, CimSuperClassName, CimClassMethods, CimClassProperties |
    Format-List
  "===== AcerGamingFunction instance ====="
  Get-CimInstance -Namespace root\wmi -ClassName AcerGamingFunction -ErrorAction SilentlyContinue | Format-List *
}

Invoke-DiagCommand "acer direct wmi read-only probes" {
  Get-AcerDirectWmiReadOnlyProbe
}

Invoke-DiagCommand "pnp devices filtered" {
  Get-PnpDevice -ErrorAction SilentlyContinue |
    Where-Object { ($_.FriendlyName + " " + $_.InstanceId + " " + $_.Class) -match 'Acer|Nitro|Predator|NVIDIA|HID|Battery|Display|Monitor|ACPI|WMI|Thermal|WinRing|OpenLibSys' } |
    Select-Object Class, FriendlyName, InstanceId, Status, Problem |
    Sort-Object Class, FriendlyName |
    Format-Table -AutoSize -Wrap
}

Invoke-DiagCommand "driver inventory filtered" {
  pnputil.exe /enum-drivers
  driverquery.exe /v /fo table
}

Invoke-DiagCommand "webview2 and edge update registry" {
  reg.exe query "HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients" /s
  reg.exe query "HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients" /s
  reg.exe query "HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients" /s
}

Invoke-DiagCommand "network update reachability" {
  Resolve-DnsName api.github.com -ErrorAction SilentlyContinue
  Resolve-DnsName github.com -ErrorAction SilentlyContinue
  try {
    $response = Invoke-WebRequest -UseBasicParsing -Method Head -Uri "https://api.github.com" -TimeoutSec 15
    [pscustomobject]@{
      Uri = "https://api.github.com"
      StatusCode = $response.StatusCode
      StatusDescription = $response.StatusDescription
      Server = $response.Headers.Server
      RateLimitRemaining = $response.Headers["X-RateLimit-Remaining"]
    } | Format-List
  } catch {
    "GitHub API HTTPS probe failed: $($_.Exception.Message)"
  }
}

Invoke-DiagCommand "windows defender status" {
  Get-MpComputerStatus -ErrorAction SilentlyContinue | Format-List *
  Get-MpThreatDetection -ErrorAction SilentlyContinue | Select-Object -First 50 | Format-List *
}

Invoke-DiagCommand "event log system relevant" {
  Get-RecentEventsMatching -LogName "System" -Days 7
}

Invoke-DiagCommand "event log application relevant" {
  Get-RecentEventsMatching -LogName "Application" -Days 7
}

Invoke-DiagCommand "event log defender relevant" {
  Get-RecentEventsMatching -LogName "Microsoft-Windows-Windows Defender/Operational" -Days 14 -Match 'AeroForge|aeroforge|Nitro|Acer|PUA|quarantine|blocked|allowed|Controlled Folder|threat'
}

Invoke-DiagCommand "event log app model relevant" {
  Get-RecentEventsMatching -LogName "Microsoft-Windows-AppModel-Runtime/Admin" -Days 7
}

Invoke-DiagCommand "volumes boot and efi read-only" {
  Get-Volume | Select-Object DriveLetter, FileSystemLabel, FileSystem, DriveType, HealthStatus, OperationalStatus, SizeRemaining, Size | Format-Table -AutoSize -Wrap
  Get-Partition | Select-Object DiskNumber, PartitionNumber, DriveLetter, Type, GptType, IsBoot, IsSystem, Size | Format-Table -AutoSize -Wrap
  mountvol.exe
  if ($script:IsAdmin) {
    bcdedit.exe /enum firmware
  } else {
    "Collector is not elevated; skipped bcdedit /enum firmware because Windows usually denies firmware BCD reads without administrator rights."
  }
}

$batteryReport = Join-Path $script:CommandsDir "battery-report.html"
Invoke-DiagCommand "battery report" {
  powercfg.exe /batteryreport /output "$batteryReport"
  if (Test-Path -LiteralPath $batteryReport) {
    "Battery report written to $batteryReport"
  }
}

$programDataService = Join-Path $env:ProgramData "AeroForge\Service"
$tempService = Join-Path $env:TEMP "AeroForge\Service"
$appDataCandidates = @(
  (Join-Path $env:APPDATA "com.noah.aeroforgecontrol"),
  (Join-Path $env:LOCALAPPDATA "com.noah.aeroforgecontrol"),
  (Join-Path $env:APPDATA "AeroForge Control"),
  (Join-Path $env:LOCALAPPDATA "AeroForge Control")
)

Write-LogLine "Copying safe AeroForge service runtime files."
Copy-SafeTextTree -Source $programDataService -Destination (Join-Path $script:RuntimeDir "ProgramData-AeroForge-Service")
Write-LogLine "Copying safe fallback service installer files."
Copy-SafeTextTree -Source $tempService -Destination (Join-Path $script:RuntimeDir "Temp-AeroForge-Service")

foreach ($candidate in $appDataCandidates) {
  $leaf = Split-Path -Leaf $candidate
  $parent = Split-Path -Leaf (Split-Path -Parent $candidate)
  $dest = Join-Path $script:RuntimeDir ("AppData-{0}-{1}" -f $parent, $leaf)
  Write-LogLine "Copying safe app state from $candidate"
  Copy-SafeTextTree -Source $candidate -Destination $dest
}

Invoke-DiagCommand "runtime file tree summary" {
  Get-ChildItem -LiteralPath $script:RuntimeDir -Recurse -Force -ErrorAction SilentlyContinue |
    Select-Object FullName, Length, LastWriteTime |
    Format-Table -AutoSize -Wrap
}

Write-LogLine "Writing summary.json."
Write-DebugSummaryJson

Invoke-OptionalSamplingCapture

if ($script:TranscriptStarted) {
  Write-LogLine "Stopping transcript before ZIP creation."
  try {
    Stop-Transcript | Out-Null
  } catch {
  }
  $script:TranscriptStarted = $false
}

$zipPath = "$script:BundleRoot.zip"
try {
  if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
  }
  Compress-Archive -Path (Join-Path $script:BundleRoot "*") -DestinationPath $zipPath -Force -ErrorAction Stop
  Write-LogLine "Created ZIP: $zipPath"
} catch {
  Write-LogLine "ZIP creation failed: $($_.Exception.Message)"
}

if ($script:TranscriptStarted) {
  try {
    Stop-Transcript | Out-Null
  } catch {
  }
}

Write-Host ""
Write-Host "AeroForge debug collection complete."
Write-Host "Folder: $script:BundleRoot"
if (Test-Path -LiteralPath $zipPath) {
  Write-Host "ZIP:    $zipPath"
} else {
  Write-Host "ZIP was not created. Send the folder instead."
}
Write-Host ""
Write-Host "Review the ZIP before posting it publicly."

if (-not $NoPause) {
  Read-Host "Press Enter to close"
}
