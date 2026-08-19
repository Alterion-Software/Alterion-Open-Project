#!/usr/bin/env bash
# Build the macOS application bundle and wrap it in a disk image.
#
# Run this ON a Mac. Everything it uses (hdiutil, iconutil, sips, lipo,
# codesign) ships with macOS and the Xcode command line tools; there is no
# cross-compiling to Apple platforms from Linux, because the SDK may not be
# redistributed.
#
#   packaging/macos/build-dmg.sh
#
# Needs: Xcode command line tools   xcode-select --install
#        Rust                       https://rustup.rs
#
# Signing is optional and off by default. Without it the disk image still
# works, but see "Opening it the first time" at the end of the run.
#
#   SIGN_ID="Developer ID Application: Your Name (TEAMID)" \
#   NOTARY_PROFILE=alterion \
#     packaging/macos/build-dmg.sh

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
# CFBundleVersion has to be numeric, so a pre-release suffix is dropped from it
# and kept in the version people actually read.
build_number="${version%%-*}"

app_name="Alterion Open Project"
bin_name="alterion-open-project"
staging="$here/stage"
dist="$here/dist"
app="$staging/$app_name.app"

step() { printf '\n\033[36m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$1" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }

step "Checking the machine"
[ "$(uname -s)" = "Darwin" ] || die "this has to run on macOS; Apple's SDK cannot be redistributed, so there is no cross-compile from Linux"
command -v cargo    >/dev/null || die "cargo not found; install Rust from https://rustup.rs"
command -v hdiutil  >/dev/null || die "hdiutil not found; install the Xcode command line tools with: xcode-select --install"
command -v iconutil >/dev/null || die "iconutil not found; install the Xcode command line tools with: xcode-select --install"

step "Building a universal binary"
# Both architectures, joined with lipo, so one download runs on an Apple
# silicon Mac and on an Intel one without Rosetta.
targets=(aarch64-apple-darwin x86_64-apple-darwin)
built=()
for target in "${targets[@]}"; do
  if ! rustup target list --installed | grep -qx "$target"; then
    echo "  adding $target"
    rustup target add "$target"
  fi
  echo "  building $target"
  ( cd "$root" && cargo build --release --target "$target" --package aop-app )
  built+=("$root/target/$target/release/$bin_name")
done

step "Assembling the bundle"
rm -rf "$staging"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$dist"

lipo -create -output "$app/Contents/MacOS/$bin_name" "${built[@]}"
chmod +x "$app/Contents/MacOS/$bin_name"
lipo -info "$app/Contents/MacOS/$bin_name" | sed 's/^/  /'

sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build_number/" \
  "$here/Info.plist" > "$app/Contents/Info.plist"
printf 'APPL????' > "$app/Contents/PkgInfo"

step "Building the icons"
# Two of them. One .icns holds every size the Finder, the Dock and Spotlight
# ask for, and a .aprj in a folder should look like a document rather than
# like a second copy of the application.

render_1024() {
  local svg="$1" out="$2"
  # Already a raster: sips resizes from it directly, so nothing has to render
  # an SVG at all.
  case "$svg" in
    *.png) cp "$svg" "$out"; return ;;
  esac
  if command -v rsvg-convert >/dev/null; then
    rsvg-convert -w 1024 -h 1024 "$svg" -o "$out"
  elif command -v magick >/dev/null; then
    magick -background none "$svg" -resize 1024x1024 "$out"
  elif command -v qlmanage >/dev/null; then
    # Nothing installed, so fall back to what macOS itself can already do.
    qlmanage -t -s 1024 -o "$(dirname "$out")" "$svg" >/dev/null 2>&1 || true
    [ -f "$(dirname "$out")/$(basename "$svg").png" ] \
      && mv "$(dirname "$out")/$(basename "$svg").png" "$out"
  fi
}

make_icns() {
  local svg="$1" name="$2"
  local iconset="$staging/$name.iconset"
  local source_png="$staging/$name-1024.png"
  mkdir -p "$iconset"
  render_1024 "$svg" "$source_png"
  if [ -f "$source_png" ]; then
    for size in 16 32 64 128 256 512; do
      sips -z $size $size "$source_png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
      sips -z $((size * 2)) $((size * 2)) "$source_png" \
        --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$app/Contents/Resources/$name.icns"
    echo "  $name built"
  else
    warn "could not render $(basename "$svg"), so $name will fall back"
    warn "install librsvg (brew install librsvg) and run again to fix it"
  fi
  rm -rf "$iconset" "$source_png"
}

# Either format, a PNG preferred when both are there.
artwork() {
  local base="$root/packaging/linux/$1"
  for candidate in "$base.png" "$base.svg"; do
    [ -f "$candidate" ] && { printf '%s' "$candidate"; return 0; }
  done
  # Explicit, because the loop's last act is a test that failed, and under
  # `set -e` a function returning that status kills the script at the
  # assignment rather than leaving an empty string to be checked.
  return 0
}
icon_src="$(artwork "$bin_name")"
make_icns "$icon_src" AppIcon
# Until the document artwork exists the application icon stands in, so this
# never fails a build; it simply looks like it does today.
doc_src="$(artwork alterion-project-document)"
[ -n "$doc_src" ] || doc_src="$icon_src"
make_icns "$doc_src" DocumentIcon

step "Signing"
if [ -n "${SIGN_ID:-}" ]; then
  # The hardened runtime is what notarisation requires. WebKit runs its own
  # processes outside this bundle, so no extra entitlements are needed.
  codesign --force --deep --options runtime --timestamp \
    --sign "$SIGN_ID" "$app"
  codesign --verify --strict --verbose=2 "$app" 2>&1 | sed 's/^/  /'
  echo "  signed as $SIGN_ID"
else
  # An ad hoc signature is not a substitute for a real one, but an entirely
  # unsigned bundle is refused outright on Apple silicon, so this at least
  # lets the person who built it run it.
  codesign --force --deep --sign - "$app" 2>/dev/null && echo "  signed ad hoc (SIGN_ID was not set)"
fi

step "Building the disk image"
dmg="$dist/AlterionOpenProject-$version.dmg"
rm -f "$dmg"
# A symlink to /Applications is the whole of the install: the user drags the
# bundle across. Nothing is written outside the folder they choose.
ln -s /Applications "$staging/Applications"
hdiutil create -volname "$app_name" -srcfolder "$staging" \
  -ov -format UDZO -quiet "$dmg"
rm -f "$staging/Applications"

if [ -n "${SIGN_ID:-}" ]; then
  codesign --force --sign "$SIGN_ID" "$dmg"
fi

step "Notarising"
if [ -n "${NOTARY_PROFILE:-}" ] && [ -n "${SIGN_ID:-}" ]; then
  # Apple staples its verdict onto the image so Gatekeeper can check it
  # without going online.
  xcrun notarytool submit "$dmg" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$dmg"
  echo "  notarised and stapled"
elif [ -n "${SIGN_ID:-}" ]; then
  warn "signed but not notarised; set NOTARY_PROFILE to do that too"
else
  warn "not signed, so Gatekeeper will refuse it on another Mac"
fi

step "Done"
ls -lh "$dmg"

if [ -z "${NOTARY_PROFILE:-}" ]; then
  cat <<'NOTE'

Opening it the first time
-------------------------
This image is not notarised, so macOS will say the application "cannot be
opened because the developer cannot be verified". That is Gatekeeper refusing
an unknown signature, not a problem with the build.

To open it anyway: right click the application and choose Open, then confirm.
That only has to be done once per machine.

To stop it happening at all, the build needs an Apple Developer account
(99 USD a year), then:

  SIGN_ID="Developer ID Application: Your Name (TEAMID)" \
  NOTARY_PROFILE=alterion \
    packaging/macos/build-dmg.sh

where NOTARY_PROFILE is a keychain profile made once with:

  xcrun notarytool store-credentials alterion \
    --apple-id you@example.com --team-id TEAMID --password APP_SPECIFIC_PASSWORD
NOTE
fi
