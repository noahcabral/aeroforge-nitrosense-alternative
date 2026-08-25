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

$candidates = @(
  'C:\Users\noah\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin\node.exe',
  'C:\Program Files\nodejs\node.exe'
)

$nodeCommand = Get-Command node.exe -ErrorAction SilentlyContinue
if ($nodeCommand) {
  $candidates += $nodeCommand.Source
}

foreach ($candidate in $candidates | Select-Object -Unique) {
  if (Test-NodeCandidate -Path $candidate) {
    return $candidate
  }
}

throw 'Unable to locate a runnable node.exe for AeroForge frontend build hooks.'
