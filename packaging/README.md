# Packaging

Two routes, deliberately different.

## Linux, via the AUR

No installer. `makepkg` builds from source and `pacman` owns the files, which is
what Arch users expect. See `aur/README.md`.

```
cd packaging/aur && makepkg -si
```

## Windows, via an installer

An NSIS installer that cross-builds from Linux, so releases can be cut from the
same machine.

```
packaging/windows/build-installer.sh
```

It produces `packaging/windows/dist/AlterionOpenProject-<version>-setup.exe`,
which installs the binary, adds Start menu and desktop shortcuts, registers
`.aprj` so plans open on double-click, and writes an Add or Remove Programs
entry with a working uninstaller.

### What the machine being installed onto needs

Nothing that has to be installed by hand, and in particular **not Rust**: the
installer carries a compiled binary, and the Rust toolchain is only ever needed
on the machine doing the building.

There are two runtime pieces, and the installer deals with both:

- **The WebView2 runtime.** The window is a WebView2 control, so without it the
  application starts and shows no window. Windows 11 ships it, and most updated
  Windows 10 machines have it through Edge, but a clean machine may not. The
  build script downloads Microsoft's bootstrapper (about two megabytes) and the
  installer runs it only when the runtime is genuinely absent, checking all
  three registry locations it can be recorded in. If the download failed at
  build time, the installer says what is missing and where to get it rather
  than leaving the user with a window that never appears.
- **MinGW runtime libraries.** A binary built against the GNU toolchain can
  want a few of these beside it. The build script reads the actual list out of
  the compiled binary with `objdump` rather than guessing, and copies only
  those. Often there are none, because Rust links most of it statically.

Prerequisites on Arch:

```
pacman -S nsis mingw-w64-gcc imagemagick
rustup target add x86_64-pc-windows-gnu
```

## Shared assets

`linux/` holds the desktop entry, the scalable icon and the MIME definition that
teaches the desktop about `.aprj` (matched on its `APRJ` magic bytes, not just
the extension). The Windows icon is generated from the same SVG.
