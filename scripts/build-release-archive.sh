#!/usr/bin/env bash
# Builds a reproducible release archive for one Rust target:
#
#   ./scripts/build-release-archive.sh [--from-stage DIR] <target> [outdir]
#
# Supported targets: x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu.
# Anything else is rejected before any build starts.
#
# Produces <outdir>/mec-<target>.tar.gz containing exactly one top-level
# directory mec-<VERSION>-<TARGET>/ with: mec (executable), README.md,
# LICENSE, SECURITY.md. No source, target dir, .git, configs, or logs.
#
# Normally the payload is assembled from a fresh
# `cargo build --release --locked --target` plus the repo docs.
# `--from-stage DIR` skips the cargo build and archives the already
# prepared staging tree DIR (which must already be named exactly
# mec-<VERSION>-<TARGET>/ with the four payload files). This narrow mode
# exists so reproducibility can be tested cheaply without compiling
# twice; release CI never uses it.
#
# Reproducibility contract: deterministic entry ordering via the real GNU
# tar option --sort=name (probed, not assumed), neutral numeric ownership
# (--owner=0 --group=0 --numeric-owner, probed as one set), a stable mtime
# (--mtime, from SOURCE_DATE_EPOCH when set, else the current git commit
# timestamp), and deterministic compression (gzip -n, so no filename or
# timestamp enters the gzip header). Each tar flag is probed so a tar
# lacking one optional flag still builds a plain archive instead of
# failing; CI on Ubuntu uses full GNU tar and takes the deterministic
# path. Identical payload bytes plus identical SOURCE_DATE_EPOCH yield
# byte-for-byte identical .tar.gz (proven by tests/release_packaging.rs).
#
# Never uses sudo, never modifies system state, never downloads anything,
# never pushes/tags/releases. Cross-compile prerequisites (Rust target,
# linker) come from the CI environment.
set -euo pipefail

FROM_STAGE=""
if [ "${1:-}" = "--from-stage" ]; then
  FROM_STAGE="${2:?usage: build-release-archive.sh [--from-stage DIR] <target> [outdir]}"
  shift 2
fi

TARGET="${1:?usage: build-release-archive.sh [--from-stage DIR] <target> [outdir]}"
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

STAGE=""
if [ -n "$FROM_STAGE" ]; then
  if [ ! -d "$FROM_STAGE/$TOPDIR" ]; then
    echo "error: staging tree missing expected top directory: $FROM_STAGE/$TOPDIR" >&2
    exit 1
  fi
  STAGE="$FROM_STAGE"
else
  cargo build --release --locked --target "$TARGET"

  STAGE="$(mktemp -d)"
  trap 'rm -rf "$STAGE"' EXIT

  mkdir -p "$STAGE/$TOPDIR" "$OUTDIR"
  cp "target/${TARGET}/release/mec" "$STAGE/$TOPDIR/mec"
  chmod 755 "$STAGE/$TOPDIR/mec"
  cp README.md LICENSE SECURITY.md "$STAGE/$TOPDIR/"
fi
mkdir -p "$OUTDIR"

# Feature probes: each flag set is tested against the local tar before
# use, so the flags actually appended are exactly the flags verified.
TAR_FLAGS=()
if tar --sort=name -cf /dev/null --files-from /dev/null >/dev/null 2>&1; then
  TAR_FLAGS+=(--sort=name)
fi
if tar --owner=0 --group=0 --numeric-owner -cf /dev/null --files-from /dev/null >/dev/null 2>&1; then
  TAR_FLAGS+=(--owner=0 --group=0 --numeric-owner)
fi
EPOCH="${SOURCE_DATE_EPOCH:-}"
if [ -z "$EPOCH" ]; then
  EPOCH="$(git log -1 --format=%ct 2>/dev/null || true)"
fi
if [ -n "$EPOCH" ] && tar "--mtime=@${EPOCH}" -cf /dev/null --files-from /dev/null >/dev/null 2>&1; then
  TAR_FLAGS+=("--mtime=@${EPOCH}")
fi

if printf '' | gzip -n -c >/dev/null 2>&1; then
  tar "${TAR_FLAGS[@]}" -cf - -C "$STAGE" "$TOPDIR" | gzip -n > "$OUTDIR/$ARCHIVE"
else
  tar "${TAR_FLAGS[@]}" -czf "$OUTDIR/$ARCHIVE" -C "$STAGE" "$TOPDIR"
fi
echo "wrote $OUTDIR/$ARCHIVE"
