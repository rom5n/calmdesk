# Renders the logo SVGs with headless Edge: app.svg into app.ico (16-256 px, PNG frames; the
# exe and tray icon), logo.svg and logo-dark.svg into logo.png and logo-dark.png (256 px, drawn
# by the gallery and the installer in the light and dark theme). Run from any folder:
#   powershell -ExecutionPolicy Bypass -File crates\manager\assets\make_icon.ps1
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
$here = $PSScriptRoot
$edge = @("${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe", "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) { throw "Microsoft Edge not found" }

$profile = Join-Path $env:TEMP "calmdesk-logo-edge"
function Render([string]$svg) {
    $big = Join-Path $env:TEMP "calmdesk-logo-1024.png"
    if (Test-Path $big) { Remove-Item $big }
    $url = "file:///" + (Join-Path $here $svg).Replace("\", "/")
    & $edge --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$profile" --default-background-color=00000000 --window-size=1024,1024 --screenshot="$big" $url | Out-Null
    for ($i = 0; -not (Test-Path $big) -and $i -lt 50; $i++) { Start-Sleep -Milliseconds 100 }
    $bytes = [System.IO.File]::ReadAllBytes($big)
    Remove-Item $big
    [System.Drawing.Image]::FromStream((New-Object System.IO.MemoryStream (, $bytes)))
}

function Png([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    $g.DrawImage($src, 0, 0, $size, $size)
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    , $ms.ToArray()
}

$src = Render "logo.svg"; [System.IO.File]::WriteAllBytes((Join-Path $here "logo.png"), (Png 256)); $src.Dispose()
$src = Render "logo-dark.svg"; [System.IO.File]::WriteAllBytes((Join-Path $here "logo-dark.png"), (Png 256)); $src.Dispose()
$src = Render "app.svg"

# ICO: header, one 16-byte entry per size, then the PNG frames.
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$frames = $sizes | ForEach-Object { , (Png $_) }
$out = New-Object System.IO.MemoryStream
$w = New-Object System.IO.BinaryWriter $out
$w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $s = $sizes[$i] % 256
    $w.Write([byte]$s); $w.Write([byte]$s); $w.Write([byte]0); $w.Write([byte]0)
    $w.Write([uint16]1); $w.Write([uint16]32)
    $w.Write([uint32]$frames[$i].Length); $w.Write([uint32]$offset)
    $offset += $frames[$i].Length
}
foreach ($f in $frames) { $w.Write($f) }
[System.IO.File]::WriteAllBytes((Join-Path $here "app.ico"), $out.ToArray())
$src.Dispose()
"wrote app.ico, logo.png and logo-dark.png"
