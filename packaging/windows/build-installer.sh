#!/usr/bin/env bash
# Cross-build the Windows binary and wrap it in an NSIS installer.
#
# Run from anywhere; paths are resolved from the script's own location.
#
#   packaging/windows/build-installer.sh
#
# Needs: rustup target add x86_64-pc-windows-gnu
#        mingw-w64-gcc  (Arch: pacman -S mingw-w64-gcc)
#        nsis           (Arch: pacman -S nsis)
#        icoutils or imagemagick, to turn the SVG icon into an .ico

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
target="x86_64-pc-windows-gnu"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
staging="$here/stage"
dist="$here/dist"

step() { printf '\n\033[36m==>\033[0m %s\n' "$1"; }
need() { command -v "$1" >/dev/null || { echo "missing: $1 ($2)" >&2; exit 1; }; }

step "Checking tools"
need cargo "rustup"
need makensis "nsis"
if ! rustup target list --installed | grep -qx "$target"; then
  echo "missing rust target $target" >&2
  echo "  rustup target add $target" >&2
  exit 1
fi
if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
  echo "missing x86_64-w64-mingw32-gcc (mingw-w64-gcc)" >&2
  exit 1
fi

step "Building aop-app $version for $target"
cd "$root"
cargo build --release --target "$target" --package aop-app

step "Staging"
rm -rf "$staging"
mkdir -p "$staging" "$dist"
cp "$root/target/$target/release/alterion-open-project.exe" "$staging/alterion-open-project.exe"
cp "$root/LICENSE" "$staging/LICENSE.txt"
cp "$root/README.md" "$staging/README.md"
cp "$here/installer.nsi" "$staging/installer.nsi"

step "Building the icon"
icon_svg="$root/packaging/linux/alterion-open-project.svg"
if command -v magick >/dev/null; then
  # Windows wants several sizes inside one .ico.
  tmp="$(mktemp -d)"
  for size in 16 24 32 48 64 128 256; do
    magick -background none "$icon_svg" -resize "${size}x${size}" "$tmp/$size.png"
  done
  magick "$tmp"/*.png "$staging/app.ico"
  rm -rf "$tmp"
elif command -v convert >/dev/null; then
  convert -background none "$icon_svg" -define icon:auto-resize=256,128,64,48,32,16 "$staging/app.ico"
else
  echo "no imagemagick; the installer will use a blank icon" >&2
  : > "$staging/app.ico"
fi

step "Building the installer"
cd "$staging"
mkdir -p dist
makensis -DVERSION="$version" installer.nsi
mv dist/*.exe "$dist/"

step "Done"
ls -lh "$dist"
