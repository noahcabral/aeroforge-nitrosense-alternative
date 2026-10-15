$ErrorActionPreference = 'Stop'

# Tauri's build script validates every bundle resource when the main crate compiles,
# so the service and hotkey helper must exist before cargo runs, not just before
# bundling. Build the frontend first, then the two resource executables.
& (Join-Path $PSScriptRoot 'Run-FrontendBuild.ps1')
& (Join-Path $PSScriptRoot 'Prepare-BundleResources.ps1')
