$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$serviceManifest = Join-Path $projectRoot 'aeroforge-service\Cargo.toml'
$serviceExe = Join-Path $projectRoot 'aeroforge-service\target\release\aeroforge-service.exe'
$serviceName = 'AeroForgeService'
$displayName = 'AeroForge Service'

if (-not (Test-Path -LiteralPath $serviceExe)) {
  cargo build --release --manifest-path $serviceManifest
}

if (Get-Service -Name $serviceName -ErrorAction SilentlyContinue) {
  Write-Output "$serviceName already exists."
  exit 0
}

New-Service -Name $serviceName -BinaryPathName "`"$serviceExe`"" -DisplayName $displayName -StartupType Automatic
sc.exe config $serviceName start= delayed-auto | Out-Null
if ($LASTEXITCODE -ne 0) {
  throw "Failed to configure $serviceName for delayed automatic startup."
}

Write-Output "Installed $serviceName with delayed automatic startup"
