$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$version = $package.version

$releaseDir = Join-Path $projectRoot 'src-tauri\target\release'
$releaseExe = Join-Path $releaseDir 'aeroforge-control.exe'
$hotkeyHelperExe = Join-Path $releaseDir 'aeroforge-hotkey-helper.exe'
$installerExe = Join-Path $releaseDir "bundle\nsis\AeroForge Control_${version}_x64-setup.exe"
$debugCollectorCmd = Join-Path $projectRoot 'scripts\AeroForge-Debug-Collector.cmd'
if (-not (Test-Path -LiteralPath $releaseExe)) {
  throw "Release executable not found at $releaseExe. Run 'npm.cmd run tauri:build' first."
}
if (-not (Test-Path -LiteralPath $hotkeyHelperExe)) {
  throw "Hotkey helper executable not found at $hotkeyHelperExe. Run 'cargo build --release --manifest-path src-tauri\Cargo.toml --bin aeroforge-hotkey-helper' first."
}
if (-not (Test-Path -LiteralPath $debugCollectorCmd)) {
  throw "Debug collector not found at $debugCollectorCmd."
}
if (-not (Test-Path -LiteralPath $installerExe)) {
  throw "Installer executable not found at $installerExe. Run 'npm.cmd run tauri:build' first."
}

$portableRoot = Join-Path $projectRoot 'portable'
$portableDir = Join-Path $portableRoot 'AeroForge Control Portable'
$portableZip = Join-Path $portableRoot "AeroForge-Control-Portable-$version.zip"
$installerCopy = Join-Path $portableRoot "AeroForge-Control-Setup-$version.exe"

New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null

if (Test-Path -LiteralPath $portableDir) {
  foreach ($taskName in @('AeroForgeHotkeyHelper', 'AeroForgePrewarm')) {
    if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) {
      Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    }
  }
  Get-Process aeroforge-hotkey-helper -ErrorAction SilentlyContinue | Stop-Process -Force
  try {
    Remove-Item -LiteralPath $portableDir -Recurse -Force
  } catch {
    Write-Warning "Could not remove portable root; reusing it after clearing contents. $($_.Exception.Message)"
    Get-ChildItem -LiteralPath $portableDir -Force | Remove-Item -Recurse -Force
  }
}

New-Item -ItemType Directory -Force -Path $portableDir | Out-Null
Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $portableDir 'aeroforge-control.exe') -Force
Copy-Item -LiteralPath $hotkeyHelperExe -Destination (Join-Path $portableDir 'aeroforge-hotkey-helper.exe') -Force
Copy-Item -LiteralPath $debugCollectorCmd -Destination (Join-Path $portableDir 'AeroForge-Debug-Collector.cmd') -Force

$runtimeDlls = Get-ChildItem -LiteralPath $releaseDir -File -Filter '*.dll'
foreach ($runtimeDll in $runtimeDlls) {
  Copy-Item -LiteralPath $runtimeDll.FullName -Destination (Join-Path $portableDir $runtimeDll.Name) -Force
}

$readme = @"
AeroForge Control Portable
Version: $version

How to run:
- Double-click aeroforge-control.exe

Notes:
- This is a portable build of the Tauri desktop app.
- aeroforge-hotkey-helper.exe is included beside the app so the Nitro keyboard key can open or focus AeroForge from the logged-in Windows session.
- The hotkey helper stays resident at logon with --daemon for Nitro key activation without keeping the WebView UI running in the background.
- Running aeroforge-hotkey-helper.exe without --daemon is a one-shot AeroForge open/focus trigger.
- AeroForge-Debug-Collector.cmd is included for support bundles when a machine has install, telemetry, fan, battery, power, or Nitro key issues.
- To start the helper automatically at logon, run scripts\Install-AeroForgeStartup.ps1 from the source tree after building the portable folder.
- Runtime DLLs from the Tauri release folder are included alongside the executable.
- WebView2 must be present on the machine. It is already installed on most modern Windows systems.
- For a first install on a fresh machine, use the Setup EXE so AeroForgeService is installed.
- Release builds do not bundle or auto-load WinRing0. CPU MSR/RAPL diagnostics require an explicit external driver opt-in.
- NVIDIA temperature/utilization/VRAM monitoring is gated by Windows dedicated-GPU activity. NVIDIA power readback follows Settings > NVIDIA Telemetry Polling; AEROFORGE_ENABLE_NVIDIA_TELEMETRY=1 can still force diagnostics.

Installer builds:
- NSIS: src-tauri\target\release\bundle\nsis
"@

Set-Content -LiteralPath (Join-Path $portableDir 'README-PORTABLE.txt') -Value $readme

if (Test-Path -LiteralPath $portableZip) {
  Remove-Item -LiteralPath $portableZip -Force
}

Compress-Archive -Path (Join-Path $portableDir '*') -DestinationPath $portableZip -Force
Copy-Item -LiteralPath $installerExe -Destination $installerCopy -Force

Write-Output "Portable folder: $portableDir"
Write-Output "Portable zip: $portableZip"
Write-Output "Installer copy: $installerCopy"
