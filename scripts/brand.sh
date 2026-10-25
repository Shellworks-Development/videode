#!/usr/bin/env bash
# Regenerates every raster brand asset from the SVG masters in assets/brand.
# Needs rsvg-convert and Python 3 with Pillow.
set -euo pipefail
cd "$(dirname "$0")/.."
src=assets/brand
out=$src/png
mkdir -p "$out"

# Let rsvg find the bundled Geist fonts for the lockups.
fc=$(mktemp -d)
trap 'rm -rf "$fc"' EXIT
cat > "$fc/fonts.conf" <<CONF
<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig><dir>$PWD/assets/fonts</dir><include ignore_missing="yes">/etc/fonts/fonts.conf</include><cachedir>$fc</cachedir></fontconfig>
CONF
export FONTCONFIG_FILE="$fc/fonts.conf"

for s in 16 24 32; do rsvg-convert -w $s -h $s $src/videode-small.svg -o $out/videode-$s.png; done
for s in 48 64 128 256 512 1024; do rsvg-convert -w $s -h $s $src/videode.svg -o $out/videode-$s.png; done
for v in dark light; do rsvg-convert -w 1120 -h 320 $src/videode-lockup-$v.svg -o $out/videode-lockup-$v.png; done

python3 -I - "$out" "$src" <<'PY'
import sys
from PIL import Image
out, src = sys.argv[1], sys.argv[2]
img = lambda s: Image.open(f"{out}/videode-{s}.png").convert("RGBA")
sizes = [16, 24, 32, 48, 64, 128, 256]
# Windows: one .ico holding every size, each drawn from its own master.
img(256).save(f"{src}/videode.ico", sizes=[(s, s) for s in sizes],
              append_images=[img(s) for s in sizes[:-1]])
# macOS placeholder until the Icon Composer (Liquid Glass) icon replaces it.
img(1024).save(f"{src}/videode.icns")
PY
echo "brand assets written to $src"
