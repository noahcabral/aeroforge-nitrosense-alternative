$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$portableHelper = Join-Path $projectRoot 'portable\AeroForge Control Portable\aeroforge-hotkey-helper.exe'
$releaseHelper = Join-Path $projectRoot 'src-tauri\target\release\aeroforge-hotkey-helper.exe'
$launcherScript = Join-Path $projectRoot 'scripts\Start-AeroForgeHotkeyHelper.ps1'
$taskName = 'AeroForgeHotkeyHelper'
$legacyTaskName = 'AeroForgePrewarm'

if (Test-Path -LiteralPath $portableHelper) {
  $helperPath = (Resolve-Path -LiteralPath $portableHelper).Path
} elseif (Test-Path -LiteralPath $releaseHelper) {
  $helperPath = (Resolve-Path -LiteralPath $releaseHelper).Path
} else {
  throw "Unable to find aeroforge-hotkey-helper.exe. Build the portable app first."
}

$identity = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name
$launcherPath = (Resolve-Path -LiteralPath $launcherScript).Path
$taskCommand = 'powershell.exe'
$taskArguments = "-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$launcherPath`" -HelperPath `"$helperPath`""
$action = New-ScheduledTaskAction -Execute $taskCommand -Argument $taskArguments -WorkingDirectory $projectRoot
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $identity
$principal = New-ScheduledTaskPrincipal -UserId $identity -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet `
  -AllowStartIfOnBatteries `
  -DontStopIfGoingOnBatteries `
  -ExecutionTimeLimit (New-TimeSpan -Hours 0) `
  -RestartCount 3 `
  -RestartInterval (New-TimeSpan -Minutes 1) `
  -StartWhenAvailable

if (Get-ScheduledTask -TaskName $legacyTaskName -ErrorAction SilentlyContinue) {
  Unregister-ScheduledTask -TaskName $legacyTaskName -Confirm:$false
}

Register-ScheduledTask `
  -TaskName $taskName `
  -Action $action `
  -Trigger $trigger `
  -Principal $principal `
  -Settings $settings `
  -Description 'Starts AeroForge hotkey helper at logon so Nitro key activation is ready without keeping the WebView UI resident.' `
  -Force | Out-Null

$runCommand = "$taskCommand -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$launcherPath`" -HelperPath `"$helperPath`""
Set-ItemProperty `
  -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' `
  -Name 'AeroForgeHotkeyHelper' `
  -Value $runCommand

$launcherTarget = 'C:\Program Files\NitroSense\Prerequisites\LauncherTarget.txt'
if (Test-Path -LiteralPath $launcherTarget) {
  $backupTarget = 'C:\Program Files\NitroSense\Prerequisites\LauncherTarget.aeroforge-backup.txt'
  if (-not (Test-Path -LiteralPath $backupTarget)) {
    Copy-Item -LiteralPath $launcherTarget -Destination $backupTarget -Force
  }
  Set-Content -LiteralPath $launcherTarget -Value $helperPath -Encoding ASCII
}

Write-Output "Installed scheduled task $taskName for $identity -> $taskCommand $taskArguments"
