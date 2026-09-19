#!/usr/bin/env bash
# Builds a deterministic-enough release archive for one Rust target:
#
#   ./scripts/build-release-archive.sh <target> [outdir]
#
# Supported targets: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu.
# Anything else is rejected before any build starts.
#
# Produces <outdir>/mec-<target>.tar.gz containing a single top-level
# directory mec-<VERSION>-<TARGET>/ with: mec (executable), README.md,
# LICENSE, SECURITY.md. No source, target dir, .git, configs, or logs.
#
# Reproducibility hygiene: deterministic entry ordering, neutral numeric
# ownership, and a stable mtime from SOURCE_DATE_EPOCH when set, else the
# current git commit timestamp. Each flag is probed individually so a tar
# lacking one optional flag still builds a plain archive instead of
# failing; CI on Ubuntu uses full GNU tar and takes the stronger path.
#
# Never uses sudo, never modifies system state, never downloads anything,
# never pushes/tags/releases. Cross-compile prerequisites (Rust target,
# linker) come from the CI environment.
set -euo pipefail

TARGET="${1:?usage: build-release-archive.sh <target> [outdir]}"
OUTDIR="${2:-target/release-artifacts}"

case "$TARGET" in
  x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu) ;;
  *)
    echo "error: unsupported target '$TARGET'" >&2
    echo "supported: x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu" >&2
    exit 1
    ;;
esac

VERSION="$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')"
ARCHIVE="mec-${TARGET}.tar.gz"
TOPDIR="mec-${VERSION}-${TARGET}"

cargo build --release --locked --target "$TARGET"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

mkdir -p "$STAGE/$TOPDIR" "$OUTDIR"
cp "target/${TARGET}/release/mec" "$STAGE/$TOPDIR/mec"
chmod 755 "$STAGE/$TOPDIR/mec"
cp README.md LICENSE SECURITY.md "$STAGE/$TOPDIR/"

TAR_FLAGS=()
if tar --sort-name -cf /dev/null --files-from /dev/null >/dev/null 2>&1; then
  TAR_FLAGS+=(--sort-name)
fi
if tar --owner=0 -cf /dev/null --files-from /dev/null >/dev/null 2>&1; then
  TAR_FLAGS+=(--owner=0 --group=0 --numeric-owner)
fi
EPOCH="${SOURCE_DATE_EPOCH:-}"
if [ -z "$EPOCH" ]; then
  EPOCH="$(git log -1 --format=%ct 2>/dev/null || true)"
fi
if [ -n "$EPOCH" ] && tar "--mtime=@${EPOCH}" -cf /dev/null --files-from /dev/null >/dev/null 2>&1; then
  TAR_FLAGS+=("--mtime=@${EPOCH}")
fi

tar "${TAR_FLAGS[@]}" -czf "$OUTDIR/$ARCHIVE" -C "$STAGE" "$TOPDIR"
echo "wrote $OUTDIR/$ARCHIVE"
