# Regenerates all icon rasters from assets/icon.svg. Run from repo root. Needs ImageMagick (magick).
$ErrorActionPreference = 'Stop'
$svg = Join-Path $PWD 'assets/icon.svg'
$tmp = Join-Path $env:TEMP 'ge-bridge-icons'
New-Item -ItemType Directory -Force $tmp, 'extension/icons' | Out-Null
function Render($size, $out) {
  magick -background none -density 1536 $svg -resize "${size}x${size}" -define png:color-type=6 $out
  if ($LASTEXITCODE -ne 0) { throw "magick failed for $size" }
}
foreach ($s in 16, 32, 48, 128) { Render $s "extension/icons/icon$s.png" }
$frames = foreach ($s in 16, 24, 32, 48, 64, 256) { $f = Join-Path $tmp "i$s.png"; Render $s $f; $f }
magick @frames assets/ge-bridge.ico
if ($LASTEXITCODE -ne 0) { throw 'ico failed' }
Remove-Item -Recurse -Force $tmp
