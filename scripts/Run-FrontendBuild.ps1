$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$nodePath = & (Join-Path $PSScriptRoot 'Resolve-NodePath.ps1')
$tscCli = Join-Path $projectRoot 'node_modules\typescript\bin\tsc'
$viteCli = Join-Path $projectRoot 'node_modules\vite\bin\vite.js'

if (-not (Test-Path -LiteralPath $tscCli)) {
  throw "TypeScript CLI not found at $tscCli"
}

if (-not (Test-Path -LiteralPath $viteCli)) {
  throw "Vite CLI not found at $viteCli"
}

& $nodePath $tscCli -b
if ($LASTEXITCODE -ne 0) {
  throw 'TypeScript build failed.'
}

& $nodePath $viteCli build
if ($LASTEXITCODE -ne 0) {
  throw 'Vite build failed.'
}
