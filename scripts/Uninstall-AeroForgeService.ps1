$ErrorActionPreference = 'Stop'

$serviceName = 'AeroForgeService'
$service = Get-Service -Name $serviceName -ErrorAction SilentlyContinue

if (-not $service) {
  Write-Output "$serviceName is not installed."
  exit 0
}

if ($service.Status -ne 'Stopped') {
  Stop-Service -Name $serviceName -Force
}

sc.exe delete $serviceName | Out-Null
Write-Output "Uninstalled $serviceName"
