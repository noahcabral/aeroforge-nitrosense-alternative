$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$nodePath = & (Join-Path $PSScriptRoot 'Resolve-NodePath.ps1')
$viteCli = Join-Path $projectRoot 'node_modules\vite\bin\vite.js'

if (-not (Test-Path -LiteralPath $viteCli)) {
  throw "Vite CLI not found at $viteCli"
}

Push-Location $projectRoot
try {
  & $nodePath $viteCli --host 127.0.0.1 --port 1420
  if ($LASTEXITCODE -ne 0) {
    throw 'Vite dev server failed.'
  }
} finally {
  Pop-Location
}
