# AUR packaging

The AUR package builds from source with cargo and installs straight into the
system. There is no installer on Linux: `makepkg` and `pacman` do that job.

```
cd packaging/aur
makepkg -si
```

`sha256sums` is `SKIP` until there is a tagged release to hash. Once a tag
exists, replace it with the real digest:

```
makepkg -g >> PKGBUILD
```

Installed files:

| Path | What |
| --- | --- |
| `/usr/bin/alterion-open-project` | the binary |
| `/usr/share/applications/…desktop` | menu entry, opens `.aprj` files |
| `/usr/share/icons/hicolor/scalable/apps/…svg` | the icon |
| `/usr/share/mime/packages/…xml` | registers `.aprj`, matched on its `APRJ` magic |

`webkit2gtk-4.1` is a dependency because the default build renders through a
webview. A webview-free build exists behind the `native` feature (Blitz, which
paints with wgpu); if that becomes the shipped build, the dependency list
changes to the graphics stack instead.
