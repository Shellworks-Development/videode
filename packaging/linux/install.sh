#!/usr/bin/env bash
# Installs the videode binary, launcher, icons and .vde file type for the
# current user. Run after `cargo build --release`.
set -euo pipefail
cd "$(dirname "$0")/../.."
prefix=${PREFIX:-$HOME/.local}
id=dev.videode.videode
install -Dm755 target/release/videode "$prefix/bin/videode"
install -Dm644 packaging/linux/$id.desktop "$prefix/share/applications/$id.desktop"
install -Dm644 packaging/linux/$id.xml "$prefix/share/mime/packages/$id.xml"
install -Dm644 assets/brand/videode.svg "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 assets/brand/png/videode-$s.png "$prefix/share/icons/hicolor/${s}x${s}/apps/$id.png"
done
update-mime-database "$prefix/share/mime" 2>/dev/null || true
update-desktop-database "$prefix/share/applications" 2>/dev/null || true
gtk-update-icon-cache -f -t "$prefix/share/icons/hicolor" 2>/dev/null || true
echo "installed videode to $prefix"
