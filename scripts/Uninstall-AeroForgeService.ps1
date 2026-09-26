$ErrorActionPreference = 'Stop'

# The bundled uninstaller also removes the SYSTEM healer task, waits for the
# service to stop and be deleted, clears PawnIO machine variables and cleans
# ProgramData, so dev uninstalls leave nothing behind either.
$bundled = Join-Path $PSScriptRoot '..\src-tauri\resources\Install-AeroForgeBundledService.ps1'
& $bundled -Uninstall
exit $LASTEXITCODE
