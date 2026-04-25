Add-Type -AssemblyName System.Drawing

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$iconDir = Join-Path $repoRoot 'src-tauri\icons'

function New-RoundedRectPath {
  param(
    [float]$X,
    [float]$Y,
    [float]$Width,
    [float]$Height,
    [float]$Radius
  )

  $diameter = $Radius * 2
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $path.AddArc($X, $Y, $diameter, $diameter, 180, 90)
  $path.AddArc($X + $Width - $diameter, $Y, $diameter, $diameter, 270, 90)
  $path.AddArc($X + $Width - $diameter, $Y + $Height - $diameter, $diameter, $diameter, 0, 90)
  $path.AddArc($X, $Y + $Height - $diameter, $diameter, $diameter, 90, 90)
  $path.CloseFigure()
  return $path
}

function New-PointF {
  param(
    [float]$X,
    [float]$Y
  )

  return [System.Drawing.PointF]::new($X, $Y)
}

function Fill-Polygon {
  param(
    [System.Drawing.Graphics]$Graphics,
    [System.Drawing.Brush]$Brush,
    [System.Drawing.PointF[]]$Points
  )

  $Graphics.FillPolygon($Brush, $Points)
}

function Draw-AeroForgeIcon {
  param(
    [int]$Size,
    [string]$OutputPath
  )

  $bitmap = [System.Drawing.Bitmap]::new($Size, $Size)
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $graphics.Clear([System.Drawing.Color]::Transparent)

  $radius = $Size * 0.22
  $canvas = New-RoundedRectPath 0 0 $Size $Size $radius
  $bg = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.PointF]::new($Size * 0.08, $Size * 0.06),
    [System.Drawing.PointF]::new($Size * 0.92, $Size * 0.94),
    [System.Drawing.Color]::FromArgb(255, 27, 18, 14),
    [System.Drawing.Color]::FromArgb(255, 14, 10, 9)
  )
  $bg.Blend = [System.Drawing.Drawing2D.Blend]::new()
  $bg.Blend.Positions = @(0.0, 0.55, 1.0)
  $bg.Blend.Factors = @(0.0, 0.65, 1.0)
  $graphics.FillPath($bg, $canvas)

  $topGlowBrush = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.PointF]::new($Size * 0.18, $Size * 0.10),
    [System.Drawing.PointF]::new($Size * 0.82, $Size * 0.22),
    [System.Drawing.Color]::FromArgb(230, 255, 139, 67),
    [System.Drawing.Color]::FromArgb(225, 255, 204, 122)
  )
  Fill-Polygon $graphics $topGlowBrush @(
    (New-PointF ($Size * 0.17) ($Size * 0.16)),
    (New-PointF ($Size * 0.35) ($Size * 0.10)),
    (New-PointF ($Size * 0.66) ($Size * 0.10)),
    (New-PointF ($Size * 0.83) ($Size * 0.16)),
    (New-PointF ($Size * 0.76) ($Size * 0.22)),
    (New-PointF ($Size * 0.24) ($Size * 0.22))
  )

  $leftBlade = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.PointF]::new($Size * 0.24, $Size * 0.20),
    [System.Drawing.PointF]::new($Size * 0.48, $Size * 0.82),
    [System.Drawing.Color]::FromArgb(255, 255, 126, 54),
    [System.Drawing.Color]::FromArgb(255, 183, 68, 24)
  )
  Fill-Polygon $graphics $leftBlade @(
    (New-PointF ($Size * 0.23) ($Size * 0.73)),
    (New-PointF ($Size * 0.39) ($Size * 0.22)),
    (New-PointF ($Size * 0.50) ($Size * 0.22)),
    (New-PointF ($Size * 0.35) ($Size * 0.73))
  )

  $rightBlade = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.PointF]::new($Size * 0.70, $Size * 0.20),
    [System.Drawing.PointF]::new($Size * 0.50, $Size * 0.82),
    [System.Drawing.Color]::FromArgb(255, 255, 208, 116),
    [System.Drawing.Color]::FromArgb(255, 240, 106, 36)
  )
  Fill-Polygon $graphics $rightBlade @(
    (New-PointF ($Size * 0.52) ($Size * 0.22)),
    (New-PointF ($Size * 0.63) ($Size * 0.22)),
    (New-PointF ($Size * 0.78) ($Size * 0.73)),
    (New-PointF ($Size * 0.67) ($Size * 0.73))
  )

  $centerBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(255, 255, 242, 233))
  Fill-Polygon $graphics $centerBrush @(
    (New-PointF ($Size * 0.37) ($Size * 0.56)),
    (New-PointF ($Size * 0.45) ($Size * 0.34)),
    (New-PointF ($Size * 0.56) ($Size * 0.34)),
    (New-PointF ($Size * 0.63) ($Size * 0.56)),
    (New-PointF ($Size * 0.52) ($Size * 0.56)),
    (New-PointF ($Size * 0.50) ($Size * 0.48)),
    (New-PointF ($Size * 0.48) ($Size * 0.56))
  )

  $innerGlow = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(198, 255, 179, 108))
  Fill-Polygon $graphics $innerGlow @(
    (New-PointF ($Size * 0.29) ($Size * 0.65)),
    (New-PointF ($Size * 0.50) ($Size * 0.27)),
    (New-PointF ($Size * 0.71) ($Size * 0.65)),
    (New-PointF ($Size * 0.64) ($Size * 0.69)),
    (New-PointF ($Size * 0.50) ($Size * 0.45)),
    (New-PointF ($Size * 0.36) ($Size * 0.69))
  )

  $baseGlow = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
    [System.Drawing.PointF]::new($Size * 0.20, $Size * 0.78),
    [System.Drawing.PointF]::new($Size * 0.80, $Size * 0.84),
    [System.Drawing.Color]::FromArgb(255, 255, 123, 45),
    [System.Drawing.Color]::FromArgb(255, 255, 190, 115)
  )
  Fill-Polygon $graphics $baseGlow @(
    (New-PointF ($Size * 0.20) ($Size * 0.79)),
    (New-PointF ($Size * 0.29) ($Size * 0.70)),
    (New-PointF ($Size * 0.71) ($Size * 0.70)),
    (New-PointF ($Size * 0.80) ($Size * 0.79)),
    (New-PointF ($Size * 0.80) ($Size * 0.84)),
    (New-PointF ($Size * 0.20) ($Size * 0.84))
  )

  $framePen = [System.Drawing.Pen]::new([System.Drawing.Color]::FromArgb(38, 255, 255, 255), [Math]::Max(1, $Size * 0.01))
  $graphics.DrawPath($framePen, $canvas)

  $bitmap.Save($OutputPath, [System.Drawing.Imaging.ImageFormat]::Png)

  $framePen.Dispose()
  $baseGlow.Dispose()
  $innerGlow.Dispose()
  $centerBrush.Dispose()
  $rightBlade.Dispose()
  $leftBlade.Dispose()
  $topGlowBrush.Dispose()
  $bg.Dispose()
  $canvas.Dispose()
  $graphics.Dispose()
  $bitmap.Dispose()
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
      $img.Dispose()
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

foreach ($target in $pngTargets) {
  Draw-AeroForgeIcon -Size $target.Size -OutputPath (Join-Path $iconDir $target.Name)
}

$icoPngs = @('32x32.png', '128x128.png', '128x128@2x.png') |
  ForEach-Object { Join-Path $iconDir $_ }

Write-IcoFromPngs -PngPaths $icoPngs -OutputPath (Join-Path $iconDir 'icon.ico')

Write-Host 'AeroForge icon assets regenerated.'
