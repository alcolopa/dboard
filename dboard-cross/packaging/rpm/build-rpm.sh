#!/usr/bin/env bash
# Build a .rpm (Fedora, RHEL, openSUSE). Run from dboard-cross/ after `cargo build --release -p dboard`.  Usage: build-rpm.sh [version]
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
VERSION="${1:-0.1.0}"; VERSION="${VERSION#v}"
TOP="$PWD/dist/rpmbuild"; rm -rf "$TOP"; mkdir -p "$TOP/SOURCES" dist
cp target/release/dboard packaging/linux/dboard.desktop "$TOP/SOURCES/"
cp assets/icon.png "$TOP/SOURCES/dboard.png"
rpmbuild -bb --define "_topdir $TOP" --define "pkg_version $VERSION" packaging/rpm/dboard.spec
cp "$TOP"/RPMS/x86_64/dboard-*.rpm dist/
