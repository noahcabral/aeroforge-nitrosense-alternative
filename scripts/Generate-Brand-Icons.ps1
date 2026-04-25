Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$iconDir = Join-Path $repoRoot 'src-tauri\icons'
$publicDir = Join-Path $repoRoot 'public'
$sourcePath = Join-Path $repoRoot 'src\assets\aeroforge-mark.png'

if (-not (Test-Path -LiteralPath $sourcePath)) {
  throw "Brand source image not found: $sourcePath"
}

function Write-ResizedPng {
  param(
    [System.Drawing.Image]$SourceImage,
    [int]$Size,
    [string]$OutputPath
  )

  $bitmap = [System.Drawing.Bitmap]::new($Size, $Size)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)

  try {
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $graphics.Clear([System.Drawing.Color]::Transparent)

    $scale = [Math]::Min($Size / $SourceImage.Width, $Size / $SourceImage.Height)
    $drawWidth = [int][Math]::Round($SourceImage.Width * $scale)
    $drawHeight = [int][Math]::Round($SourceImage.Height * $scale)
    $offsetX = [int][Math]::Round(($Size - $drawWidth) / 2)
    $offsetY = [int][Math]::Round(($Size - $drawHeight) / 2)

    $graphics.DrawImage(
      $SourceImage,
      [System.Drawing.Rectangle]::new($offsetX, $offsetY, $drawWidth, $drawHeight)
    )

    if (Test-Path -LiteralPath $OutputPath) {
      Remove-Item -LiteralPath $OutputPath -Force
    }

    $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)
  }
  finally {
    $graphics.Dispose()
    $bitmap.Dispose()
  }
}

function Write-IcoFromPngs {
  param(
    [string[]]$PngPaths,
    [string]$OutputPath
  )

  $writer = [System.IO.BinaryWriter]::new([System.IO.File]::Open($OutputPath, [System.IO.FileMode]::Create))
  try {
    $count = $PngPaths.Count
    $writer.Write([UInt16]0)
    $writer.Write([UInt16]1)
    $writer.Write([UInt16]$count)

    $imageBlobs = @()
    $offset = 6 + (16 * $count)

    foreach ($path in $PngPaths) {
      $bytes = [System.IO.File]::ReadAllBytes($path)
      $img = [System.Drawing.Image]::FromFile($path)
      try {
        $width = if ($img.Width -ge 256) { 0 } else { [byte]$img.Width }
        $height = if ($img.Height -ge 256) { 0 } else { [byte]$img.Height }

        $writer.Write([byte]$width)
        $writer.Write([byte]$height)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([UInt16]1)
        $writer.Write([UInt16]32)
        $writer.Write([UInt32]$bytes.Length)
        $writer.Write([UInt32]$offset)

        $offset += $bytes.Length
        $imageBlobs += ,$bytes
      }
      finally {
        $img.Dispose()
      }
    }

    foreach ($blob in $imageBlobs) {
      $writer.Write($blob)
    }
  }
  finally {
    $writer.Dispose()
  }
}

$pngTargets = @(
  @{ Name = '32x32.png'; Size = 32 },
  @{ Name = '128x128.png'; Size = 128 },
  @{ Name = '128x128@2x.png'; Size = 256 },
  @{ Name = 'icon.png'; Size = 512 },
  @{ Name = 'Square30x30Logo.png'; Size = 30 },
  @{ Name = 'Square44x44Logo.png'; Size = 44 },
  @{ Name = 'Square71x71Logo.png'; Size = 71 },
  @{ Name = 'Square89x89Logo.png'; Size = 89 },
  @{ Name = 'Square107x107Logo.png'; Size = 107 },
  @{ Name = 'Square142x142Logo.png'; Size = 142 },
  @{ Name = 'Square150x150Logo.png'; Size = 150 },
  @{ Name = 'Square284x284Logo.png'; Size = 284 },
  @{ Name = 'Square310x310Logo.png'; Size = 310 },
  @{ Name = 'StoreLogo.png'; Size = 50 }
)

$sourceImage = [System.Drawing.Image]::FromFile($sourcePath)
try {
  foreach ($target in $pngTargets) {
    Write-ResizedPng -SourceImage $sourceImage -Size $target.Size -OutputPath (Join-Path $iconDir $target.Name)
  }

  Write-ResizedPng -SourceImage $sourceImage -Size 64 -OutputPath (Join-Path $publicDir 'favicon.png')
}
finally {
  $sourceImage.Dispose()
}

$icoPngs = @('32x32.png', '128x128.png', '128x128@2x.png') |
  ForEach-Object { Join-Path $iconDir $_ }

Write-IcoFromPngs -PngPaths $icoPngs -OutputPath (Join-Path $iconDir 'icon.ico')

Write-Host 'AeroForge icon assets regenerated from src\\assets\\aeroforge-mark.png.'
