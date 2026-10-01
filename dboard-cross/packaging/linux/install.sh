#!/usr/bin/env sh
# User-level installer for the tar.gz release (no root needed). Use --system for /usr/local, --uninstall to remove.
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ "$(id -u)" = 0 ] || [ "$1" = "--system" ]; then PREFIX=/usr/local; else PREFIX="$HOME/.local"; fi
BIN="$PREFIX/bin/dboard"; DESK="$PREFIX/share/applications/dboard.desktop"; ICON="$PREFIX/share/icons/hicolor/256x256/apps/dboard.png"
if [ "$1" = "--uninstall" ]; then rm -f "$BIN" "$DESK" "$ICON"; echo "dboard removed from $PREFIX"; exit 0; fi
mkdir -p "$PREFIX/bin" "$PREFIX/share/applications" "$(dirname "$ICON")"
install -m755 "$HERE/dboard" "$BIN"
install -m644 "$HERE/dboard.png" "$ICON"
sed "s|^Exec=.*|Exec=$BIN|" "$HERE/dboard.desktop" > "$DESK"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" || true
echo "dboard installed to $PREFIX (make sure $PREFIX/bin is on your PATH)"
