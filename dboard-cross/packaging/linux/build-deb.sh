#!/usr/bin/env bash
# Build a .deb. Run from dboard-cross/ after `cargo build --release -p dboard`.  Usage: build-deb.sh [version] [amd64|arm64]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
VERSION="${1:-2.0.1}"; VERSION="${VERSION#v}"
ARCH="${2:-amd64}"
ROOT="dist/deb/dboard_${VERSION}_${ARCH}"
rm -rf "$ROOT"; mkdir -p "$ROOT/DEBIAN" "$ROOT/usr/bin" "$ROOT/usr/share/applications" "$ROOT/usr/share/icons/hicolor/256x256/apps"
install -m755 target/release/dboard "$ROOT/usr/bin/dboard"
install -m644 packaging/linux/dboard.desktop "$ROOT/usr/share/applications/dboard.desktop"
install -m644 assets/icon.png "$ROOT/usr/share/icons/hicolor/256x256/apps/dboard.png"
cat > "$ROOT/DEBIAN/control" <<CTRL
Package: dboard
Version: $VERSION
Section: database
Priority: optional
Architecture: $ARCH
Maintainer: alcolopa <noreply@github.com>
Homepage: https://github.com/alcolopa/dboard
Description: Native database client for PostgreSQL, MySQL and MongoDB
 Fast desktop client with instant cell editing and production-safety guards.
CTRL
mkdir -p dist
dpkg-deb --build --root-owner-group "$ROOT" "dist/dboard-${VERSION}-linux-${ARCH}.deb"
