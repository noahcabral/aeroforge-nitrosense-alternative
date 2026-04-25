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
  $cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
  if ($cargoCommand) {
    return $cargoCommand.Source
  }

  $fallbacks = @(
    'C:\Users\noah\.cargo\bin\cargo.exe',
    'C:\Users\noah\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe'
  )

  foreach ($candidate in $fallbacks) {
    if (Test-Path -LiteralPath $candidate) {
      return $candidate
    }
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

Copy-Item -LiteralPath $buildExe -Destination $installedExe -Force

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
