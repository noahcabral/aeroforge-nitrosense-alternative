param(
  [string]$Repo = 'noahcabral/aeroforge-nitrosense-alternative',
  [string]$Tag = '',
  [string]$TargetCommitish = 'main',
  [string]$Name = '',
  [string]$BodyFile = '',
  [switch]$Prerelease
)

$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$package = Get-Content (Join-Path $projectRoot 'package.json') | ConvertFrom-Json
$version = $package.version
if ([string]::IsNullOrWhiteSpace($Tag)) {
  $Tag = "v$version"
}
if ([string]::IsNullOrWhiteSpace($Name)) {
  $Name = "AeroForge Control v$version"
}

$makeDebugCollector = Join-Path $projectRoot 'scripts\Make-DebugCollector.ps1'
& $makeDebugCollector

$portable = Join-Path $projectRoot "portable\AeroForge-Control-Portable-$version.zip"
$installer = Join-Path $projectRoot "portable\AeroForge-Control-Setup-$version.exe"
$debugger = Join-Path $projectRoot "portable\AeroForge-Debug-Collector-$version.zip"
$assets = @($portable, $installer, $debugger)
foreach ($asset in $assets) {
  if (-not (Test-Path -LiteralPath $asset)) {
    throw "Missing release asset: $asset"
  }
}

$bodyWasProvided = -not [string]::IsNullOrWhiteSpace($BodyFile)
if ($bodyWasProvided) {
  $body = [string](Get-Content -LiteralPath $BodyFile -Raw)
} else {
  $body = @"
AeroForge Control v$version

Release assets:
- AeroForge-Control-Portable-$version.zip
- AeroForge-Control-Setup-$version.exe
- AeroForge-Debug-Collector-$version.zip

The debug collector is included as a standalone asset on every release so users can file support bundles even when AeroForge will not install or launch.
"@
}

$credentialInput = "protocol=https`nhost=github.com`n`n"
$credentialOutput = $credentialInput | git credential fill
$token = ($credentialOutput -split "`n" | Where-Object { $_ -like 'password=*' } | Select-Object -First 1) -replace '^password=', ''
if (-not $token) {
  throw 'Could not resolve GitHub credential token from git credential manager.'
}

$headers = @{
  Authorization = "Bearer $token"
  Accept = 'application/vnd.github+json'
  'X-GitHub-Api-Version' = '2022-11-28'
  'User-Agent' = 'AeroForgeReleasePublisher'
}
$apiBase = "https://api.github.com/repos/$Repo"

try {
  $release = Invoke-RestMethod -Headers $headers -Uri "$apiBase/releases/tags/$Tag" -Method Get
  if (-not $bodyWasProvided -and -not [string]::IsNullOrWhiteSpace($release.body)) {
    $body = [string]$release.body
    $debugAssetName = "AeroForge-Debug-Collector-$version.zip"
    if ($body -notmatch [regex]::Escape($debugAssetName)) {
      $body = $body.TrimEnd() + [Environment]::NewLine + [Environment]::NewLine + "Release asset added: $debugAssetName for standalone support diagnostics."
    }
  }
  $release = Invoke-RestMethod -Headers $headers -Uri "$apiBase/releases/$($release.id)" -Method Patch -ContentType 'application/json' -Body (@{
    name = $Name
    body = [string]$body
    draft = $false
    prerelease = [bool]$Prerelease
    make_latest = 'true'
  } | ConvertTo-Json)
} catch {
  $status = $null
  if ($_.Exception.Response) {
    $status = [int]$_.Exception.Response.StatusCode
  }
  if ($status -ne 404) {
    throw
  }
  $release = Invoke-RestMethod -Headers $headers -Uri "$apiBase/releases" -Method Post -ContentType 'application/json' -Body (@{
    tag_name = $Tag
    target_commitish = $TargetCommitish
    name = $Name
    body = [string]$body
    draft = $false
    prerelease = [bool]$Prerelease
    make_latest = 'true'
  } | ConvertTo-Json)
}

$uploadBase = ($release.upload_url -replace '\{.*$', '')
foreach ($asset in $assets) {
  $assetName = Split-Path -Leaf $asset
  foreach ($existing in @($release.assets | Where-Object { $_.name -eq $assetName })) {
    Invoke-RestMethod -Headers $headers -Uri "$apiBase/releases/assets/$($existing.id)" -Method Delete | Out-Null
  }

  $contentType = if ($assetName.EndsWith('.zip')) { 'application/zip' } else { 'application/octet-stream' }
  $uploadUri = "$($uploadBase)?name=$([uri]::EscapeDataString($assetName))"
  Invoke-RestMethod -Headers $headers -Uri $uploadUri -Method Post -ContentType $contentType -InFile $asset | Out-Null
}

$published = Invoke-RestMethod -Headers $headers -Uri "$apiBase/releases/tags/$Tag" -Method Get
Write-Output "Published release: $($published.html_url)"
foreach ($asset in $published.assets) {
  Write-Output "Asset: $($asset.name) $($asset.size) bytes"
}
