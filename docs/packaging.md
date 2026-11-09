# Packaging

## macOS
```sh
sh tools/bundle_macos.sh            # → dist/oto-albums.app + dist/oto-albums-<ver>-macos-arm64.zip
INSTALL=1 sh tools/bundle_macos.sh  # also copies to ~/Applications and registers .otoalbum
```
The script builds in release mode, makes the icon, writes Info.plist (bundle id `com.johorstudio.oto-albums`; `.otoalbum` is a package UTI) and signs ad-hoc. For distribution outside your own Macs, sign with a Developer ID and notarise (not done in v0.1).

## Windows (not built yet)
`cargo build --release -p oto-app` with the MSVC toolchain. Still to do: a Per-Monitor-V2 DPI manifest, a per-user installer (WiX/MSIX), signing (otherwise document the SmartScreen prompt) and the HKCU file association (code exists in `oto-platform`).
