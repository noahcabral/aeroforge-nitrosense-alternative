# Run AeroForge GUI in development mode
$ErrorActionPreference = "Stop"

Write-Host "Starting AeroForge GUI (development mode)..." -ForegroundColor Cyan

# Ensure service is running first
$serviceStatus = sc.exe query AeroForgeService 2>&1
if ($serviceStatus -match "RUNNING") {
    Write-Host "AeroForge service is running" -ForegroundColor Green
} else {
    Write-Warning "AeroForge service is not running. Start it with: sc.exe start AeroForgeService"
}

# Run GUI in debug mode
Push-Location "aeroforge-gui"
cargo run
Pop-Location
