$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$version = $package.version

$source = Join-Path $projectRoot 'scripts\AeroForge-Debug-Collector.cmd'
$portableRoot = Join-Path $projectRoot 'portable'
$debugRoot = Join-Path $portableRoot 'AeroForge Debug Collector'
$debugCmdName = "AeroForge-Debug-Collector-$version.cmd"
$debugZip = Join-Path $portableRoot "AeroForge-Debug-Collector-$version.zip"

if (-not (Test-Path -LiteralPath $source)) {
  throw "Debug collector source was not found at $source"
}

New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null
if (Test-Path -LiteralPath $debugRoot) {
  Remove-Item -LiteralPath $debugRoot -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $debugRoot | Out-Null

Copy-Item -LiteralPath $source -Destination (Join-Path $debugRoot $debugCmdName) -Force

$readme = @"
AeroForge Debug Collector
Version: $version

How to use:
- Extract this ZIP.
- Double-click $debugCmdName.
- Accept the Windows admin prompt.
- Send the AeroForge-Debug-*.zip generated on the Desktop to AeroForge support.

Useful support modes:
- $debugCmdName -Quick -PollSeconds 60
- $debugCmdName -Deep -PollSeconds 120 -PollIntervalMs 1000
- $debugCmdName -Poll fans pipe performance -PollSeconds 45 -PollIntervalMs 500
- $debugCmdName -Poll nvidia gpu-counters processes -PollSeconds 90
- $debugCmdName -Deep -NoNvidiaSmi

Notes:
- The collector is read-only. It does not apply fan, power, battery, firmware, EFI, display, or registry changes.
- It redacts GitHub-token-like strings and common Authorization/password/secret fields from copied text.
- It is shipped as a standalone release asset on every AeroForge release so users do not need to install AeroForge before collecting diagnostics.
"@
Set-Content -LiteralPath (Join-Path $debugRoot 'README-Debug-Collector.txt') -Value $readme -Encoding UTF8

if (Test-Path -LiteralPath $debugZip) {
  Remove-Item -LiteralPath $debugZip -Force
}
Compress-Archive -Path (Join-Path $debugRoot '*') -DestinationPath $debugZip -Force

Write-Output "Debug collector folder: $debugRoot"
Write-Output "Debug collector zip: $debugZip"
