Var NitroSenseDisplayName
Var NitroSenseUninstallString
Var AeroForgeInstallDirSafeToWipe
Var AeroForgeInstallPawnIO

Function CleanAeroForgeInstallDirForInstall
  IfFileExists "$INSTDIR\aeroforge-control.exe" 0 check_display_exe_install
    Goto clean_install_dir
  check_display_exe_install:
  IfFileExists "$INSTDIR\AeroForge Control.exe" 0 check_service_exe_install
    Goto clean_install_dir
  check_service_exe_install:
  IfFileExists "$INSTDIR\aeroforge-service.exe" 0 check_helper_exe_install
    Goto clean_install_dir
  check_helper_exe_install:
  IfFileExists "$INSTDIR\aeroforge-hotkey-helper.exe" 0 check_service_script_install
    Goto clean_install_dir
  check_service_script_install:
  IfFileExists "$INSTDIR\Install-AeroForgeBundledService.ps1" 0 install_dir_clean_done
    Goto clean_install_dir

  clean_install_dir:
    DetailPrint "Removing stale AeroForge install files from $INSTDIR..."
    RMDir /r /REBOOTOK "$INSTDIR"

  install_dir_clean_done:
FunctionEnd

Function StopAeroForgeRuntimeForInstall
  DetailPrint "Stopping existing AeroForge runtime processes..."
  InitPluginsDir
  FileOpen $9 "$PLUGINSDIR\StopAeroForgeRuntime.ps1" w
  FileWrite $9 "$$ErrorActionPreference = 'SilentlyContinue'$\r$\n"
  FileWrite $9 "foreach ($$taskName in @('AeroForgeHotkeyHelper', 'AeroForgePrewarm')) {$\r$\n"
  FileWrite $9 "  $$task = Get-ScheduledTask -TaskName $$taskName -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "  if ($$task) { Stop-ScheduledTask -TaskName $$taskName -ErrorAction SilentlyContinue }$\r$\n"
  FileWrite $9 "}$\r$\n"
  FileWrite $9 "$$svc = Get-Service -Name 'AeroForgeService' -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "if ($$svc) { Stop-Service -Name 'AeroForgeService' -Force -ErrorAction SilentlyContinue }$\r$\n"
  FileWrite $9 "for ($$i = 0; $$i -lt 30; $$i++) {$\r$\n"
  FileWrite $9 "  $$svc = Get-Service -Name 'AeroForgeService' -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "  $$procs = @(Get-Process aeroforge-control,aeroforge-hotkey-helper,aeroforge-update-bridge,aeroforge-service -ErrorAction SilentlyContinue)$\r$\n"
  FileWrite $9 "  if ((-not $$svc -or $$svc.Status -eq 'Stopped') -and $$procs.Count -eq 0) { break }$\r$\n"
  FileWrite $9 "  $$procs | Stop-Process -Force -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "  Start-Sleep -Milliseconds 500$\r$\n"
  FileWrite $9 "}$\r$\n"
  FileClose $9
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\StopAeroForgeRuntime.ps1"' $9
FunctionEnd

Function un.StopAeroForgeRuntimeForUninstall
  DetailPrint "Stopping AeroForge runtime processes..."
  InitPluginsDir
  FileOpen $9 "$PLUGINSDIR\StopAeroForgeRuntime.ps1" w
  FileWrite $9 "$$ErrorActionPreference = 'SilentlyContinue'$\r$\n"
  FileWrite $9 "foreach ($$taskName in @('AeroForgeHotkeyHelper', 'AeroForgePrewarm')) {$\r$\n"
  FileWrite $9 "  $$task = Get-ScheduledTask -TaskName $$taskName -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "  if ($$task) { Stop-ScheduledTask -TaskName $$taskName -ErrorAction SilentlyContinue }$\r$\n"
  FileWrite $9 "  if ($$task) { Unregister-ScheduledTask -TaskName $$taskName -Confirm:$$false -ErrorAction SilentlyContinue }$\r$\n"
  FileWrite $9 "}$\r$\n"
  FileWrite $9 "$$svc = Get-Service -Name 'AeroForgeService' -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "if ($$svc) { Stop-Service -Name 'AeroForgeService' -Force -ErrorAction SilentlyContinue }$\r$\n"
  FileWrite $9 "for ($$i = 0; $$i -lt 30; $$i++) {$\r$\n"
  FileWrite $9 "  $$svc = Get-Service -Name 'AeroForgeService' -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "  $$procs = @(Get-Process aeroforge-control,aeroforge-hotkey-helper,aeroforge-update-bridge,aeroforge-service -ErrorAction SilentlyContinue)$\r$\n"
  FileWrite $9 "  if ((-not $$svc -or $$svc.Status -eq 'Stopped') -and $$procs.Count -eq 0) { break }$\r$\n"
  FileWrite $9 "  $$procs | Stop-Process -Force -ErrorAction SilentlyContinue$\r$\n"
  FileWrite $9 "  Start-Sleep -Milliseconds 500$\r$\n"
  FileWrite $9 "}$\r$\n"
  FileClose $9
  ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "$PLUGINSDIR\StopAeroForgeRuntime.ps1"' $9
FunctionEnd

Function FindNitroSenseInCurrentRoot
  StrCpy $0 0

  nitro_loop:
    EnumRegKey $1 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" nitro_done
    IntOp $0 $0 + 1

    ReadRegStr $2 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    StrCmp $2 "NitroSense" nitro_match
    StrCmp $2 "Nitro Sense" nitro_match
    StrCmp $2 "NitroSense Config" nitro_match
    Goto nitro_loop

  nitro_match:
    ReadRegStr $3 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "QuietUninstallString"
    StrCmp $3 "" 0 nitro_store
    ReadRegStr $3 HKLM "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    StrCmp $3 "" nitro_loop nitro_store

  nitro_store:
    StrCpy $NitroSenseDisplayName $2
    StrCpy $NitroSenseUninstallString $3

  nitro_done:
FunctionEnd

Function FindNitroSenseInCurrentUser
  StrCpy $0 0

  nitro_user_loop:
    EnumRegKey $1 HKCU "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall" $0
    StrCmp $1 "" nitro_user_done
    IntOp $0 $0 + 1

    ReadRegStr $2 HKCU "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "DisplayName"
    StrCmp $2 "NitroSense" nitro_user_match
    StrCmp $2 "Nitro Sense" nitro_user_match
    StrCmp $2 "NitroSense Config" nitro_user_match
    Goto nitro_user_loop

  nitro_user_match:
    ReadRegStr $3 HKCU "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "QuietUninstallString"
    StrCmp $3 "" 0 nitro_user_store
    ReadRegStr $3 HKCU "SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$1" "UninstallString"
    StrCmp $3 "" nitro_user_loop nitro_user_store

  nitro_user_store:
    StrCpy $NitroSenseDisplayName $2
    StrCpy $NitroSenseUninstallString $3

  nitro_user_done:
FunctionEnd

Function DetectNitroSense
  StrCpy $NitroSenseDisplayName ""
  StrCpy $NitroSenseUninstallString ""

  SetRegView 64
  Call FindNitroSenseInCurrentRoot
  StrCmp $NitroSenseUninstallString "" 0 nitro_found

  SetRegView 32
  Call FindNitroSenseInCurrentRoot
  StrCmp $NitroSenseUninstallString "" 0 nitro_found

  SetRegView 64
  Call FindNitroSenseInCurrentUser

  nitro_found:
FunctionEnd

Function RunNitroSenseUninstall
  StrCpy $4 $NitroSenseUninstallString
  StrCpy $5 $4 11
  StrCmp $5 "MsiExec.exe" 0 nitro_uninstall_generic
    StrCpy $6 $4 "" 11
    StrCpy $6 "$6 /passive /norestart"
    ExecWait '"$SYSDIR\msiexec.exe"$6' $7
    Goto nitro_uninstall_done

  nitro_uninstall_generic:
    ExecWait '$4' $7

  nitro_uninstall_done:
    ${If} $7 = 0
    ${OrIf} $7 = 1605
    ${OrIf} $7 = 1641
    ${OrIf} $7 = 3010
      Return
    ${EndIf}

    MessageBox MB_ICONSTOP|MB_OK "AeroForge Control could not uninstall $NitroSenseDisplayName. NitroSense uninstall exited with code $7."
    Abort
FunctionEnd

!macro NSIS_HOOK_PREINSTALL
  Call StopAeroForgeRuntimeForInstall
  Call DetectNitroSense
  StrCmp $NitroSenseUninstallString "" nitro_preinstall_done

  IfSilent 0 nitro_prompt_user
    IfFileExists "$INSTDIR\uninstall.exe" nitro_preinstall_done 0
    IfFileExists "$INSTDIR\aeroforge-control.exe" nitro_preinstall_done 0
    MessageBox MB_ICONSTOP|MB_OK "AeroForge Control cannot continue in silent mode while $NitroSenseDisplayName is installed."
    Abort

  nitro_prompt_user:
  MessageBox MB_ICONEXCLAMATION|MB_YESNO "$NitroSenseDisplayName is installed.$\r$\n$\r$\nInstalling AeroForge Control will uninstall $NitroSenseDisplayName before setup continues.$\r$\n$\r$\nSelect Yes to uninstall NitroSense and continue, or No to cancel AeroForge setup." IDYES nitro_continue IDNO nitro_cancel

  nitro_cancel:
    Abort

  nitro_continue:
    Call RunNitroSenseUninstall

  nitro_preinstall_done:
    Call CleanAeroForgeInstallDirForInstall
!macroend

Function InstallAeroForgeService
  IfFileExists "$INSTDIR\Install-AeroForgeBundledService.ps1" 0 aeroforge_service_missing
    StrCpy $AeroForgeInstallPawnIO "0"
    IfSilent aeroforge_pawnio_prompt_done 0
    IfFileExists "$INSTDIR\PawnIO_setup.exe" 0 aeroforge_pawnio_prompt_done
      MessageBox MB_ICONQUESTION|MB_YESNO "Enable CPU wattage and PL1/PL2 readback/control?$\r$\n$\r$\nThis installs the open-source PawnIO low-level driver runtime used for CPU MSR/RAPL access.$\r$\n$\r$\nSelect Yes to install/configure PawnIO, or No to continue without CPU watt/PL readback." IDYES aeroforge_pawnio_yes IDNO aeroforge_pawnio_prompt_done

    aeroforge_pawnio_yes:
      StrCpy $AeroForgeInstallPawnIO "1"

    aeroforge_pawnio_prompt_done:
    StrCmp $AeroForgeInstallPawnIO "1" 0 aeroforge_service_without_pawnio
      ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\Install-AeroForgeBundledService.ps1" -InstallPawnIO -ServiceSource "$INSTDIR\aeroforge-service.exe"' $8
      Goto aeroforge_service_installed

    aeroforge_service_without_pawnio:
      ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\Install-AeroForgeBundledService.ps1" -ServiceSource "$INSTDIR\aeroforge-service.exe"' $8

    aeroforge_service_installed:
    ${If} $8 = 0
      Return
    ${EndIf}

    MessageBox MB_ICONSTOP|MB_OK "AeroForge Control could not install AeroForgeService. The service installer exited with code $8.$\r$\n$\r$\nOpen this log for the exact Windows service error:$\r$\n$COMMONAPPDATA\AeroForge\Service\logs\installer-service.log"
    Abort

  aeroforge_service_missing:
    MessageBox MB_ICONSTOP|MB_OK "AeroForge Control could not install AeroForgeService because bundled service resources are missing."
    Abort
FunctionEnd

Function InstallAeroForgeUserRuntime
  IfFileExists "$INSTDIR\aeroforge-hotkey-helper.exe" 0 aeroforge_runtime_missing
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AeroForgeHotkeyHelper" '"$INSTDIR\aeroforge-hotkey-helper.exe" --daemon'
    Exec '"$INSTDIR\aeroforge-hotkey-helper.exe" --daemon'
    Return

  aeroforge_runtime_missing:
    DetailPrint "AeroForge hotkey helper missing; background update checks will start after AeroForge opens."
FunctionEnd

Function un.UninstallAeroForgeService
  IfFileExists "$INSTDIR\Install-AeroForgeBundledService.ps1" 0 aeroforge_service_uninstall_fallback
    ExecWait '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\Install-AeroForgeBundledService.ps1" -Uninstall -ServiceSource "$INSTDIR\aeroforge-service.exe"' $8
    ${If} $8 = 0
      Return
    ${EndIf}

    MessageBox MB_ICONSTOP|MB_OK "AeroForge Control could not remove AeroForgeService. The service uninstaller exited with code $8.$\r$\n$\r$\nOpen this log for the exact Windows service error:$\r$\n$COMMONAPPDATA\AeroForge\Service\logs\installer-service.log"
    Abort

  aeroforge_service_uninstall_fallback:
    ExecWait '"$SYSDIR\sc.exe" stop AeroForgeService' $8
    ExecWait '"$SYSDIR\sc.exe" delete AeroForgeService' $8
    ${If} $8 = 0
    ${OrIf} $8 = 1060
    ${OrIf} $8 = 1072
      Return
    ${EndIf}

    MessageBox MB_ICONSTOP|MB_OK "AeroForge Control could not remove AeroForgeService because bundled service resources are missing and fallback service deletion failed with code $8."
    Abort
FunctionEnd

Function un.MarkAeroForgeInstallDirForWipe
  StrCpy $AeroForgeInstallDirSafeToWipe "0"
  IfFileExists "$INSTDIR\aeroforge-control.exe" 0 check_display_exe_uninstall
    Goto mark_install_dir_safe
  check_display_exe_uninstall:
  IfFileExists "$INSTDIR\AeroForge Control.exe" 0 check_service_exe_uninstall
    Goto mark_install_dir_safe
  check_service_exe_uninstall:
  IfFileExists "$INSTDIR\aeroforge-service.exe" 0 check_helper_exe_uninstall
    Goto mark_install_dir_safe
  check_helper_exe_uninstall:
  IfFileExists "$INSTDIR\aeroforge-hotkey-helper.exe" 0 check_service_script_uninstall
    Goto mark_install_dir_safe
  check_service_script_uninstall:
  IfFileExists "$INSTDIR\Install-AeroForgeBundledService.ps1" 0 mark_install_dir_done
    Goto mark_install_dir_safe

  mark_install_dir_safe:
    StrCpy $AeroForgeInstallDirSafeToWipe "1"

  mark_install_dir_done:
FunctionEnd

Function un.RemoveAeroForgeInstallDir
  StrCmp $AeroForgeInstallDirSafeToWipe "1" 0 remove_install_dir_done
    DetailPrint "Removing remaining AeroForge install files from $INSTDIR..."
    RMDir /r /REBOOTOK "$INSTDIR"

  remove_install_dir_done:
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  Call InstallAeroForgeService
  Call InstallAeroForgeUserRuntime
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Call un.MarkAeroForgeInstallDirForWipe
  Call un.StopAeroForgeRuntimeForUninstall
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "AeroForgeHotkeyHelper"
  Call un.UninstallAeroForgeService
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Call un.RemoveAeroForgeInstallDir
!macroend
