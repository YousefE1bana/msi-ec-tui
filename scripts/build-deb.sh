#!/usr/bin/env bash
# Builds one Debian package from a prebuilt mec release binary:
#
#   ./scripts/build-deb.sh <binary> <target> <outdir>
#
# Supported targets map to Debian architectures: x86_64 -> amd64,
# aarch64 -> arm64. Anything else is rejected before doing anything.
#
# Metadata comes from packaging/deb/control.template with @VERSION@
# (from Cargo metadata, never duplicated) and @ARCH@ filled in. No
# Depends line on purpose: the binary links only essential system
# libraries and MEC treats a missing msi-ec interface as READ-ONLY.
# No maintainer scripts are created: passive file payload only
# (/usr/bin/mec plus documentation; nothing under /etc, /sys, /home,
# and no services, udev rules, or user management).
#
# Never uses sudo, never installs anything on the host, never downloads
# anything. Builds entirely inside a temporary directory (cleaned by
# trap) using distro dpkg-deb. Outputs exactly one .deb.
set -euo pipefail

BINARY="${1:?usage: build-deb.sh <binary> <target> <outdir>}"
TARGET="${2:?usage: build-deb.sh <binary> <target> <outdir>}"
OUTDIR="${3:?usage: build-deb.sh <binary> <target> <outdir>}"

case "$TARGET" in
  x86_64-unknown-linux-gnu) DEB_ARCH=amd64 ;;
  aarch64-unknown-linux-gnu) DEB_ARCH=arm64 ;;
  *)
    echo "error: unsupported target '$TARGET'" >&2
    echo "supported: x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu" >&2
    exit 1
    ;;
esac

if [ ! -f "$BINARY" ]; then
  echo "error: not a regular file: $BINARY" >&2
  exit 1
fi
if [ ! -x "$BINARY" ]; then
  echo "error: not executable: $BINARY" >&2
  exit 1
fi

VERSION="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
DEB="mec_${VERSION}_${DEB_ARCH}.deb"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/DEBIAN" "$STAGE/usr/bin" "$STAGE/usr/share/doc/mec"
sed -e "s/@VERSION@/${VERSION}/g" -e "s/@ARCH@/${DEB_ARCH}/g" \
  "$REPO_DIR/packaging/deb/control.template" > "$STAGE/DEBIAN/control"
cp "$BINARY" "$STAGE/usr/bin/mec"
chmod 755 "$STAGE/usr/bin/mec"
cp "$REPO_DIR/README.md" "$REPO_DIR/LICENSE" "$REPO_DIR/SECURITY.md" \
  "$STAGE/usr/share/doc/mec/"
chmod 644 "$STAGE/usr/share/doc/mec/"*

mkdir -p "$OUTDIR"
# --root-owner-group keeps payload ownership neutral for rootless builds.
dpkg-deb --root-owner-group --build "$STAGE" "$OUTDIR/$DEB"

# Verify internal metadata before reporting success, so the filename can
# never claim a package/version/architecture the control data disagrees
# with.
GOT_PACKAGE="$(dpkg-deb -f "$OUTDIR/$DEB" Package)"
GOT_VERSION="$(dpkg-deb -f "$OUTDIR/$DEB" Version)"
GOT_ARCH="$(dpkg-deb -f "$OUTDIR/$DEB" Architecture)"
if [ "$GOT_PACKAGE" != "mec" ] || [ "$GOT_VERSION" != "$VERSION" ] \
  || [ "$GOT_ARCH" != "$DEB_ARCH" ]; then
  echo "error: deb metadata mismatch: got '$GOT_PACKAGE $GOT_VERSION $GOT_ARCH', want 'mec $VERSION $DEB_ARCH'" >&2
  exit 1
fi
echo "wrote $OUTDIR/$DEB"
