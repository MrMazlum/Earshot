#!/bin/sh
# Build a .deb from the current source tree.
#
#   packaging/build-deb.sh [--output-dir dist/]
#
# Builds both binaries with cargo (--locked, same flags as CI), then packs
# them with dpkg-deb. No cargo-deb, no root, no network at pack time.
# Version comes from receiver/Cargo.toml so there is one source of truth.
#
# Layout inside the package (FHS, root-owned):
#   /usr/bin/earshot-receiver, /usr/bin/earshot-tray
#   /usr/share/applications/earshot.desktop
#   /usr/share/icons/hicolor/512x512/apps/earshot.png
#   /usr/share/doc/earshot/{copyright,changelog.gz}

set -eu

ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
OUT="$ROOT/dist"
while [ $# -gt 0 ]; do
  case "$1" in
    --output-dir) OUT="$2"; shift 2 ;;
    --output-dir=*) OUT="${1#--output-dir=}"; shift ;;
    -h|--help) echo "usage: packaging/build-deb.sh [--output-dir DIR]"; exit 0 ;;
    *) echo "error: unknown option '$1'" >&2; exit 2 ;;
  esac
done

VER="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$ROOT/receiver/Cargo.toml" | head -1)"
[ -n "$VER" ] || { echo "error: cannot read version from receiver/Cargo.toml" >&2; exit 1; }

echo "Building Earshot $VER (release, locked) ..."
if command -v apt-get >/dev/null 2>&1 && [ ! -f /usr/include/asoundlib.h ] && [ "$(id -u)" -eq 0 ]; then
  apt-get update && apt-get install -y libasound2-dev libdbus-1-dev pkg-config cmake 2>/dev/null || true
fi
(cd "$ROOT/receiver" && cargo build --release --locked)

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT INT TERM
PKG="$STAGE/earshot_${VER}_amd64"
mkdir -p "$PKG/DEBIAN" "$PKG/usr/bin" "$PKG/usr/share/applications" \
         "$PKG/usr/share/icons/hicolor/512x512/apps" "$PKG/usr/share/doc/earshot"

cp "$ROOT/receiver/target/release/earshot-receiver" "$PKG/usr/bin/"
cp "$ROOT/receiver/target/release/earshot-tray" "$PKG/usr/bin/"
cp "$ROOT/packaging/earshot.desktop" "$PKG/usr/share/applications/"
cp "$ROOT/docs/icon.png" "$PKG/usr/share/icons/hicolor/512x512/apps/earshot.png"
cp "$ROOT/LICENSE" "$PKG/usr/share/doc/earshot/copyright"
cp "$ROOT/README.md" "$PKG/usr/share/doc/earshot/README.md" 2>/dev/null || true
gzip -9n "$PKG/usr/share/doc/earshot/README.md" 2>/dev/null || true
if [ -f "$ROOT/CHANGELOG.md" ]; then
  cp "$ROOT/CHANGELOG.md" "$PKG/usr/share/doc/earshot/changelog"
  gzip -9n "$PKG/usr/share/doc/earshot/changelog"
fi

cat > "$PKG/DEBIAN/control" <<EOF
Package: earshot
Version: $VER
Section: sound
Priority: optional
Architecture: amd64
Depends: libasound2t64 | libasound2, libdbus-1-3, pulseaudio-utils | pipewire-pulse
Recommends: gnome-shell-extension-appindicator
Maintainer: Mazlum <https://github.com/MrMazlum/Earshot>
Homepage: https://github.com/MrMazlum/Earshot
Description: Use your phone as your PC's microphone, over Wi-Fi
 Free, open source, no ads, no account, no cloud. Your phone sends
 microphone audio over Wi-Fi; this PC receives it and exposes it as an
 ordinary microphone (a PipeWire/PulseAudio source named "Earshot").
 Includes earshot-receiver (terminal) and earshot-tray (system tray,
 start-at-login via earshot-tray --install).
EOF

cat > "$PKG/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database /usr/share/applications >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f /usr/share/icons/hicolor >/dev/null 2>&1 || true
exit 0
EOF
chmod 0755 "$PKG/DEBIAN/postinst"
printf '#!/bin/sh\nset -e\n# nothing to do\n' > "$PKG/DEBIAN/postrm"
chmod 0755 "$PKG/DEBIAN/postrm"

mkdir -p "$OUT"
dpkg-deb --build --root-owner-group "$PKG" "$OUT/earshot_${VER}_amd64.deb"
echo "Wrote $OUT/earshot_${VER}_amd64.deb"
dpkg-deb -c "$OUT/earshot_${VER}_amd64.deb" | head -20
