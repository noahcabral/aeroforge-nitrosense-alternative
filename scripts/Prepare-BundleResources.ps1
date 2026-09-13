$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$serviceManifest = Join-Path $projectRoot 'aeroforge-service\Cargo.toml'
$tauriManifest = Join-Path $projectRoot 'src-tauri\Cargo.toml'
$serviceExe = Join-Path $projectRoot 'aeroforge-service\target\release\aeroforge-service.exe'
$helperExe = Join-Path $projectRoot 'src-tauri\target\release\aeroforge-hotkey-helper.exe'
$installerServiceScript = Join-Path $projectRoot 'src-tauri\resources\Install-AeroForgeBundledService.ps1'

function Resolve-CargoPath {
  # Explicit override first, then PATH, then the standard rustup install locations.
  if ($env:AEROFORGE_CARGO -and (Test-Path -LiteralPath $env:AEROFORGE_CARGO)) {
    return $env:AEROFORGE_CARGO
  }

  $cargoCommand = Get-Command cargo.exe -ErrorAction SilentlyContinue
  if ($cargoCommand) {
    return $cargoCommand.Source
  }

  $fallbacks = @()
  if ($env:CARGO_HOME) {
    $fallbacks += (Join-Path $env:CARGO_HOME 'bin\cargo.exe')
  }
  if ($env:USERPROFILE) {
    $fallbacks += (Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe')
  }

  foreach ($candidate in $fallbacks) {
    if (Test-Path -LiteralPath $candidate) {
      return $candidate
    }
  }

  throw 'Unable to locate cargo.exe. Install the Rust toolchain from https://rustup.rs or set AEROFORGE_CARGO to the full path of cargo.exe.'
}

$cargoPath = Resolve-CargoPath

& $cargoPath build --release --manifest-path $serviceManifest
if ($LASTEXITCODE -ne 0) {
  throw 'Failed to build aeroforge-service.exe.'
}

$previousTauriConfig = $env:TAURI_CONFIG
try {
  $env:TAURI_CONFIG = '{"bundle":{"resources":null}}'
  & $cargoPath build --release --manifest-path $tauriManifest --bin aeroforge-hotkey-helper
  if ($LASTEXITCODE -ne 0) {
    throw 'Failed to build aeroforge-hotkey-helper.exe.'
  }
}
finally {
  if ($null -eq $previousTauriConfig) {
    Remove-Item Env:\TAURI_CONFIG -ErrorAction SilentlyContinue
  } else {
    $env:TAURI_CONFIG = $previousTauriConfig
  }
}

foreach ($requiredPath in @($serviceExe, $helperExe, $installerServiceScript)) {
  if (-not (Test-Path -LiteralPath $requiredPath)) {
    throw "Required bundle resource missing: $requiredPath"
  }
}

Write-Output "Prepared bundle resources:"
Write-Output "  $serviceExe"
Write-Output "  $helperExe"
Write-Output "  $installerServiceScript"
