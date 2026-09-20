#!/usr/bin/env bash
# Generates AUR binary package files from release checksums:
#
#   ./scripts/generate-aur-package.sh <version> <SHA256SUMS-path> <output-directory>
#
# Reads the exact SHA256 values for mec-x86_64-unknown-linux-gnu.tar.gz
# and mec-aarch64-unknown-linux-gnu.tar.gz from an existing SHA256SUMS
# file and renders deterministic PKGBUILD and .SRCINFO under the output
# directory. No network access: generation is fully offline from local
# inputs. The template at packaging/arch/PKGBUILD.template carries the
# static package contract; only version and the two checksums vary.
#
# Fails closed on: missing checksum file, missing x86_64 entry, missing
# aarch64 entry, duplicate entries for either artifact, malformed SHA256
# values, or an unsupported/malformed version. Never uses sudo and never
# contacts the network.
set -euo pipefail

VERSION="${1:?usage: generate-aur-package.sh <version> <SHA256SUMS-path> <output-directory>}"
SUMS="${2:?usage: generate-aur-package.sh <version> <SHA256SUMS-path> <output-directory>}"
OUTDIR="${3:?usage: generate-aur-package.sh <version> <SHA256SUMS-path> <output-directory>}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATE="$REPO_DIR/packaging/arch/PKGBUILD.template"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: unsupported/malformed version '$VERSION' (want X.Y.Z)" >&2
  exit 1
fi

if [ ! -f "$SUMS" ]; then
  echo "error: checksum file missing: $SUMS" >&2
  exit 1
fi

if [ ! -f "$TEMPLATE" ]; then
  echo "error: template missing: $TEMPLATE" >&2
  exit 1
fi

X86_NAME="mec-x86_64-unknown-linux-gnu.tar.gz"
ARM_NAME="mec-aarch64-unknown-linux-gnu.tar.gz"
X86_SHA=""
ARM_SHA=""
X86_COUNT=0
ARM_COUNT=0

while IFS= read -r line || [ -n "$line" ]; do
  # Skip blank lines.
  if [[ "$line" =~ ^[[:space:]]*$ ]]; then
    continue
  fi
  sha="$(printf '%s' "$line" | awk '{print $1}')"
  rest="$(printf '%s' "$line" | awk '{ $1=""; sub(/^ +/, ""); print }')"
  # sha256sum binary mode prefixes the name with '*'; strip it.
  name="$(printf '%s' "$rest" | sed 's/^\*//; s#.*/##')"
  # Basename match against the two release archives only.
  if [ "$rest" = "$X86_NAME" ] || [ "$name" = "$X86_NAME" ]; then
    if ! [[ "$sha" =~ ^[0-9a-f]{64}$ ]]; then
      echo "error: malformed SHA256 for $X86_NAME: '$sha'" >&2
      exit 1
    fi
    X86_COUNT=$((X86_COUNT + 1))
    X86_SHA="$sha"
  elif [ "$rest" = "$ARM_NAME" ] || [ "$name" = "$ARM_NAME" ]; then
    if ! [[ "$sha" =~ ^[0-9a-f]{64}$ ]]; then
      echo "error: malformed SHA256 for $ARM_NAME: '$sha'" >&2
      exit 1
    fi
    ARM_COUNT=$((ARM_COUNT + 1))
    ARM_SHA="$sha"
  fi
done < "$SUMS"

if [ "$X86_COUNT" -eq 0 ]; then
  echo "error: missing checksum entry for $X86_NAME" >&2
  exit 1
fi
if [ "$ARM_COUNT" -eq 0 ]; then
  echo "error: missing checksum entry for $ARM_NAME" >&2
  exit 1
fi
if [ "$X86_COUNT" -ne 1 ]; then
  echo "error: duplicate checksum entries for $X86_NAME ($X86_COUNT)" >&2
  exit 1
fi
if [ "$ARM_COUNT" -ne 1 ]; then
  echo "error: duplicate checksum entries for $ARM_NAME ($ARM_COUNT)" >&2
  exit 1
fi

mkdir -p "$OUTDIR"

sed -e "s/@VERSION@/${VERSION}/g" -e "s/@X86_SHA@/${X86_SHA}/g" -e "s/@AARCH64_SHA@/${ARM_SHA}/g" \
  "$TEMPLATE" > "$OUTDIR/PKGBUILD"

X86_URL="https://github.com/YousefE1bana/msi-ec-tui/releases/download/v${VERSION}/mec-x86_64-unknown-linux-gnu.tar.gz"
ARM_URL="https://github.com/YousefE1bana/msi-ec-tui/releases/download/v${VERSION}/mec-aarch64-unknown-linux-gnu.tar.gz"

{
  printf 'pkgbase = mec-bin\n'
  printf '\t%s\n' \
    "pkgdesc = A safe capability-aware terminal control center for MSI laptops on Linux" \
    "pkgver = $VERSION" \
    "pkgrel = 1" \
    "url = https://github.com/YousefE1bana/msi-ec-tui" \
    "arch = x86_64" \
    "arch = aarch64" \
    "license = MIT" \
    "provides = mec" \
    "conflicts = mec" \
    "source_x86_64 = mec-x86_64-unknown-linux-gnu.tar.gz::$X86_URL" \
    "sha256sums_x86_64 = $X86_SHA" \
    "source_aarch64 = mec-aarch64-unknown-linux-gnu.tar.gz::$ARM_URL" \
    "sha256sums_aarch64 = $ARM_SHA"
  printf '\n'
  printf 'pkgname = mec-bin\n'
} > "$OUTDIR/.SRCINFO"

echo "wrote $OUTDIR/PKGBUILD $OUTDIR/.SRCINFO"
