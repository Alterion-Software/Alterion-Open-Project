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

Prerequisites on Arch:

```
pacman -S nsis mingw-w64-gcc imagemagick
rustup target add x86_64-pc-windows-gnu
```

## Shared assets

`linux/` holds the desktop entry, the scalable icon and the MIME definition that
teaches the desktop about `.aprj` (matched on its `APRJ` magic bytes, not just
the extension). The Windows icon is generated from the same SVG.
