# AeroForge Native Rust GUI Build Script
# Builds the pure Rust GUI application with the aeroforge-service

$ErrorActionPreference = "Stop"

Write-Host "Building AeroForge Native Rust GUI..." -ForegroundColor Cyan

# Build the service first
Write-Host "Building aeroforge-service..." -ForegroundColor Yellow
Push-Location "aeroforge-service"
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Pop-Location
    Write-Error "Failed to build aeroforge-service"
    exit 1
}
Pop-Location

# Build the GUI
Write-Host "Building aeroforge-gui..." -ForegroundColor Yellow
Push-Location "aeroforge-gui"
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Pop-Location
    Write-Error "Failed to build aeroforge-gui"
    exit 1
}
Pop-Location

Write-Host "Build completed successfully!" -ForegroundColor Green
Write-Host "Binaries:"
Write-Host "  Service: aeroforge-service\target\release\aeroforge-service.exe" -ForegroundColor Cyan
Write-Host "  GUI: aeroforge-gui\target\release\aeroforge-gui.exe" -ForegroundColor Cyan
