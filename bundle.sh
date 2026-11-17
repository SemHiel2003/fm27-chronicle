#!/bin/bash
# Bouwt "FM27 Chronicle.app" in ./dist (release, ad-hoc ondertekend: voor je eigen Mac).
set -euo pipefail
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p fm27-chronicle
# de module voor in FM27: universeel (arm64 + x86_64), want FM27 is universeel
cargo build --release -p fm27-agent --target aarch64-apple-darwin
cargo build --release -p fm27-agent --target x86_64-apple-darwin
lipo -create -output target/release/libfm27_agent.dylib \
  target/aarch64-apple-darwin/release/libfm27_agent.dylib \
  target/x86_64-apple-darwin/release/libfm27_agent.dylib
codesign --force --sign - target/release/libfm27_agent.dylib
VERSION=$(grep -m1 '^version' app/Cargo.toml | cut -d'"' -f2)
APP="dist/FM27 Chronicle.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/fm27-chronicle "$APP/Contents/MacOS/FM27 Chronicle"
cp target/release/libfm27_agent.dylib "$APP/Contents/MacOS/libfm27_agent.dylib"
cp app/assets/icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>FM27 Chronicle</string>
  <key>CFBundleDisplayName</key><string>FM27 Chronicle</string>
  <key>CFBundleIdentifier</key><string>app.fm27chronicle</string>
  <key>CFBundleExecutable</key><string>FM27 Chronicle</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.sports</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
xattr -cr "$APP"
codesign --force --deep --sign - "$APP"
echo "Klaar: $APP"
