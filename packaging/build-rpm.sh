#!/bin/sh
# Build an RPM from the current source tree.
#   packaging/build-rpm.sh [--output-dir dist/]
# Syncs Version: in the spec from receiver/Cargo.toml first, then builds
# via rpmbuild. Works without mock/copr for a local artifact.

set -eu
ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
OUT="$ROOT/dist"
while [ $# -gt 0 ]; do
  case "$1" in
    --output-dir) OUT="$2"; shift 2 ;;
    --output-dir=*) OUT="${1#--output-dir=}"; shift ;;
    -h|--help) echo "usage: packaging/build-rpm.sh [--output-dir DIR]"; exit 0 ;;
    *) echo "error: unknown option '$1'" >&2; exit 2 ;;
  esac
done

VER="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$ROOT/receiver/Cargo.toml" | head -1)"
[ -n "$VER" ] || { echo "error: cannot read version" >&2; exit 1; }

# Keep the spec honest: it must carry the same version as Cargo.toml.
if grep -q "^Version:" "$ROOT/packaging/earshot.spec"; then
  sed -i "s/^Version:.*/Version:        $VER/" "$ROOT/packaging/earshot.spec"
fi

TOP="$HOME/rpmbuild"
mkdir -p "$TOP/SOURCES" "$TOP/SPECS" "$OUT"
TARBALL="$TOP/SOURCES/earshot-$VER.tar.gz"
(cd "$ROOT" && git archive --format=tar.gz --prefix="earshot-$VER/" HEAD > "$TARBALL") || \
  (cd "$ROOT" && tar --exclude=.git --exclude=target --exclude=dist -czf "$TARBALL" .)
cp "$ROOT/packaging/earshot.spec" "$TOP/SPECS/"

rpmbuild -bb "$TOP/SPECS/earshot.spec"
find "$TOP/RPMS" -name "earshot-$VER-*.rpm" -exec cp -v {} "$OUT/" \;
echo "Wrote:"; ls -la "$OUT"/*.rpm
