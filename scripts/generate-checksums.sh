#!/usr/bin/env bash
# Writes SHA256SUMS for release artifacts in one directory:
#
#   ./scripts/generate-checksums.sh <artifact-dir> [required-basename ...]
#
# Checksums every regular file directly inside <artifact-dir> except
# SHA256SUMS itself, in deterministic lexical order, with basename-only
# entries (no absolute paths). Fails on an empty artifact directory and
# on any missing explicitly required basename. Never uses sudo.
set -euo pipefail

DIR="${1:?usage: generate-checksums.sh <artifact-dir> [required-basename ...]}"
shift || true

if [ ! -d "$DIR" ]; then
  echo "error: not a directory: $DIR" >&2
  exit 1
fi

mapfile -t FILES < <(find "$DIR" -maxdepth 1 -type f ! -name 'SHA256SUMS' -printf '%f\n' | LC_ALL=C sort)

if [ "${#FILES[@]}" -eq 0 ]; then
  echo "error: no artifacts in $DIR" >&2
  exit 1
fi

for required in "$@"; do
  found=0
  for have in "${FILES[@]}"; do
    if [ "$have" = "$required" ]; then
      found=1
      break
    fi
  done
  if [ "$found" -eq 0 ]; then
    echo "error: expected release artifact missing: $required" >&2
    exit 1
  fi
done

(cd "$DIR" && sha256sum "${FILES[@]}" > SHA256SUMS)
echo "wrote $DIR/SHA256SUMS (${#FILES[@]} files)"
