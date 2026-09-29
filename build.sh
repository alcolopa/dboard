#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$DIR"

echo "🔨 Building dboard (Native macOS Database Client)..."

export DEVELOPER_DIR="/Applications/Xcode.app/Contents/Developer"
SDK_PATH="/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"

mkdir -p ./build-cache/swift-module-cache ./build-cache/clang-module-cache
mkdir -p dboard.app/Contents/MacOS dboard.app/Contents/Resources

cat << 'EOF' > dboard.app/Contents/Info.plist
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>dboard</string>
    <key>CFBundleIdentifier</key>
    <string>com.personal.dboard</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>dboard</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>1.0.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

cp AppIcon.icns dboard.app/Contents/Resources/AppIcon.icns


swiftc \
  -sdk "$SDK_PATH" \
  -target arm64-apple-macos14.0 \
  -module-cache-path ./build-cache/swift-module-cache \
  -Xcc -fmodules-cache-path=./build-cache/clang-module-cache \
  -parse-as-library \
  -O \
  -o dboard.app/Contents/MacOS/dboard \
  dboard/Core/*.swift \
  dboard/Security/*.swift \
  dboard/Drivers/*.swift \
  dboard/Managers/*.swift \
  dboard/UI/Components/*.swift \
  dboard/UI/Navigation/*.swift \
  dboard/UI/TableData/*.swift \
  dboard/UI/QueryEditor/*.swift \
  dboard/UI/MongoDB/*.swift \
  dboard/UI/Schema/*.swift \
  dboard/UI/Inspector/*.swift \
  dboard/UI/Connections/*.swift \
  dboard/UI/Settings/*.swift \
  dboard/ContentView.swift \
  dboard/MyApp.swift

echo "✅ Build Successful: dboard.app"

if [ "$1" == "run" ]; then
  echo "🚀 Launching dboard.app..."
  open dboard.app
fi
