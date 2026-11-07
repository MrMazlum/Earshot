#!/bin/sh
# Build an AppImage from the current source tree.
#
#   packaging/build-appimage.sh [--output-dir dist/]
#
# Builds both binaries with cargo (--locked, same flags as CI), assembles an
# AppDir, and runs appimagetool. Downloads appimagetool if missing.
# Version comes from receiver/Cargo.toml.

set -eu
ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
OUT="$ROOT/dist"
while [ $# -gt 0 ]; do
  case "$1" in
    --output-dir) OUT="$2"; shift 2 ;;
    --output-dir=*) OUT="${1#--output-dir=}"; shift ;;
    -h|--help) echo "usage: packaging/build-appimage.sh [--output-dir DIR]"; exit 0 ;;
    *) echo "error: unknown option '$1'" >&2; exit 2 ;;
  esac
done

VER="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$ROOT/receiver/Cargo.toml" | head -1)"
[ -n "$VER" ] || { echo "error: cannot read version" >&2; exit 1; }

echo "Building Earshot $VER (release, locked) ..."
(cd "$ROOT/receiver" && cargo build --release --locked)

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM
APPDIR="$WORK/Earshot.AppDir"
mkdir -p "$APPDIR/usr/bin" "$OUT"

cp "$ROOT/receiver/target/release/earshot-receiver" "$APPDIR/usr/bin/"
cp "$ROOT/receiver/target/release/earshot-tray" "$APPDIR/usr/bin/"
cp "$ROOT/packaging/AppRun" "$APPDIR/AppRun"
chmod 0755 "$APPDIR/AppRun"
cp "$ROOT/packaging/earshot.desktop" "$APPDIR/earshot.desktop"
cp "$ROOT/docs/icon.png" "$APPDIR/earshot.png"
ln -sf earshot.desktop "$APPDIR/.DirIcon" 2>/dev/null || cp "$ROOT/packaging/earshot.desktop" "$APPDIR/.DirIcon"

if ! command -v appimagetool >/dev/null 2>&1; then
  echo "Fetching appimagetool ..."
  curl -fsSL -o "$WORK/appimagetool" \
    https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$WORK/appimagetool"
  APPIMAGETOOL="$WORK/appimagetool"
else
  APPIMAGETOOL="appimagetool"
fi

(cd "$WORK" && ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "Earshot-${VER}-x86_64.AppImage")
cp "$WORK"/Earshot-*.AppImage "$OUT/"
echo "Wrote:"; ls -la "$OUT"/*.AppImage
