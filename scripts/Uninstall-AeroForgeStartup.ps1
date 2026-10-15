$ErrorActionPreference = 'Stop'

$taskNames = @('AeroForgeHotkeyHelper', 'AeroForgePrewarm')

foreach ($taskName in $taskNames) {
  if (Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) {
    Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
    Write-Output "Removed scheduled task $taskName."
  } else {
    Write-Output "Scheduled task $taskName is not installed."
  }
}

Remove-ItemProperty `
  -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
  -Name 'AeroForgeHotkeyHelper' `
  -ErrorAction SilentlyContinue

Get-CimInstance Win32_Process -Filter "Name='aeroforge-hotkey-helper.exe'" -ErrorAction SilentlyContinue |
  ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }

# Install-AeroForgeStartup.ps1 points NitroSense's launcher at the helper and keeps
# the original next to it; put it back so the Nitro key opens NitroSense again.
$launcherTarget = 'C:\Program Files\NitroSense\Prerequisites\LauncherTarget.txt'
$backupTarget = 'C:\Program Files\NitroSense\Prerequisites\LauncherTarget.aeroforge-backup.txt'
if (Test-Path -LiteralPath $backupTarget) {
  Move-Item -LiteralPath $backupTarget -Destination $launcherTarget -Force
  Write-Output "Restored NitroSense launcher target."
}
