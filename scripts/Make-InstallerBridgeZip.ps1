$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$version = $package.version
$tauriManifest = Join-Path $projectRoot 'src-tauri\Cargo.toml'
$releaseDir = Join-Path $projectRoot 'src-tauri\target\release'
$bridgeExe = Join-Path $releaseDir 'aeroforge-update-bridge.exe'
$setupExe = Join-Path $releaseDir "bundle\nsis\AeroForge Control_$version`_x64-setup.exe"

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

if (-not (Test-Path -LiteralPath $setupExe)) {
  throw "Setup executable not found at $setupExe. Run the Tauri bundle build first."
}

$cargoPath = Resolve-CargoPath
& $cargoPath build --release --manifest-path $tauriManifest --bin aeroforge-update-bridge
if ($LASTEXITCODE -ne 0) {
  throw 'Failed to build aeroforge-update-bridge.exe.'
}

if (-not (Test-Path -LiteralPath $bridgeExe)) {
  throw "Bridge executable not found at $bridgeExe."
}

$portableRoot = Join-Path $projectRoot 'portable'
$bridgeRoot = Join-Path $portableRoot 'AeroForge Installer Bridge'
$bridgeZip = Join-Path $portableRoot "AeroForge-Control-Portable-$version.zip"

New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null

if (Test-Path -LiteralPath $bridgeRoot) {
  Remove-Item -LiteralPath $bridgeRoot -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $bridgeRoot | Out-Null

Copy-Item -LiteralPath $bridgeExe -Destination (Join-Path $bridgeRoot 'aeroforge-control.exe') -Force
Copy-Item -LiteralPath $setupExe -Destination (Join-Path $bridgeRoot "AeroForge-Control-Setup-$version.exe") -Force

$readme = @"
AeroForge Control Update Bridge
Version: $version

This ZIP keeps old AeroForge portable updaters compatible with the newer Setup EXE installer.

How it works:
- Older AeroForge builds only know how to stage ZIP assets named AeroForge-Control-Portable-*.zip.
- This ZIP provides that expected asset name.
- Its aeroforge-control.exe is a small bridge that launches AeroForge-Control-Setup-$version.exe.
- After AeroForge is installed, the same bridge forwards to the installed AeroForge app instead of rerunning setup.

For manual installs, run AeroForge-Control-Setup-$version.exe directly.
"@

Set-Content -LiteralPath (Join-Path $bridgeRoot 'README-UPDATE-BRIDGE.txt') -Value $readme

if (Test-Path -LiteralPath $bridgeZip) {
  Remove-Item -LiteralPath $bridgeZip -Force
}

Compress-Archive -Path (Join-Path $bridgeRoot '*') -DestinationPath $bridgeZip -Force

Write-Output "Bridge folder: $bridgeRoot"
Write-Output "Bridge zip: $bridgeZip"
