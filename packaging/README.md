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

## macOS

```
packaging/macos/build-dmg.sh
```

It produces `packaging/macos/dist/AlterionOpenProject-<version>.dmg`, holding a
universal `Alterion Open Project.app` (Apple silicon and Intel in one binary,
joined with `lipo`) beside a link to Applications, so installing is a drag.
`.aprj` is registered through the bundle's `Info.plist`, the same association
the Linux MIME file and the Windows installer set up.

**This has to run on a Mac.** Apple's SDK may not be redistributed, so unlike
Windows there is no cross-compiling to it from Linux. Everything the script
uses beyond Rust (`hdiutil`, `iconutil`, `sips`, `lipo`, `codesign`) ships with
macOS and the Xcode command line tools:

```
xcode-select --install
```

Nothing needs to be installed on the machine the app is *used* on. The window
is a WKWebView, which is part of macOS, so there is no runtime to bundle the
way Windows needs WebView2.

### Signing

Signing is optional and off by default, and the build works without it. An
unsigned image still opens, but the first launch on any other Mac is refused by
Gatekeeper with "the developer cannot be verified"; right clicking the
application and choosing Open gets past it, once per machine.

To avoid that entirely you need an Apple Developer account, then:

```
SIGN_ID="Developer ID Application: Your Name (TEAMID)" \
NOTARY_PROFILE=alterion \
  packaging/macos/build-dmg.sh
```

Set the keychain profile up once with:

```
xcrun notarytool store-credentials alterion \
  --apple-id you@example.com --team-id TEAMID --password APP_SPECIFIC_PASSWORD
```

Signing uses the hardened runtime, which notarisation requires. No extra
entitlements are needed: WebKit runs its own processes outside the bundle.

## Shared assets

`linux/` holds the desktop entry, the scalable icon and the MIME definition that
teaches the desktop about `.aprj` (matched on its `APRJ` magic bytes, not just
the extension). The Windows icon is generated from the same SVG.
