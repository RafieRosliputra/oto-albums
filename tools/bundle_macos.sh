#!/bin/sh
# Build oto-albums.app (release) and a portable zip. No admin rights needed.
#   sh tools/bundle_macos.sh            -> dist/oto-albums.app + dist/oto-albums-macos-arm64.zip
#   INSTALL=1 sh tools/bundle_macos.sh  -> also copies the app to ~/Applications
set -e
cd "$(dirname "$0")/.."
. "$HOME/.cargo/env" 2>/dev/null || true
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release -p oto-app
APP=dist/oto-albums.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/oto-albums "$APP/Contents/MacOS/oto-albums"

# Icon: render the original oto-albums mark at all sizes, then iconutil.
PY=${PYTHON:-python3}
ICONSET=$(mktemp -d)/oto.iconset
mkdir -p "$ICONSET"
$PY tools/make_icon.py "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/oto-albums.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>oto-albums</string>
  <key>CFBundleDisplayName</key><string>oto-albums</string>
  <key>CFBundleIdentifier</key><string>com.johorstudio.oto-albums</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>oto-albums</string>
  <key>CFBundleIconFile</key><string>oto-albums</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>oto-albums project</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSHandlerRank</key><string>Owner</string>
      <key>LSTypeIsPackage</key><true/>
      <key>LSItemContentTypes</key><array><string>com.johorstudio.otoalbum</string></array>
      <key>CFBundleTypeIconFile</key><string>oto-albums</string>
    </dict>
  </array>
  <key>UTExportedTypeDeclarations</key>
  <array>
    <dict>
      <key>UTTypeIdentifier</key><string>com.johorstudio.otoalbum</string>
      <key>UTTypeDescription</key><string>oto-albums project</string>
      <key>UTTypeConformsTo</key><array><string>com.apple.package</string></array>
      <key>UTTypeTagSpecification</key><dict><key>public.filename-extension</key><array><string>otoalbum</string></array></dict>
    </dict>
  </array>
</dict>
</plist>
PLIST

# Ad-hoc signature (required on Apple Silicon). A Developer ID signature + notarization needs
# Johor Studio's Apple developer certificate; see docs/packaging.md.
codesign --force --deep --sign - "$APP"
(cd dist && rm -f "oto-albums-$VERSION-macos-arm64.zip" && ditto -c -k --keepParent oto-albums.app "oto-albums-$VERSION-macos-arm64.zip")
if [ "${INSTALL:-0}" = "1" ]; then
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/oto-albums.app"
  cp -R "$APP" "$HOME/Applications/"
  /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$HOME/Applications/oto-albums.app" || true
  echo "Installed to ~/Applications/oto-albums.app"
fi
echo "Built $APP"
