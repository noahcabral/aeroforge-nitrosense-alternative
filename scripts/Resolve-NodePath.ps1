$ErrorActionPreference = 'Stop'

function Test-NodeCandidate {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Path
  )

  if (-not (Test-Path -LiteralPath $Path)) {
    return $false
  }

  try {
    & $Path --version *> $null
    return $LASTEXITCODE -eq 0
  } catch {
    return $false
  }
}

# Explicit override first, then PATH, then the standard Node installer locations.
$candidates = @()
if ($env:AEROFORGE_NODE) {
  $candidates += $env:AEROFORGE_NODE
}

$nodeCommand = Get-Command node.exe -ErrorAction SilentlyContinue
if ($nodeCommand) {
  $candidates += $nodeCommand.Source
}

$candidates += 'C:\Program Files\nodejs\node.exe'
if ($env:LOCALAPPDATA) {
  $candidates += (Join-Path $env:LOCALAPPDATA 'Programs\nodejs\node.exe')
}

foreach ($candidate in $candidates | Select-Object -Unique) {
  if (Test-NodeCandidate -Path $candidate) {
    return $candidate
  }
}

throw 'Unable to locate a runnable node.exe for AeroForge frontend build hooks. Install Node.js 22+ or set AEROFORGE_NODE to the full path of node.exe.'
