#!/usr/bin/env bash
# Wrap the release binary in a dboard.app bundle. Run from dboard-cross/ after `cargo build --release -p dboard`.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
APP="${1:-dboard.app}"
VERSION="${2:-0.1.0}"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "${DBOARD_BINARY:-target/release/dboard}" "$APP/Contents/MacOS/dboard"
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
# Sign the whole bundle (no Developer ID needed). Without a sealed signature, Apple Silicon
# Macs report a downloaded app as "damaged and can't be opened". Gatekeeper still warns (not notarized).
# Local builds automatically reuse the dedicated identity once it is installed.
if [ -z "${MACOS_SIGN_IDENTITY:-}" ] && security find-identity -p codesigning | grep -Fq '"dboard Local Development"'; then
  MACOS_SIGN_IDENTITY="dboard Local Development"
fi
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  # Reuse the same identity across releases so Keychain recognizes updated builds.
  # Self-signed certificates work without a Developer account. Only Developer ID
  # releases intended for notarization need Apple's secure timestamp service.
  timestamp_flag="--timestamp=none"
  if [ "${MACOS_SIGN_TIMESTAMP:-false}" = "true" ]; then
    timestamp_flag="--timestamp"
  fi
  codesign --force --deep --options runtime "$timestamp_flag" --sign "$MACOS_SIGN_IDENTITY" "$APP"
else
  # Ad-hoc signatures default to a requirement pinned to the binary hash, so every release looks like
  # a different app to Keychain and macOS asks for the login password again after each update.
  # Pin the designated requirement to the bundle identifier instead; it is stable across builds.
  codesign --force --deep --sign - -r='designated => identifier "com.alcolopa.dboard"' "$APP"
fi
codesign --verify --deep --strict "$APP"
echo "Built $APP"
