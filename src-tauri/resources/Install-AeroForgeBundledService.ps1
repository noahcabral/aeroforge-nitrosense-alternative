param(
  [switch]$Uninstall,
  [switch]$RepairPrerequisites,
  [switch]$InstallPawnIO,
  [string]$ServiceSource = (Join-Path $PSScriptRoot 'aeroforge-service.exe')
)

$ErrorActionPreference = 'Stop'

$serviceName = 'AeroForgeService'
$displayName = 'AeroForge Service'
$wmiServiceName = 'Winmgmt'
$serviceRoot = Join-Path $env:ProgramData 'AeroForge\Service'
$serviceBinDir = Join-Path $serviceRoot 'bin'
$serviceDriverDir = Join-Path $serviceRoot 'drivers'
$serviceLogDir = Join-Path $serviceRoot 'logs'
$installedExe = Join-Path $serviceBinDir 'aeroforge-service.exe'
$healerTaskName = 'AeroForge Service Healer'
$healerScript = Join-Path $serviceRoot 'Start-AeroForgeServiceHealer.ps1'
$pawnIoIntelMsrInstalled = Join-Path $serviceDriverDir 'IntelMSR.bin'
$pawnIoDllEnv = 'AEROFORGE_PAWNIO_DLL'
$pawnIoModuleEnv = 'AEROFORGE_PAWNIO_MODULE'
$pawnIoEnableEnv = 'AEROFORGE_ENABLE_PAWNIO'
$pawnIoSetupStdoutLog = Join-Path $serviceLogDir 'pawnio-setup.stdout.log'
$pawnIoSetupStderrLog = Join-Path $serviceLogDir 'pawnio-setup.stderr.log'
$script:LogFile = Join-Path $serviceLogDir 'installer-service.log'

function Initialize-InstallLog {
  try {
    New-Item -ItemType Directory -Force -Path $serviceLogDir | Out-Null
  } catch {
    $fallbackRoot = Join-Path $env:TEMP 'AeroForge\Service\logs'
    New-Item -ItemType Directory -Force -Path $fallbackRoot | Out-Null
    $script:LogFile = Join-Path $fallbackRoot 'installer-service.log'
  }
}

function Write-InstallLog {
  param([string]$Message)

  $line = '[{0}] {1}' -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), $Message
  Write-Output $line
  Add-Content -LiteralPath $script:LogFile -Value $line -Encoding UTF8
}

function Resolve-BundledResource {
  param(
    [Parameter(Mandatory = $true)][string]$FileName,
    [string]$SourceRelativePath
  )

  $candidates = New-Object System.Collections.Generic.List[string]
  [void]$candidates.Add((Join-Path $PSScriptRoot $FileName))
  if ($SourceRelativePath) {
    $projectRoot = Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..') -ErrorAction SilentlyContinue
    if ($projectRoot) {
      [void]$candidates.Add((Join-Path $projectRoot.Path $SourceRelativePath))
    }
  }

  foreach ($candidate in $candidates) {
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
      return (Resolve-Path -LiteralPath $candidate).Path
    }
  }

  return $null
}

function Fail-Install {
  param(
    [string]$Message,
    [int]$Code = 1
  )

  Write-InstallLog "ERROR: $Message"
  [Console]::Error.WriteLine($Message)
  exit $Code
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

function Write-PawnIoInstallerEnvironment {
  $pawnIoSetupSource = Resolve-BundledResource -FileName 'PawnIO_setup.exe' -SourceRelativePath 'third_party\pawnio\PawnIO_setup.exe'
  Write-InstallLog "PawnIO installer diagnostics: admin=$(Test-IsAdmin) user=$([Security.Principal.WindowsIdentity]::GetCurrent().Name) os=$([Environment]::OSVersion.VersionString)."

  if ($pawnIoSetupSource -and (Test-Path -LiteralPath $pawnIoSetupSource -PathType Leaf)) {
    try {
      $hash = Get-FileHash -LiteralPath $pawnIoSetupSource -Algorithm SHA256 -ErrorAction Stop
      Write-InstallLog "PawnIO setup file: path=$pawnIoSetupSource size=$((Get-Item -LiteralPath $pawnIoSetupSource).Length) sha256=$($hash.Hash)."
    } catch {
      Write-InstallLog "PawnIO setup hash probe failed: $($_.Exception.Message)"
    }

    try {
      $signature = Get-AuthenticodeSignature -LiteralPath $pawnIoSetupSource -ErrorAction Stop
      Write-InstallLog "PawnIO setup signature: status=$($signature.Status) signer=$($signature.SignerCertificate.Subject)."
    } catch {
      Write-InstallLog "PawnIO setup signature probe failed: $($_.Exception.Message)"
    }
  }

  try {
    $deviceGuard = Get-CimInstance -Namespace root\Microsoft\Windows\DeviceGuard -ClassName Win32_DeviceGuard -ErrorAction Stop
    Write-InstallLog "DeviceGuard: VBS=$($deviceGuard.VirtualizationBasedSecurityStatus) configured=$($deviceGuard.SecurityServicesConfigured -join ',') running=$($deviceGuard.SecurityServicesRunning -join ',') CI=$($deviceGuard.CodeIntegrityPolicyEnforcementStatus) UMCI=$($deviceGuard.UserModeCodeIntegrityPolicyEnforcementStatus)."
  } catch {
    Write-InstallLog "DeviceGuard probe failed: $($_.Exception.Message)"
  }

  try {
    $hvci = Get-ItemProperty -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity' -ErrorAction Stop
    Write-InstallLog "HVCI registry: Enabled=$($hvci.Enabled) Locked=$($hvci.Locked) WasEnabledBy=$($hvci.WasEnabledBy)."
  } catch {
    Write-InstallLog "HVCI registry probe failed: $($_.Exception.Message)"
  }
}

function Write-RecentPawnIoSetupTranscript {
  foreach ($path in @($pawnIoSetupStdoutLog, $pawnIoSetupStderrLog)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
      Write-InstallLog "PawnIO setup transcript missing: $path"
      continue
    }

    Write-InstallLog "PawnIO setup transcript: $path"
    try {
      $lines = @(Get-Content -LiteralPath $path -Tail 80 -ErrorAction Stop)
      if ($lines.Count -eq 0) {
        Write-InstallLog "  <empty>"
      } else {
        foreach ($line in $lines) {
          Write-InstallLog "  $line"
        }
      }
    } catch {
      Write-InstallLog "  transcript read failed: $($_.Exception.Message)"
    }
  }
}

function Protect-ServiceRoot {
  # CreateProcess and LoadLibrary search the application directory first, and
  # ProgramData grants Users create-file by inheritance. Without this ACL any
  # local user could plant a powershell.exe beside the service and run as SYSTEM.
  & icacls $serviceRoot /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "icacls failed to restrict $serviceRoot (exit code $LASTEXITCODE)."
  }
}

function Invoke-Sc {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Arguments,
    [int[]]$AllowedExitCodes = @(0)
  )

  Write-InstallLog "sc.exe $($Arguments -join ' ')"
  $output = & sc.exe @Arguments 2>&1
  $exitCode = $LASTEXITCODE
  if ($output) {
    foreach ($line in $output) {
      Write-InstallLog "  $line"
    }
  }
  if ($AllowedExitCodes -notcontains $exitCode) {
    throw "sc.exe $($Arguments -join ' ') failed with exit code $exitCode."
  }
  return $output
}

function Get-ScText {
  param([Parameter(Mandatory = $true)][string[]]$Arguments)

  $output = Invoke-Sc -Arguments $Arguments -AllowedExitCodes @(0, 1060)
  if ($null -eq $output) {
    return ''
  }
  return ($output | Out-String)
}

function Get-WindowsServiceStartType {
  param([Parameter(Mandatory = $true)][string]$Name)

  $text = Get-ScText -Arguments @('qc', $Name)
  foreach ($line in ($text -split "`r?`n")) {
    if ($line -match 'START_TYPE\s*:\s*\d+\s+(.+)$') {
      return $Matches[1].Trim()
    }
  }
  return 'UNKNOWN'
}

function Test-WindowsServiceRunning {
  param([Parameter(Mandatory = $true)][string]$Name)

  $text = Get-ScText -Arguments @('query', $Name)
  return ($text -match 'STATE\s*:\s*\d+\s+RUNNING')
}

function Wait-WindowsServiceRunning {
  param(
    [Parameter(Mandatory = $true)][string]$Name,
    [int]$TimeoutSeconds = 20
  )

  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    if (Test-WindowsServiceRunning -Name $Name) {
      return $true
    }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)

  return $false
}

function Repair-WindowsManagementInstrumentation {
  $startType = Get-WindowsServiceStartType -Name $wmiServiceName
  $wasRunning = Test-WindowsServiceRunning -Name $wmiServiceName
  Write-InstallLog "WMI prerequisite before repair: service=$wmiServiceName startType=$startType running=$wasRunning."

  if ($startType -match 'DISABLED') {
    Write-InstallLog "Enabling Windows Management Instrumentation because Acer WMI control paths require it."
    Invoke-Sc -Arguments @('config', $wmiServiceName, 'start=', 'auto') | Out-Null
  }

  if (-not (Test-WindowsServiceRunning -Name $wmiServiceName)) {
    Write-InstallLog "Starting Windows Management Instrumentation."
    Invoke-Sc -Arguments @('start', $wmiServiceName) -AllowedExitCodes @(0, 1056) | Out-Null
    if (-not (Wait-WindowsServiceRunning -Name $wmiServiceName -TimeoutSeconds 25)) {
      Invoke-Sc -Arguments @('query', $wmiServiceName) -AllowedExitCodes @(0, 1060) | Out-Null
      throw "Windows Management Instrumentation did not reach Running state before timeout."
    }
  }

  $finalStartType = Get-WindowsServiceStartType -Name $wmiServiceName
  $finalRunning = Test-WindowsServiceRunning -Name $wmiServiceName
  Write-InstallLog "WMI prerequisite after repair: service=$wmiServiceName startType=$finalStartType running=$finalRunning."
}

function Get-AeroForgeServicePid {
  try {
    $output = & sc.exe queryex $serviceName 2>&1
    foreach ($line in $output) {
      if ($line -match 'PID\s*:\s*(\d+)') {
        return [int]$Matches[1]
      }
    }
  } catch {
    Write-InstallLog "Unable to query $serviceName PID: $($_.Exception.Message)"
  }

  return 0
}

function Stop-AeroForgeServiceProcesses {
  param([string]$Reason = 'service binary update')

  $pids = @()
  $servicePid = Get-AeroForgeServicePid
  if ($servicePid -gt 0) {
    $pids += $servicePid
  }

  $namedProcesses = Get-Process aeroforge-service -ErrorAction SilentlyContinue
  foreach ($process in $namedProcesses) {
    if ($pids -notcontains $process.Id) {
      $pids += $process.Id
    }
  }

  foreach ($targetPid in $pids | Select-Object -Unique) {
    try {
      Write-InstallLog "Terminating aeroforge-service PID $targetPid for $Reason."
      Stop-Process -Id $targetPid -Force -ErrorAction Stop
    } catch {
      Write-InstallLog "Stop-Process failed for PID ${targetPid}: $($_.Exception.Message)"
      & taskkill.exe /PID $targetPid /F /T 2>&1 | ForEach-Object {
        Write-InstallLog "  taskkill: $_"
      }
    }
  }

  $deadline = (Get-Date).AddSeconds(15)
  do {
    $remaining = Get-LiveAeroForgeServiceProcesses
    if (-not $remaining) {
      return
    }

    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)

  $remaining = Get-LiveAeroForgeServiceProcesses
  if (-not $remaining) {
    return
  }

  $remainingIds = ($remaining | Select-Object -ExpandProperty ProcessId) -join ', '
  throw "aeroforge-service process still running after termination wait. Remaining PID(s): $remainingIds"
}

function Get-LiveAeroForgeServiceProcesses {
  $processes = @(Get-CimInstance Win32_Process -Filter "Name='aeroforge-service.exe'" -ErrorAction SilentlyContinue)
  if (-not $processes) {
    return @()
  }

  $live = @()
  foreach ($process in $processes) {
    $threadCount = 0
    $handleCount = 0
    if ($null -ne $process.ThreadCount) {
      $threadCount = [int]$process.ThreadCount
    }
    if ($null -ne $process.HandleCount) {
      $handleCount = [int]$process.HandleCount
    }

    if ($threadCount -le 0 -and $handleCount -le 0) {
      Write-InstallLog "Ignoring stale terminated aeroforge-service PID $($process.ProcessId) with 0 threads and 0 handles." | Out-Null
      continue
    }

    $live += $process
  }

  return $live
}

function Copy-WithRetry {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Source,
    [Parameter(Mandatory = $true)]
    [string]$Destination
  )

  $deadline = (Get-Date).AddSeconds(20)
  do {
    try {
      Copy-Item -LiteralPath $Source -Destination $Destination -Force
      Write-InstallLog "Copied service binary to $Destination."
      return
    } catch {
      if ((Get-Date) -ge $deadline) {
        throw "Could not copy $Source to $Destination after retrying: $($_.Exception.Message)"
      }
      Write-InstallLog "Copy retry after error: $($_.Exception.Message)"
      Stop-AeroForgeServiceProcesses -Reason 'locked service binary copy retry'
      Start-Sleep -Milliseconds 500
    }
  } while ($true)
}

function Get-ExistingFile {
  param([string[]]$Candidates)

  foreach ($candidate in $Candidates) {
    if ([string]::IsNullOrWhiteSpace($candidate)) {
      continue
    }
    if (Test-Path -LiteralPath $candidate -PathType Leaf) {
      return $candidate
    }
  }

  return $null
}

function Get-PawnIoInstallLocations {
  $locations = New-Object System.Collections.Generic.List[string]

  foreach ($root in @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
  )) {
    try {
      Get-ItemProperty -Path $root -ErrorAction SilentlyContinue |
        Where-Object { ($_.DisplayName -as [string]) -match 'PawnIO' } |
        ForEach-Object {
          foreach ($property in @('InstallLocation', 'DisplayIcon')) {
            $value = $_.$property -as [string]
            if ([string]::IsNullOrWhiteSpace($value)) {
              continue
            }

            $trimmed = $value.Trim('"')
            if ($property -eq 'DisplayIcon') {
              $trimmed = Split-Path -Parent $trimmed
            }
            if (-not [string]::IsNullOrWhiteSpace($trimmed)) {
              [void]$locations.Add($trimmed)
            }
          }
        }
    } catch {
      Write-InstallLog "PawnIO registry probe failed at ${root}: $($_.Exception.Message)"
    }
  }

  foreach ($path in @(
    (Join-Path $env:ProgramFiles 'PawnIO'),
    (Join-Path ${env:ProgramFiles(x86)} 'PawnIO'),
    $env:SystemRoot
  )) {
    if (-not [string]::IsNullOrWhiteSpace($path)) {
      [void]$locations.Add($path)
    }
  }

  return $locations | Where-Object { -not [string]::IsNullOrWhiteSpace($_) } | Select-Object -Unique
}

function Resolve-PawnIoDll {
  $candidates = New-Object System.Collections.Generic.List[string]
  foreach ($location in Get-PawnIoInstallLocations) {
    [void]$candidates.Add((Join-Path $location 'PawnIOLib.dll'))
  }
  [void]$candidates.Add((Join-Path $env:SystemRoot 'System32\PawnIOLib.dll'))

  return Get-ExistingFile -Candidates ($candidates | Select-Object -Unique)
}

function Install-PawnIoRuntime {
  $pawnIoSetupSource = Resolve-BundledResource -FileName 'PawnIO_setup.exe' -SourceRelativePath 'third_party\pawnio\PawnIO_setup.exe'

  if (-not $InstallPawnIO) {
    Write-InstallLog "PawnIO runtime install was not requested."
    return $false
  }

  Write-PawnIoInstallerEnvironment

  if (-not ($pawnIoSetupSource -and (Test-Path -LiteralPath $pawnIoSetupSource -PathType Leaf))) {
    Write-InstallLog "WARN: PawnIO runtime install was requested, but bundled setup was not found at $pawnIoSetupSource. AeroForgeService install will continue without CPU package power and PL readback."
    return $false
  }

  try {
    Remove-Item -LiteralPath $pawnIoSetupStdoutLog, $pawnIoSetupStderrLog -Force -ErrorAction SilentlyContinue
    Write-InstallLog "Installing bundled PawnIO runtime from $pawnIoSetupSource."
    $process = Start-Process -FilePath $pawnIoSetupSource -ArgumentList @('-install', '-silent') -Wait -PassThru -WindowStyle Hidden -RedirectStandardOutput $pawnIoSetupStdoutLog -RedirectStandardError $pawnIoSetupStderrLog
    Write-InstallLog "PawnIO setup exited with code $($process.ExitCode)."
    Write-RecentPawnIoSetupTranscript
    if ($process.ExitCode -ne 0) {
      Write-InstallLog "WARN: PawnIO setup failed with exit code $($process.ExitCode). AeroForgeService install will continue without CPU package power and PL readback."
      return $false
    }
    return $true
  } catch {
    Write-InstallLog "WARN: PawnIO setup launch failed: $($_.Exception.Message). AeroForgeService install will continue without CPU package power and PL readback."
    Write-RecentPawnIoSetupTranscript
    return $false
  }
}

function Set-MachineEnvironmentVariable {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Name,
    [AllowNull()]
    [string]$Value
  )

  [Environment]::SetEnvironmentVariable($Name, $Value, 'Machine')
  if ($null -eq $Value) {
    Write-InstallLog "Cleared machine environment variable $Name."
  } else {
    Write-InstallLog "Set machine environment variable $Name=$Value."
  }
}

function Get-AeroForgeService {
  Get-Service -Name $serviceName -ErrorAction SilentlyContinue
}

function Wait-ServiceDeleted {
  param([int]$TimeoutSeconds = 25)

  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  do {
    if (-not (Get-AeroForgeService)) {
      return $true
    }
    if (Test-AeroForgeServiceMarkedForDeletion) {
      return $true
    }
    Stop-AeroForgeServiceProcesses -Reason 'delete wait cleanup' | Out-Null
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)

  return $false
}

function Test-AeroForgeServiceMarkedForDeletion {
  $output = & sc.exe queryex $serviceName 2>&1
  $exitCode = $LASTEXITCODE
  $text = ($output | Out-String)

  return $exitCode -eq 1072 -or $text -match 'marked for deletion'
}

function Stop-AeroForgeService {
  $service = Get-AeroForgeService
  if (-not $service) {
    Stop-AeroForgeServiceProcesses -Reason 'orphan service process cleanup'
    return
  }

  if ($service.Status -ne 'Stopped') {
    Write-InstallLog "Stopping $serviceName."
    Invoke-Sc -Arguments @('stop', $serviceName) -AllowedExitCodes @(0, 1062) | Out-Null
    $deadline = (Get-Date).AddSeconds(25)
    do {
      $service = Get-AeroForgeService
      if (-not $service -or $service.Status -eq 'Stopped') {
        break
      }
      Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)

    $service = Get-AeroForgeService
    if ($service -and $service.Status -ne 'Stopped') {
      Write-InstallLog "$serviceName did not stop cleanly; terminating service process if still present."
    }
  }

  Stop-AeroForgeServiceProcesses -Reason 'service stop'
}

function Disable-AeroForgeServiceForRemoval {
  if (-not (Get-AeroForgeService)) {
    return
  }

  Write-InstallLog "Disabling $serviceName before removal."
  Invoke-Sc -Arguments @('config', $serviceName, 'start=', 'disabled') -AllowedExitCodes @(0, 1060) | Out-Null
}

function Remove-AeroForgeServiceRegistration {
  if (-not (Get-AeroForgeService)) {
    return
  }

  $attempts = 0
  do {
    $attempts += 1
    Write-InstallLog "Deleting $serviceName registration, attempt $attempts."
    Invoke-Sc -Arguments @('delete', $serviceName) -AllowedExitCodes @(0, 1060, 1072) | Out-Null
    if (Wait-ServiceDeleted -TimeoutSeconds 10) {
      return
    }
    Stop-AeroForgeServiceProcesses -Reason 'service delete retry'
    Start-Sleep -Seconds 1
  } while ($attempts -lt 3)

  Invoke-Sc -Arguments @('queryex', $serviceName) -AllowedExitCodes @(0, 1060, 1072) | Out-Null
  if (Test-AeroForgeServiceMarkedForDeletion) {
    Write-InstallLog "$serviceName is marked for deletion. Windows will finish removal after reboot."
    return
  }

  Write-InstallLog "WARN: $serviceName is still present after delete attempts. A reboot may be required before reinstalling."
}

function Wait-ServiceRunning {
  $deadline = (Get-Date).AddSeconds(25)
  do {
    $service = Get-AeroForgeService
    if ($service -and $service.Status -eq 'Running') {
      return
    }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)

  Invoke-Sc -Arguments @('queryex', $serviceName) -AllowedExitCodes @(0, 1060) | Out-Null
  throw "$serviceName did not reach Running state before timeout."
}

function Install-AeroForgeServiceHealerTask {
  New-Item -ItemType Directory -Force -Path $serviceRoot | Out-Null

  $healerBody = @"
`$ErrorActionPreference = 'SilentlyContinue'
try {
  Set-Service -Name '$wmiServiceName' -StartupType Automatic -ErrorAction SilentlyContinue
} catch {}
try {
  Start-Service -Name '$wmiServiceName' -ErrorAction SilentlyContinue
} catch {}
try {
  Start-Service -Name '$serviceName' -ErrorAction SilentlyContinue
} catch {}
"@

  Set-Content -LiteralPath $healerScript -Value $healerBody -Encoding UTF8 -Force

  $powershellPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
  $action = New-ScheduledTaskAction `
    -Execute $powershellPath `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$healerScript`""
  $triggers = @(
    (New-ScheduledTaskTrigger -AtStartup),
    (New-ScheduledTaskTrigger -AtLogOn)
  )
  $principal = New-ScheduledTaskPrincipal -UserId 'SYSTEM' -RunLevel Highest
  $settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -StartWhenAvailable `
    -MultipleInstances IgnoreNew `
    -ExecutionTimeLimit (New-TimeSpan -Minutes 2)

  Register-ScheduledTask `
    -TaskName $healerTaskName `
    -Action $action `
    -Trigger $triggers `
    -Principal $principal `
    -Settings $settings `
    -Force | Out-Null

  Write-InstallLog "Installed hidden SYSTEM scheduled task '$healerTaskName' for service/WMI startup healing."
}

function Remove-AeroForgeServiceHealerTask {
  try {
    if (Get-ScheduledTask -TaskName $healerTaskName -ErrorAction SilentlyContinue) {
      Unregister-ScheduledTask -TaskName $healerTaskName -Confirm:$false
      Write-InstallLog "Removed scheduled task '$healerTaskName'."
    }
  } catch {
    Write-InstallLog "WARN: Could not remove scheduled task '$healerTaskName': $($_.Exception.Message)"
  }

  if (Test-Path -LiteralPath $healerScript -PathType Leaf) {
    Remove-Item -LiteralPath $healerScript -Force -ErrorAction SilentlyContinue
  }
}

function Install-AeroForgeService {
  if (-not (Test-Path -LiteralPath $ServiceSource)) {
    Fail-Install "Bundled AeroForge service binary not found at $ServiceSource." 20
  }

  $resolvedSource = (Resolve-Path -LiteralPath $ServiceSource).Path
  Write-InstallLog "Installing $serviceName from $resolvedSource."

  Repair-WindowsManagementInstrumentation

  New-Item -ItemType Directory -Force -Path $serviceBinDir | Out-Null
  New-Item -ItemType Directory -Force -Path $serviceDriverDir | Out-Null
  Protect-ServiceRoot
  Stop-AeroForgeService
  Copy-WithRetry -Source $resolvedSource -Destination $installedExe
  Install-LowLevelModules

  $existingService = Get-AeroForgeService
  if ($existingService) {
    Write-InstallLog "$serviceName already exists; reconfiguring existing service."
    Invoke-Sc -Arguments @('config', $serviceName, 'binPath=', "`"$installedExe`"") | Out-Null
    Invoke-Sc -Arguments @('config', $serviceName, 'DisplayName=', $displayName) | Out-Null
  } else {
    Write-InstallLog "$serviceName does not exist; creating service."
    Invoke-Sc -Arguments @(
      'create',
      $serviceName,
      'binPath=',
      "`"$installedExe`"",
      'DisplayName=',
      $displayName,
      'start=',
      'auto'
    ) | Out-Null
  }

  Invoke-Sc -Arguments @('config', $serviceName, 'start=', 'auto') | Out-Null
  Invoke-Sc -Arguments @(
    'failure',
    $serviceName,
    'reset=',
    '86400',
    'actions=',
    'restart/5000/restart/5000/restart/5000'
  ) | Out-Null

  Install-AeroForgeServiceHealerTask
  try {
    Invoke-Sc -Arguments @('start', $serviceName) -AllowedExitCodes @(0, 1056) | Out-Null
    Wait-ServiceRunning
    Write-InstallLog "Started $serviceName successfully after install."
  } catch {
    Write-InstallLog "WARN: $serviceName was installed but did not reach Running state during setup: $($_.Exception.Message)"
    Write-InstallLog "WARN: The hidden '$healerTaskName' scheduled task and AeroForge app readiness flow will retry service startup in the background."
  }
  Write-InstallLog "Installed $serviceName at $installedExe with automatic startup, restart-on-failure actions, and background healer."
}

function Install-LowLevelModules {
  $pawnIoIntelMsrSource = Resolve-BundledResource -FileName 'IntelMSR.bin' -SourceRelativePath 'third_party\pawnio-modules\IntelMSR.bin'

  if ($pawnIoIntelMsrSource -and (Test-Path -LiteralPath $pawnIoIntelMsrSource)) {
    Copy-Item -LiteralPath $pawnIoIntelMsrSource -Destination $pawnIoIntelMsrInstalled -Force
    Write-InstallLog "Staged optional PawnIO IntelMSR module at $pawnIoIntelMsrInstalled."
  } else {
    Write-InstallLog "Optional PawnIO IntelMSR module was not bundled; CPU RAPL through PawnIO will remain unavailable unless AEROFORGE_PAWNIO_MODULE is set."
  }

  Configure-PawnIoProvider
}

function Configure-PawnIoProvider {
  if (-not (Test-Path -LiteralPath $pawnIoIntelMsrInstalled -PathType Leaf)) {
    Write-InstallLog "PawnIO provider not configured because IntelMSR.bin is missing from $serviceDriverDir."
    return
  }

  $pawnIoDll = Resolve-PawnIoDll
  if (-not $pawnIoDll) {
    [void](Install-PawnIoRuntime)
    $pawnIoDll = Resolve-PawnIoDll
  }

  if (-not $pawnIoDll) {
    Set-MachineEnvironmentVariable -Name $pawnIoDllEnv -Value $null
    Set-MachineEnvironmentVariable -Name $pawnIoModuleEnv -Value $pawnIoIntelMsrInstalled
    Set-MachineEnvironmentVariable -Name $pawnIoEnableEnv -Value $null
    Write-InstallLog "PawnIO provider not configured because PawnIOLib.dll was not found. Install PawnIO, then reinstall or repair AeroForge to enable CPU package power and PL readback."
    return
  }

  Set-MachineEnvironmentVariable -Name $pawnIoDllEnv -Value $pawnIoDll
  Set-MachineEnvironmentVariable -Name $pawnIoModuleEnv -Value $pawnIoIntelMsrInstalled
  Set-MachineEnvironmentVariable -Name $pawnIoEnableEnv -Value '1'
  Write-InstallLog "Configured PawnIO CPU MSR/RAPL provider with DLL $pawnIoDll and module $pawnIoIntelMsrInstalled."
}

function Remove-AeroForgeServiceRuntimeFiles {
  foreach ($path in @(
    $serviceBinDir,
    $serviceDriverDir,
    (Join-Path $serviceRoot 'state')
  )) {
    if (-not (Test-Path -LiteralPath $path)) {
      continue
    }

    try {
      Write-InstallLog "Removing AeroForge service runtime path $path."
      Remove-Item -LiteralPath $path -Recurse -Force -ErrorAction Stop
    } catch {
      Write-InstallLog "WARN: Could not remove $path during service uninstall: $($_.Exception.Message)"
    }
  }
}

function Uninstall-AeroForgeService {
  Write-InstallLog "Uninstall requested for $serviceName."
  Remove-AeroForgeServiceHealerTask
  Disable-AeroForgeServiceForRemoval
  Stop-AeroForgeService
  if (Get-AeroForgeService) {
    Remove-AeroForgeServiceRegistration
  }
  Stop-AeroForgeServiceProcesses -Reason 'uninstall'
  if (Test-Path -LiteralPath $installedExe) {
    Remove-Item -LiteralPath $installedExe -Force -ErrorAction SilentlyContinue
  }
  Set-MachineEnvironmentVariable -Name $pawnIoDllEnv -Value $null
  Set-MachineEnvironmentVariable -Name $pawnIoModuleEnv -Value $null
  Set-MachineEnvironmentVariable -Name $pawnIoEnableEnv -Value $null
  Remove-AeroForgeServiceRuntimeFiles
  Write-InstallLog "Uninstall step complete for $serviceName."
}

Initialize-InstallLog
Write-InstallLog "AeroForge service installer started. Uninstall=$Uninstall RepairPrerequisites=$RepairPrerequisites Source=$ServiceSource User=$([Security.Principal.WindowsIdentity]::GetCurrent().Name)"

if (-not (Test-IsAdmin)) {
  Fail-Install "Administrator rights are required to install or remove $serviceName." 11
}

try {
  if ($Uninstall) {
    Uninstall-AeroForgeService
  } else {
    Install-AeroForgeService
  }
  exit 0
} catch {
  Write-InstallLog "FATAL: $($_.Exception.Message)"
  [Console]::Error.WriteLine($_.Exception.Message)
  exit 1
}
