param(
  [switch]$Uninstall,
  [string]$ServiceSource = (Join-Path $PSScriptRoot 'aeroforge-service.exe')
)

$ErrorActionPreference = 'Stop'

$serviceName = 'AeroForgeService'
$displayName = 'AeroForge Service'
$serviceRoot = Join-Path $env:ProgramData 'AeroForge\Service'
$serviceBinDir = Join-Path $serviceRoot 'bin'
$installedExe = Join-Path $serviceBinDir 'aeroforge-service.exe'

function Invoke-Sc {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Arguments,
    [int[]]$AllowedExitCodes = @(0)
  )

  & sc.exe @Arguments | Out-Null
  if ($AllowedExitCodes -notcontains $LASTEXITCODE) {
    throw "sc.exe $($Arguments -join ' ') failed with exit code $LASTEXITCODE."
  }
}

function Copy-WithRetry {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Source,
    [Parameter(Mandatory = $true)]
    [string]$Destination
  )

  $deadline = (Get-Date).AddSeconds(15)
  do {
    try {
      Copy-Item -LiteralPath $Source -Destination $Destination -Force
      return
    } catch {
      if ((Get-Date) -ge $deadline) {
        throw
      }
      Start-Sleep -Milliseconds 500
    }
  } while ($true)
}

function Stop-AeroForgeService {
  $service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
  if ($service -and $service.Status -ne 'Stopped') {
    Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
    try {
      $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(20))
    } catch {
      Get-Process aeroforge-service -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
  }
}

function Install-AeroForgeService {
  if (-not (Test-Path -LiteralPath $ServiceSource)) {
    throw "Bundled AeroForge service binary not found at $ServiceSource."
  }

  New-Item -ItemType Directory -Force -Path $serviceBinDir | Out-Null
  Stop-AeroForgeService
  Get-Process aeroforge-service -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Copy-WithRetry -Source $ServiceSource -Destination $installedExe

  $existingService = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
  if ($existingService) {
    Invoke-Sc -Arguments @('config', $serviceName, 'binPath=', "`"$installedExe`"")
    Invoke-Sc -Arguments @('config', $serviceName, 'start=', 'delayed-auto')
  } else {
    New-Service `
      -Name $serviceName `
      -BinaryPathName "`"$installedExe`"" `
      -DisplayName $displayName `
      -StartupType Automatic
    Invoke-Sc -Arguments @('config', $serviceName, 'start=', 'delayed-auto')
  }

  Invoke-Sc -Arguments @(
    'failure',
    $serviceName,
    'reset=',
    '86400',
    'actions=',
    'restart/5000/restart/5000/restart/5000'
  )

  Start-Service -Name $serviceName
  (Get-Service -Name $serviceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(20))
  Write-Output "Installed $serviceName at $installedExe with delayed automatic startup and restart-on-failure actions."
}

function Uninstall-AeroForgeService {
  Stop-AeroForgeService
  if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
    Invoke-Sc -Arguments @('delete', $serviceName) -AllowedExitCodes @(0, 1060)
  }
  Get-Process aeroforge-service -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  if (Test-Path -LiteralPath $installedExe) {
    Remove-Item -LiteralPath $installedExe -Force -ErrorAction SilentlyContinue
  }
}

if ($Uninstall) {
  Uninstall-AeroForgeService
} else {
  Install-AeroForgeService
}
