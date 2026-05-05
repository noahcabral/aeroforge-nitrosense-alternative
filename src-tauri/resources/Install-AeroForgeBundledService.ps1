param(
  [switch]$Uninstall,
  [string]$ServiceSource = (Join-Path $PSScriptRoot 'aeroforge-service.exe')
)

$ErrorActionPreference = 'Stop'

$serviceName = 'AeroForgeService'
$displayName = 'AeroForge Service'
$serviceRoot = Join-Path $env:ProgramData 'AeroForge\Service'
$serviceBinDir = Join-Path $serviceRoot 'bin'
$serviceLogDir = Join-Path $serviceRoot 'logs'
$installedExe = Join-Path $serviceBinDir 'aeroforge-service.exe'
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
      Start-Sleep -Milliseconds 500
    }
  } while ($true)
}

function Get-AeroForgeService {
  Get-Service -Name $serviceName -ErrorAction SilentlyContinue
}

function Wait-ServiceDeleted {
  $deadline = (Get-Date).AddSeconds(25)
  do {
    if (-not (Get-AeroForgeService)) {
      return
    }
    Start-Sleep -Milliseconds 500
  } while ((Get-Date) -lt $deadline)

  throw "$serviceName is still present after delete wait. A reboot may be required before reinstalling."
}

function Stop-AeroForgeService {
  $service = Get-AeroForgeService
  if (-not $service) {
    return
  }

  if ($service.Status -ne 'Stopped') {
    Write-InstallLog "Stopping $serviceName."
    Invoke-Sc -Arguments @('stop', $serviceName) -AllowedExitCodes @(0, 1062) | Out-Null
    $deadline = (Get-Date).AddSeconds(25)
    do {
      $service = Get-AeroForgeService
      if (-not $service -or $service.Status -eq 'Stopped') {
        return
      }
      Start-Sleep -Milliseconds 500
    } while ((Get-Date) -lt $deadline)

    Write-InstallLog "$serviceName did not stop cleanly; terminating service process if still present."
    Get-Process aeroforge-service -ErrorAction SilentlyContinue |
      Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 750
  }
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

function Install-AeroForgeService {
  if (-not (Test-Path -LiteralPath $ServiceSource)) {
    Fail-Install "Bundled AeroForge service binary not found at $ServiceSource." 20
  }

  $resolvedSource = (Resolve-Path -LiteralPath $ServiceSource).Path
  Write-InstallLog "Installing $serviceName from $resolvedSource."

  New-Item -ItemType Directory -Force -Path $serviceBinDir | Out-Null
  Stop-AeroForgeService
  Get-Process aeroforge-service -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
  Copy-WithRetry -Source $resolvedSource -Destination $installedExe

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

  Invoke-Sc -Arguments @('config', $serviceName, 'start=', 'delayed-auto') | Out-Null
  Invoke-Sc -Arguments @(
    'failure',
    $serviceName,
    'reset=',
    '86400',
    'actions=',
    'restart/5000/restart/5000/restart/5000'
  ) | Out-Null

  Invoke-Sc -Arguments @('start', $serviceName) -AllowedExitCodes @(0, 1056) | Out-Null
  Wait-ServiceRunning
  Write-InstallLog "Installed $serviceName at $installedExe with delayed automatic startup and restart-on-failure actions."
}

function Uninstall-AeroForgeService {
  Write-InstallLog "Uninstall requested for $serviceName."
  Stop-AeroForgeService
  if (Get-AeroForgeService) {
    Invoke-Sc -Arguments @('delete', $serviceName) -AllowedExitCodes @(0, 1060) | Out-Null
    Wait-ServiceDeleted
  }
  Get-Process aeroforge-service -ErrorAction SilentlyContinue |
    Stop-Process -Force -ErrorAction SilentlyContinue
  if (Test-Path -LiteralPath $installedExe) {
    Remove-Item -LiteralPath $installedExe -Force -ErrorAction SilentlyContinue
  }
  Write-InstallLog "Uninstall step complete for $serviceName."
}

Initialize-InstallLog
Write-InstallLog "AeroForge service installer started. Uninstall=$Uninstall Source=$ServiceSource User=$([Security.Principal.WindowsIdentity]::GetCurrent().Name)"

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
