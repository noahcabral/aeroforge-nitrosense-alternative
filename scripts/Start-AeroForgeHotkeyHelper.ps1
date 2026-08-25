param(
  [Parameter(Mandatory = $true)]
  [string]$HelperPath
)

$ErrorActionPreference = 'Stop'

$helperPath = [Environment]::ExpandEnvironmentVariables($HelperPath)
$deadline = (Get-Date).AddMinutes(2)

while ((Get-Date) -lt $deadline) {
  if (Test-Path -LiteralPath $helperPath) {
    $resolvedHelperPath = (Resolve-Path -LiteralPath $helperPath).Path
    $helperProcesses = Get-CimInstance Win32_Process -Filter "Name='aeroforge-hotkey-helper.exe'" -ErrorAction SilentlyContinue
    $helperProcesses |
      Where-Object { -not $_.ExecutablePath -or ($_.ExecutablePath -ine $resolvedHelperPath) } |
      ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

    $existing = $helperProcesses |
      Where-Object { $_.ExecutablePath -ieq $resolvedHelperPath }
    if (-not $existing) {
      Start-Process -FilePath $resolvedHelperPath -ArgumentList '--daemon' -WorkingDirectory (Split-Path -Parent $resolvedHelperPath) -WindowStyle Hidden
    }
    exit 0
  }

  Start-Sleep -Seconds 5
}

exit 1
