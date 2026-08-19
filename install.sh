#!/usr/bin/env bash
# Install Alterion Open Project on Linux.
#
#   ./install.sh              install for the current user
#   ./install.sh --system     install for everyone (needs sudo)
#   ./install.sh --build      build from source instead of downloading
#   ./install.sh --uninstall  remove it again
#
# On Arch this offers to build a real package instead, so pacman owns the
# files and can remove them cleanly. Everywhere else it drops a prebuilt
# binary into place along with the desktop entry, icon and file association.

set -euo pipefail

repo="Alterion-Software/Alterion-Open-Project"
version="v1.0.4-beta"
name="alterion-open-project"
pretty="Alterion Open Project"

mode=user
action=install
from_source=0

for arg in "$@"; do
  case "$arg" in
    --system)    mode=system ;;
    --user)      mode=user ;;
    --build)     from_source=1 ;;
    --uninstall) action=uninstall ;;
    -h|--help)   sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *)           echo "unknown option: $arg" >&2; exit 1 ;;
  esac
done

step() { printf '\n\033[36m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[33mwarning:\033[0m %s\n' "$1" >&2; }
die()  { printf '\033[31merror:\033[0m %s\n' "$1" >&2; exit 1; }

# Where things go. A user install needs no root and is undone by deleting a
# handful of files; a system install is the one to use when several people
# share the machine.
# Run one command as root, but only when it is actually needed.
#
# The whole script is deliberately not re-run under sudo: it would build as
# root and leave root owned files scattered through the user's checkout. The
# binary is produced as the person running it, and only the placement of files
# under /usr asks for a password.
as_root() {
  # A user install writes only inside the home directory, so asking for a
  # password there would be both pointless and alarming.
  if [ "$mode" != system ] || [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v sudo >/dev/null; then
    sudo -- "$@"
  elif command -v pkexec >/dev/null; then
    pkexec "$@"
  else
    die "installing for all users needs root, and neither sudo nor pkexec is installed"
  fi
}

if [ "$mode" = system ]; then
  prefix=/usr/local
else
  prefix="${XDG_DATA_HOME:-$HOME/.local}"
  prefix="${prefix%/share}"
  [ "$prefix" = "$HOME/.local" ] || prefix="$HOME/.local"
fi
bindir="$prefix/bin"
sharedir="$prefix/share"

# ---------------------------------------------------------------- uninstall

if [ "$action" = uninstall ]; then
  step "Removing $pretty"
  as_root rm -fv "$bindir/$name" \
         "$sharedir/applications/$name.desktop" \
         "$sharedir/icons/hicolor/scalable/apps/$name.svg" \
         "$sharedir/mime/packages/$name.xml" 2>/dev/null || true
  command -v update-desktop-database >/dev/null && as_root update-desktop-database "$sharedir/applications" 2>/dev/null || true
  command -v update-mime-database    >/dev/null && as_root update-mime-database "$sharedir/mime" 2>/dev/null || true
  echo
  echo "Removed. Your plans and settings were left alone:"
  echo "  ${XDG_CONFIG_HOME:-$HOME/.config}/$name/"
  exit 0
fi

# ------------------------------------------------------------ arch shortcut

if [ "$from_source" -eq 0 ] && command -v pacman >/dev/null && command -v makepkg >/dev/null; then
  cat <<MSG

This looks like an Arch based system. Building the package instead means
pacman owns the files and can remove them cleanly later:

    cd packaging/aur && makepkg -si

That is the better route once the AUR listing is up. Carrying on with the
plain install; pass --build to compile from source instead.
MSG
fi

# ------------------------------------------------------------- dependencies

step "Checking what the application needs"
missing=()
# The window is a WebKitGTK view, so those libraries have to be present. They
# are checked by asking the loader, which is the same question the binary will
# ask when it starts.
#
# The cache is read once into a variable rather than piped per library. Piping
# it into `grep -q` looks tidier but is wrong here: grep exits at the first
# match and closes the pipe, ldconfig dies of SIGPIPE, and with `pipefail` the
# whole pipeline then reports failure, so a library that is present reads as
# missing. A substring match on the text avoids the pipe and the regex both.
cache="$(ldconfig -p 2>/dev/null || true)"
for lib in libgtk-3.so.0 libwebkit2gtk-4.1.so.0 libxdo.so.4; do
  case "$cache" in
    *"$lib"*) ;;
    *) missing+=("$lib") ;;
  esac
done

if [ ${#missing[@]} -gt 0 ]; then
  warn "these shared libraries are not installed: ${missing[*]}"
  echo "  Arch      sudo pacman -S gtk3 webkit2gtk-4.1 xdotool"
  echo "  Debian    sudo apt install libgtk-3-0 libwebkit2gtk-4.1-0 libxdo3"
  echo "  Fedora    sudo dnf install gtk3 webkit2gtk4.1 xdotool-libs"
  echo
  echo "  Install them first, or the application will not start."
  printf "  Carry on anyway? [y/N] "
  read -r reply
  [ "$reply" = y ] || [ "$reply" = Y ] || exit 1
else
  echo "  everything it needs is present"
fi

# ------------------------------------------------------------------- binary

place() { as_root install -Dm"$1" "$2" "$3"; }
as_root mkdir -p "$bindir" "$sharedir/applications" \
         "$sharedir/icons/hicolor/scalable/apps" "$sharedir/mime/packages"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ "$from_source" -eq 1 ]; then
  step "Building from source"
  command -v cargo >/dev/null || die "cargo not found; install Rust from https://rustup.rs"
  ( cd "$here" && cargo build --release --package aop-app )
  place 755 "$here/target/release/$name" "$bindir/$name"
else
  step "Downloading"
  arch="$(uname -m)"
  [ "$arch" = "x86_64" ] || die "no prebuilt binary for $arch; rerun with --build to compile it"

  # The name has to match what packaging/release.sh publishes, exactly. It is
  # a versioned tarball carrying the binary and the desktop files together,
  # not a bare binary.
  bare="${version#v}"
  archive="$name-$bare-x86_64-linux.tar.gz"
  base="https://github.com/$repo/releases/download/$version"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  fetch() {
    if command -v curl >/dev/null; then
      curl -fL --progress-bar -o "$2" "$1"
    elif command -v wget >/dev/null; then
      wget -q --show-progress -O "$2" "$1"
    else
      die "neither curl nor wget is installed"
    fi
  }

  fetch "$base/$archive" "$tmp/$archive"
  [ -s "$tmp/$archive" ] || die "the download was empty"

  # Verify before unpacking, not after. SHA256SUMS is published beside the
  # artefacts; this proves the download matches what was published. It does
  # not prove who published it, which is what HTTPS to github.com is for.
  step "Verifying the download"
  if fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null && [ -s "$tmp/SHA256SUMS" ]; then
    if command -v sha256sum >/dev/null; then
      ( cd "$tmp" && grep " $archive\$" SHA256SUMS | sha256sum -c - ) \
        || die "checksum mismatch; the download is corrupt or has been tampered with"
      echo "  checksum matches the published SHA256SUMS"
    else
      warn "sha256sum is not installed, so the download could not be verified"
    fi
  else
    warn "SHA256SUMS could not be fetched, so the download could not be verified"
  fi

  step "Unpacking"
  tar -xzf "$tmp/$archive" -C "$tmp"
  [ -f "$tmp/$name" ] || die "the archive did not contain $name"
  head -c 4 "$tmp/$name" | grep -q "ELF" || die "that is not a Linux binary; the download may have failed"
  place 755 "$tmp/$name" "$bindir/$name"
  # The tarball carries the desktop files too, which matters when this script
  # was downloaded on its own rather than as part of a checkout.
  downloaded="$tmp"
fi
echo "  installed $bindir/$name"

# ------------------------------------------------------- desktop integration

step "Registering with the desktop"
# A checkout has these beside the script; a plain download has them only
# because the tarball carried them.
assets=""
[ -d "$here/packaging/linux" ] && assets="$here/packaging/linux"
[ -z "$assets" ] && [ -n "${downloaded:-}" ] && [ -f "$downloaded/$name.desktop" ] && assets="$downloaded"

if [ -n "$assets" ]; then
  place 644 "$assets/$name.desktop" "$sharedir/applications/$name.desktop"
  place 644 "$assets/$name.svg"     "$sharedir/icons/hicolor/scalable/apps/$name.svg"
  place 644 "$assets/$name.xml"     "$sharedir/mime/packages/$name.xml"
  # The document icon, when there is one. Named after the MIME type rather
  # than after the application, which is how the desktop finds it, and why a
  # plan in a folder can look like a document rather than like a second copy
  # of the application.
  if [ -f "$assets/alterion-project-document.svg" ]; then
    place 644 "$assets/alterion-project-document.svg" \
      "$sharedir/icons/hicolor/scalable/mimetypes/application-x-alterion-project.svg"
    echo "  desktop entry, application icon, document icon and .aprj association installed"
  else
    echo "  desktop entry, icon and .aprj association installed"
  fi
else
  warn "packaging/linux was not found beside this script, so only the binary was installed"
  warn "the application will still run; it just will not appear in your menu"
fi

# These are what put the application in the menu, give .aprj its icon and make
# a double click open it here. Without them the files are on disk and the
# desktop does not know about any of them.
command -v update-desktop-database >/dev/null && as_root update-desktop-database "$sharedir/applications" 2>/dev/null || true
command -v update-mime-database    >/dev/null && as_root update-mime-database "$sharedir/mime" 2>/dev/null || true
command -v gtk-update-icon-cache   >/dev/null && as_root gtk-update-icon-cache -qtf "$sharedir/icons/hicolor" 2>/dev/null || true

# ------------------------------------------------------------------- finish

step "Done"
echo "  $pretty $version is installed."
echo
case ":$PATH:" in
  *":$bindir:"*)
    echo "  Run it with:  $name"
    ;;
  *)
    # Saying this plainly is better than the command silently not being found.
    warn "$bindir is not on your PATH, so the command will not be found yet"
    echo "  Add it with:"
    echo "      echo 'export PATH=\"\$PATH:$bindir\"' >> ~/.bashrc"
    echo "  or launch it from your application menu, which works either way."
    ;;
esac
echo
echo "  Uninstall with:  ./install.sh${mode:+ --$mode} --uninstall"
