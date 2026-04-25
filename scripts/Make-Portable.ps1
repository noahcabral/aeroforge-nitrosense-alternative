$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$version = $package.version

$releaseDir = Join-Path $projectRoot 'src-tauri\target\release'
$releaseExe = Join-Path $releaseDir 'aeroforge-control.exe'
if (-not (Test-Path -LiteralPath $releaseExe)) {
  throw "Release executable not found at $releaseExe. Run 'npm.cmd run tauri:build' first."
}

$portableRoot = Join-Path $projectRoot 'portable'
$portableDir = Join-Path $portableRoot 'AeroForge Control Portable'
$portableZip = Join-Path $portableRoot "AeroForge-Control-Portable-$version.zip"

New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null

if (Test-Path -LiteralPath $portableDir) {
  Remove-Item -LiteralPath $portableDir -Recurse -Force
}

New-Item -ItemType Directory -Force -Path $portableDir | Out-Null
Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $portableDir 'aeroforge-control.exe') -Force

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
- Runtime DLLs from the Tauri release folder are included alongside the executable.
- WebView2 must be present on the machine. It is already installed on most modern Windows systems.
- If a fresh machine is missing Visual C++ runtime components and the app does not launch, use the MSI or NSIS installer builds instead.

Installer builds:
- MSI: src-tauri\target\release\bundle\msi
- NSIS: src-tauri\target\release\bundle\nsis
"@

Set-Content -LiteralPath (Join-Path $portableDir 'README-PORTABLE.txt') -Value $readme

if (Test-Path -LiteralPath $portableZip) {
  Remove-Item -LiteralPath $portableZip -Force
}

Compress-Archive -Path (Join-Path $portableDir '*') -DestinationPath $portableZip -Force

Write-Output "Portable folder: $portableDir"
Write-Output "Portable zip: $portableZip"
