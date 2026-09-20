#!/usr/bin/env bash
# Builds one RPM package from a prebuilt mec release binary:
#
#   ./scripts/build-rpm.sh <binary> <target> <outdir>
#
# Supported targets map to RPM architectures: x86_64 -> x86_64,
# aarch64 -> aarch64. Anything else is rejected before doing anything.
#
# The spec at packaging/rpm/mec.spec is rendered with @VERSION@ (from
# Cargo metadata, never duplicated) and @ARCH@. No cargo runs inside
# %build, no lifecycle scripts, no services, udev rules, sysfs changes,
# or user management: passive file payload only (/usr/bin/mec plus
# documentation under /usr/share/doc/mec). Stripping and debug
# subpackages are disabled so the payload binary stays identical to the
# tested release binary.
#
# Never uses sudo, never installs anything on the host, never downloads
# anything. Builds inside a temporary rpmbuild topdir (cleaned by trap)
# using distro rpmbuild, with the intended RPM architecture passed
# explicitly via --target and required to match rpm's native host CPU
# (cross-RPM generation fails closed). Outputs exactly one .rpm, normalized to
# mec-<VERSION>-1.<arch>.rpm, only after its internal Name, Version,
# Release, and Architecture metadata verify against expectations (so a
# filename can never claim an architecture the RPM metadata disagrees
# with).
set -euo pipefail

BINARY="${1:?usage: build-rpm.sh <binary> <target> <outdir>}"
TARGET="${2:?usage: build-rpm.sh <binary> <target> <outdir>}"
OUTDIR="${3:?usage: build-rpm.sh <binary> <target> <outdir>}"

case "$TARGET" in
  x86_64-unknown-linux-gnu) RPM_ARCH=x86_64 ;;
  aarch64-unknown-linux-gnu) RPM_ARCH=aarch64 ;;
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
RPM="mec-${VERSION}-1.${RPM_ARCH}.rpm"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

TOPDIR="$(mktemp -d)"
trap 'rm -rf "$TOPDIR"' EXIT

mkdir -p "$TOPDIR"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
cp "$BINARY" "$TOPDIR/SOURCES/mec"
chmod 755 "$TOPDIR/SOURCES/mec"
cp "$REPO_DIR/README.md" "$REPO_DIR/LICENSE" "$REPO_DIR/SECURITY.md" "$TOPDIR/SOURCES/"
sed -e "s/@VERSION@/${VERSION}/g" -e "s/@ARCH@/${RPM_ARCH}/g" \
  "$REPO_DIR/packaging/rpm/mec.spec" > "$TOPDIR/SPECS/mec.spec"

# Fail closed unless the builder is native for the requested RPM
# architecture: distro rpmbuild cannot create a foreign-arch RPM here
# (observed: "No compatible architectures found for build" for aarch64
# on x86_64). Release CI runs each architecture on its native runner.
HOST_RPM_ARCH="$(rpm --eval '%{_host_cpu}')"
case "$HOST_RPM_ARCH" in
  x86_64|amd64) HOST_RPM_ARCH=x86_64 ;;
  aarch64|arm64) HOST_RPM_ARCH=aarch64 ;;
esac
if [ "$HOST_RPM_ARCH" != "$RPM_ARCH" ]; then
  echo "error: RPM packaging requires a native $RPM_ARCH builder (rpm host: $HOST_RPM_ARCH)" >&2
  exit 1
fi

rpmbuild -bb --target "$RPM_ARCH" --define "_topdir $TOPDIR" "$TOPDIR/SPECS/mec.spec"

BUILT="$(find "$TOPDIR/RPMS" -name '*.rpm' | LC_ALL=C sort)"
COUNT="$(printf '%s\n' "$BUILT" | grep -c .)"
if [ "$COUNT" -ne 1 ]; then
  echo "error: expected exactly one rpm, found $COUNT" >&2
  printf '%s\n' "$BUILT" >&2
  exit 1
fi

# Verify internal metadata before emitting the release artifact.
read -r GOT_NAME GOT_VERSION GOT_RELEASE GOT_ARCH < <(
  rpm -qp --queryformat '%{NAME} %{VERSION} %{RELEASE} %{ARCH}\n' "$BUILT"
)
if [ "$GOT_NAME" != "mec" ] || [ "$GOT_VERSION" != "$VERSION" ] \
  || [ "$GOT_RELEASE" != "1" ] || [ "$GOT_ARCH" != "$RPM_ARCH" ]; then
  echo "error: rpm metadata mismatch: got '$GOT_NAME $GOT_VERSION $GOT_RELEASE $GOT_ARCH', want 'mec $VERSION 1 $RPM_ARCH'" >&2
  exit 1
fi

mkdir -p "$OUTDIR"
cp "$BUILT" "$OUTDIR/$RPM"
echo "wrote $OUTDIR/$RPM"
