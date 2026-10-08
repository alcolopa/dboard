#!/usr/bin/env bash
# Build a .rpm (Fedora, RHEL, openSUSE). Run from dboard-cross/ after `cargo build --release -p dboard`.  Usage: build-rpm.sh [version] [x86_64|aarch64]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
VERSION="${1:-2.1.0}"; VERSION="${VERSION#v}"
ARCH="${2:-x86_64}"
TOP="$PWD/dist/rpmbuild"; rm -rf "$TOP"; mkdir -p "$TOP/SOURCES" dist
cp target/release/dboard packaging/linux/dboard.desktop "$TOP/SOURCES/"
cp assets/icon.png "$TOP/SOURCES/dboard.png"
rpmbuild -bb --define "_topdir $TOP" --define "pkg_version $VERSION" --target "$ARCH-linux" packaging/rpm/dboard.spec
cp "$TOP"/RPMS/$ARCH/dboard-*.rpm dist/
