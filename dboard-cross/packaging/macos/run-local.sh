#!/usr/bin/env bash
# Build and reopen a consistently signed local app, preserving its Keychain identity.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
if ! security find-identity -p codesigning | grep -Fq '"dboard Local Development"'; then
  echo "Run packaging/macos/setup-local-signing.sh once before launching local builds." >&2
  exit 1
fi
cargo build --locked -p dboard
LOCAL_APP="$PWD/target/local/dboard.app"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)"
# Quit before replacing a running bundle. macOS invokes the app's quit handler.
if [ -d "$LOCAL_APP" ]; then
  osascript -e 'tell application id "com.alcolopa.dboard" to quit' || true
fi
DBOARD_BINARY=target/debug/dboard MACOS_SIGN_IDENTITY="dboard Local Development" \
  packaging/macos/bundle.sh "$LOCAL_APP" "$VERSION"
open "$LOCAL_APP"
