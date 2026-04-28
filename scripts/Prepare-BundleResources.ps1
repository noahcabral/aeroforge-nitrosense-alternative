$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$serviceManifest = Join-Path $projectRoot 'aeroforge-service\Cargo.toml'
$tauriManifest = Join-Path $projectRoot 'src-tauri\Cargo.toml'
$serviceExe = Join-Path $projectRoot 'aeroforge-service\target\release\aeroforge-service.exe'
$helperExe = Join-Path $projectRoot 'src-tauri\target\release\aeroforge-hotkey-helper.exe'
$installerServiceScript = Join-Path $projectRoot 'src-tauri\resources\Install-AeroForgeBundledService.ps1'

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

$cargoPath = Resolve-CargoPath

& $cargoPath build --release --manifest-path $serviceManifest
if ($LASTEXITCODE -ne 0) {
  throw 'Failed to build aeroforge-service.exe.'
}

& $cargoPath build --release --manifest-path $tauriManifest --bin aeroforge-hotkey-helper
if ($LASTEXITCODE -ne 0) {
  throw 'Failed to build aeroforge-hotkey-helper.exe.'
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
