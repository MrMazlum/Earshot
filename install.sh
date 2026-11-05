#!/bin/sh
# Earshot one-line installer (Linux x86_64).
#
#   curl -fsSL https://raw.githubusercontent.com/MrMazlum/Earshot/main/install.sh | sh
#
# What it does:
#   1. Downloads the latest Linux release tarball from GitHub releases.
#   2. Installs earshot-receiver + earshot-tray to /usr/local/bin (root)
#      or ~/.local/bin (non-root).
#   3. Installs the app menu entry + icon in the matching prefix.
#
# What it does NOT do (on purpose): it never enables start-at-login.
# Run `earshot-tray --install` yourself, or tick "Start at login" in the
# tray menu. See receiver/src/autostart.rs for why Linux asks instead of
# assuming.
#
# Options (env or args):
#   --prefix DIR     install prefix (default: /usr/local when root, else ~/.local)
#   --bin-dir DIR    override binary dir
#   --uninstall      remove installed files and exit
#   --help           this text
#
# Everything printed here is plain ASCII (project rule: Windows consoles are
# not reliably UTF-8, and install logs get pasted there too).

set -eu

REPO="MrMazlum/Earshot"
BINARIES="earshot-receiver earshot-tray"
TARBALL_URL="https://github.com/${REPO}/releases/latest/download/earshot-receiver-linux-x86_64.tar.gz"

PREFIX="${PREFIX:-}"
BIN_DIR="${BIN_DIR:-}"
UNINSTALL=0

usage() {
  cat <<'EOF'
Earshot one-line installer (Linux x86_64).
  curl -fsSL https://raw.githubusercontent.com/MrMazlum/Earshot/main/install.sh | sh
Options: --prefix DIR, --bin-dir DIR, --uninstall, --help.
Installs earshot-receiver + earshot-tray, a menu entry and an icon.
Never enables start-at-login; run `earshot-tray --install` yourself.
EOF
  exit "${1:-0}"
}

log() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
    --prefix) PREFIX="${2:?--prefix needs a value}"; shift 2 ;;
    --bin-dir=*) BIN_DIR="${1#--bin-dir=}"; shift ;;
    --bin-dir) BIN_DIR="${2:?--bin-dir needs a value}"; shift 2 ;;
    --uninstall) UNINSTALL=1; shift ;;
    -h|--help) usage 0 ;;
    *) die "unknown option '$1' (try --help)" ;;
  esac
done

if [ -z "$PREFIX" ]; then
  if [ "$(id -u)" -eq 0 ]; then
    PREFIX="/usr/local"
  else
    PREFIX="$HOME/.local"
  fi
fi
if [ -z "$BIN_DIR" ]; then
  BIN_DIR="$PREFIX/bin"
fi

APP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/512x512/apps"
DOC_DIR="$PREFIX/share/doc/earshot"

if [ "$UNINSTALL" -eq 1 ]; then
  log "Removing Earshot from $PREFIX ..."
  rm -f "$BIN_DIR/earshot-receiver" "$BIN_DIR/earshot-tray"
  rm -f "$APP_DIR/earshot.desktop"
  rm -f "$ICON_DIR/earshot.png"
  rm -rf "$DOC_DIR"
  log "Done. (Login item, if any, is yours: earshot-tray --uninstall removes it.)"
  exit 0
fi

# Only x86_64 ships a Linux tarball today.
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ;;
  *) die "no prebuilt Linux build for '$ARCH' (only x86_64). Build from source: cd receiver && cargo build --release --locked" ;;
esac

command -v tar >/dev/null 2>&1 || die "need 'tar' to unpack the release"
if command -v curl >/dev/null 2>&1; then
  FETCH="curl -fsSL -o"
elif command -v wget >/dev/null 2>&1; then
  FETCH="wget -qO"
else
  die "need 'curl' or 'wget'"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

log "Downloading $TARBALL_URL ..."
# shellcheck disable=SC2086
$FETCH "$TMP/earshot.tar.gz" "$TARBALL_URL" || die "download failed (is there a release yet? see https://github.com/${REPO}/releases)"

tar -tzf "$TMP/earshot.tar.gz" | head -20 >&2 || die "not a valid tar.gz"
tar -xzf "$TMP/earshot.tar.gz" -C "$TMP"

# Release tarballs wrap binaries in one top-level dir; find them wherever they are.
find_bin() {
  found="$(find "$TMP" -name "$1" -type f | head -1)"
  printf '%s' "$found"
}

RECEIVER="$(find_bin earshot-receiver)"
TRAY="$(find_bin earshot-tray)"
[ -n "$RECEIVER" ] || die "tarball has no earshot-receiver"
[ -n "$TRAY" ] || die "tarball has no earshot-tray"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR" "$DOC_DIR"
install -m 0755 "$RECEIVER" "$BIN_DIR/earshot-receiver"
install -m 0755 "$TRAY" "$BIN_DIR/earshot-tray"

# Desktop entry + icon: prefer the repo files when piped via a checkout,
# else fetch them from main so the curl-pipe still gets a menu entry.
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" 2>/dev/null && pwd || pwd)"
if [ -f "$SCRIPT_DIR/packaging/earshot.desktop" ]; then
  cp "$SCRIPT_DIR/packaging/earshot.desktop" "$APP_DIR/earshot.desktop"
else
  cat > "$APP_DIR/earshot.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Earshot
GenericName=Phone microphone bridge
Comment=Use your phone as this machine's microphone
Exec=earshot-tray
Icon=earshot
Terminal=false
Categories=AudioVideo;Audio;
StartupNotify=false
EOF
fi
if [ -f "$SCRIPT_DIR/docs/icon.png" ]; then
  cp "$SCRIPT_DIR/docs/icon.png" "$ICON_DIR/earshot.png"
else
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$ICON_DIR/earshot.png" "https://raw.githubusercontent.com/${REPO}/main/docs/icon.png" || true
  else
    wget -qO "$ICON_DIR/earshot.png" "https://raw.githubusercontent.com/${REPO}/main/docs/icon.png" || true
  fi
fi

# Refresh caches when present; failures are cosmetic.
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APP_DIR" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f "$PREFIX/share/icons/hicolor" >/dev/null 2>&1 || true

log ""
log "Installed:"
log "  $BIN_DIR/earshot-receiver"
log "  $BIN_DIR/earshot-tray"
log ""
log "Next:"
log "  1. Run: earshot-tray   (the address is in the tray menu)"
log "  2. Optional, start at login: earshot-tray --install"
log "  3. In Discord, pick 'Earshot' as your microphone."
log ""
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) log "NOTE: $BIN_DIR is not on your PATH. Add it or re-login." ;;
esac
