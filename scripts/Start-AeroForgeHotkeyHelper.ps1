param(
  [Parameter(Mandatory = $true)]
  [string]$HelperPath
)

$ErrorActionPreference = 'Stop'

$helperPath = [Environment]::ExpandEnvironmentVariables($HelperPath)
$deadline = (Get-Date).AddMinutes(2)

while ((Get-Date) -lt $deadline) {
  if (Test-Path -LiteralPath $helperPath) {
    $existing = Get-CimInstance Win32_Process -Filter "Name='aeroforge-hotkey-helper.exe'" -ErrorAction SilentlyContinue |
      Where-Object { $_.CommandLine -like "*$helperPath*" }
    if (-not $existing) {
      Start-Process -FilePath $helperPath -ArgumentList '--daemon' -WorkingDirectory (Split-Path -Parent $helperPath) -WindowStyle Hidden
    }
    exit 0
  }

  Start-Sleep -Seconds 5
}

exit 1
