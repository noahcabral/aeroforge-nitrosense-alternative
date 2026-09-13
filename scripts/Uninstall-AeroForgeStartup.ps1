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
