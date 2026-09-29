#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "🔨 Building dboard (Native macOS Database Client)..."

# Ensure developer directory is configured
if [ -z "$DEVELOPER_DIR" ] && [ -d "/Applications/Xcode.app/Contents/Developer" ]; then
  export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
fi

BUILD_DIR="./build-output"
rm -rf "$BUILD_DIR" dboard.app

xcodebuild \
  -project dboard.xcodeproj \
  -scheme dboard \
  -configuration Release \
  -destination 'generic/platform=macOS' \
  -derivedDataPath "$BUILD_DIR" \
  CODE_SIGN_IDENTITY="" \
  CODE_SIGNING_REQUIRED=NO \
  CODE_SIGN_ENTITLEMENTS="" \
  CODE_SIGNING_ALLOWED=NO \
  build

# Copy built application to root for standalone packaging/launching
cp -R "$BUILD_DIR/Build/Products/Release/dboard.app" ./dboard.app
rm -rf "$BUILD_DIR"

echo "✅ Build Successful: dboard.app"

if [ "$1" == "run" ]; then
  echo "🚀 Launching dboard.app..."
  open dboard.app
fi
