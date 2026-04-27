$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$serviceManifest = Join-Path $projectRoot 'aeroforge-service\Cargo.toml'
$buildExe = Join-Path $projectRoot 'aeroforge-service\target\release\aeroforge-service.exe'
$serviceName = 'AeroForgeService'
$displayName = 'AeroForge Service'
$serviceRoot = Join-Path $env:ProgramData 'AeroForge\Service'
$serviceBinDir = Join-Path $serviceRoot 'bin'
$installedExe = Join-Path $serviceBinDir 'aeroforge-service.exe'

function Resolve-CargoPath {
  $fallbacks = @(
    'C:\Users\noah\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe',
    'C:\Users\noah\.cargo\bin\cargo.exe'
  )

  foreach ($candidate in $fallbacks) {
    if (Test-Path -LiteralPath $candidate) {
      return $candidate
    }
  }

  $cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
  if ($cargoCommand) {
    return $cargoCommand.Source
  }

  throw 'Unable to locate cargo.exe. Install or repair the Rust toolchain path first.'
}

function Invoke-Sc {
  param(
    [Parameter(Mandatory = $true)]
    [string[]]$Arguments
  )

  & sc.exe @Arguments | Out-Null
  if ($LASTEXITCODE -ne 0) {
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

$cargoPath = Resolve-CargoPath

& $cargoPath build --release --manifest-path $serviceManifest
if ($LASTEXITCODE -ne 0) {
  throw 'Failed to build aeroforge-service.exe.'
}

New-Item -ItemType Directory -Force -Path $serviceBinDir | Out-Null

$existingService = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
if ($existingService) {
  if ($existingService.Status -ne 'Stopped') {
    Stop-Service -Name $serviceName -Force
    $existingService.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(15))
  }
}

Copy-WithRetry -Source $buildExe -Destination $installedExe

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
(Get-Service -Name $serviceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(15))

Write-Output "Installed $serviceName at $installedExe with delayed automatic startup and restart-on-failure actions."
