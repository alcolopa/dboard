#!/usr/bin/env bash
# Wrap the release binary in a dboard.app bundle. Run from dboard-cross/ after `cargo build --release -p dboard`.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
APP="${1:-dboard.app}"
VERSION="${2:-0.1.0}"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/dboard "$APP/Contents/MacOS/dboard"
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>dboard</string>
  <key>CFBundleDisplayName</key><string>dboard</string>
  <key>CFBundleIdentifier</key><string>com.alcolopa.dboard</string>
  <key>CFBundleExecutable</key><string>dboard</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION#v}</string>
  <key>CFBundleVersion</key><string>${VERSION#v}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
# Ad-hoc sign the whole bundle (no Developer ID needed). Without a sealed signature, Apple Silicon
# Macs report a downloaded app as "damaged and can't be opened". Gatekeeper still warns (not notarized).
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  # Developer ID signing with the hardened runtime (required for notarization).
  codesign --force --deep --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
  codesign --force --deep --sign - "$APP"
fi
echo "Built $APP"
