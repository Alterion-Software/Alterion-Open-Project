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
# Windows file version resources must be four numbers. A pre-release suffix
# like "-beta" is meaningful to people and meaningless to the resource format,
# so it is kept in the display name and stripped from the numeric one.
numeric="${version%%-*}"
numeric="$numeric.0"
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

step "Collecting runtime libraries"
# A binary built against the GNU toolchain can want a few MinGW libraries
# beside it. Which ones depends on the toolchain, so they are read out of the
# binary rather than guessed at. Anything the system provides (a Windows DLL)
# is skipped; only the MinGW ones travel with us.
exe="$staging/alterion-open-project.exe"
if command -v x86_64-w64-mingw32-objdump >/dev/null; then
  mingw_bin="$(dirname "$(command -v x86_64-w64-mingw32-gcc)")/../x86_64-w64-mingw32/bin"
  # Where cargo leaves libraries that ride along with a crate, such as the
  # WebView2 loader. Newest first, so a rebuilt one wins.
  build_out="$root/target/$target/release/build"
  found=0
  while read -r dll; do
    src=""
    for dir in "$mingw_bin" /usr/x86_64-w64-mingw32/bin; do
      [ -f "$dir/$dll" ] && { src="$dir/$dll"; break; }
    done
    if [ -z "$src" ]; then
      src="$(find "$build_out" -name "$dll" -path '*x64*' -printf '%T@ %p\n' 2>/dev/null \
             | sort -rn | head -1 | cut -d' ' -f2-)"
    fi
    if [ -n "$src" ] && [ -f "$src" ]; then
      cp "$src" "$staging/"
      echo "  bundled $dll"
      found=$((found + 1))
    else
      echo "  WARNING: $dll is needed and was not found" >&2
    fi
  done < <(x86_64-w64-mingw32-objdump -p "$exe" 2>/dev/null \
            | sed -n 's/^\s*DLL Name: //p' | sort -u \
            | grep -iE '^(libgcc|libstdc\+\+|libwinpthread|libssp|WebView2Loader)')
  [ "$found" -eq 0 ] && echo "  none needed, the binary is self contained"
else
  echo "  objdump not found, skipping the check" >&2
fi

step "Fetching the WebView2 bootstrapper"
# The window is a WebView2 control. Windows 11 ships the runtime and most
# updated Windows 10 machines have it, but a clean machine may not, and without
# it the application starts and shows nothing. The bootstrapper is about two
# megabytes and pulls the runtime down itself.
wv2="$here/MicrosoftEdgeWebview2Setup.exe"
if [ ! -f "$wv2" ]; then
  if command -v curl >/dev/null; then
    curl -fsSL -o "$wv2" "https://go.microsoft.com/fwlink/p/?LinkId=2124703" || rm -f "$wv2"
  elif command -v wget >/dev/null; then
    wget -qO "$wv2" "https://go.microsoft.com/fwlink/p/?LinkId=2124703" || rm -f "$wv2"
  fi
fi
if [ -f "$wv2" ]; then
  cp "$wv2" "$staging/"
  echo "  bundled the bootstrapper"
else
  echo "  could not fetch it; the installer will tell the user where to get it" >&2
fi

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
makensis -DVERSION="$version" -DFILEVERSION="$numeric" installer.nsi
mv dist/*.exe "$dist/"

step "Done"
ls -lh "$dist"
