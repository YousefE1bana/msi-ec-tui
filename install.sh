#!/usr/bin/env bash
# MEC release installer. Inspect before running. Never installs kernel code.
set -euo pipefail
REPO=YousefE1bana/msi-ec-tui
MEC_VERSION=""
DRY_RUN=false
TEMP_DIR=""
INSTALL_STARTED=false
usage() {
  cat <<'HELP'
MEC installer — Linux / glibc / x86_64 or aarch64
Usage: bash install.sh [--version 1.0.1] [--dry-run] [--help]
Default: discover latest stable GitHub Release, verify exact SHA256SUMS entry,
then install a native .deb/.rpm or a user-local portable binary.
--dry-run performs discovery only: no artifact install or sudo.
Driver setup and Secure Boot enrollment are explicit manual steps; never automatic.
HELP
}
installer_failure() {
  if [[ "$INSTALL_STARTED" == true ]]; then
    printf 'Installation stopped; the MEC binary/package may have changed.\nCheck mec --version and mec doctor; see docs/troubleshooting.md before retrying.\n' >&2
  else
    printf 'Nothing was installed. Fix the reported problem, then rerun with --dry-run.\n' >&2
  fi
}
die() { printf 'MEC installer: %s\n' "$*" >&2; installer_failure; exit 1; }
map_arch() {
  case "$1" in
    x86_64|amd64) printf 'x86_64 amd64 x86_64-unknown-linux-gnu\n' ;;
    aarch64|arm64) printf 'aarch64 arm64 aarch64-unknown-linux-gnu\n' ;;
    *) return 1 ;;
  esac
}
select_format() {
  case " $1 $2 " in
    *' ubuntu '*|*' debian '*) printf 'deb\n' ;;
    *' fedora '*|*' rhel '*|*' centos '*|*' rocky '*|*' almalinux '*|*' opensuse '*|*' suse '*) printf 'rpm\n' ;;
    *) printf 'tar\n' ;;
  esac
}
# Require a unique exact basename entry. Never trust an arbitrary checksum path.
verify_artifact() {
  python3 - "$1" "$2" "$3" <<'PY'
import hashlib, pathlib, re, sys
artifact, sums, name = sys.argv[1:]
entries=[]
for line in pathlib.Path(sums).read_text().splitlines():
    match=re.fullmatch(r'([0-9a-fA-F]{64}) [ *](.+)',line)
    if match and match[2]==name: entries.append(match[1].lower())
if len(entries)!=1: sys.exit('Missing or ambiguous exact checksum entry')
h=hashlib.sha256()
with open(artifact,'rb') as source:
    for chunk in iter(lambda: source.read(1048576), b''): h.update(chunk)
if h.hexdigest()!=entries[0]: sys.exit('SHA256 mismatch — nothing will be installed')
print('Verified exact SHA256:',name)
PY
}
guidance() {
  if [[ -d /sys/devices/platform/msi-ec && -r /sys/devices/platform/msi-ec/fw_version ]]; then
    printf 'msi-ec interface detected. MEC doctor remains authoritative.\n'
  else
    printf 'msi-ec interface missing/incomplete. Monitoring may be READ-ONLY.\n'
    printf 'Driver guide: https://github.com/BeardOverflow/msi-ec\n'
    printf 'Use your distro kernel headers/build tools and upstream DKMS instructions.\n'
  fi
  if command -v mokutil >/dev/null 2>&1; then mokutil --sb-state || true
  else printf 'Secure Boot status unavailable (mokutil not installed).\n'; fi
  printf 'Secure Boot: sign/enroll a DKMS module through your distro MOK workflow,\nreboot if requested, then rerun mec doctor. Do not disable security to bypass signing.\n'
  printf 'MEC uses current-process permissions. READY does not grant sysfs write access.\n'
  printf 'Do not chmod hardware nodes world-writable or make MEC setuid.\n'
}
main() {
  trap 'installer_failure' ERR
  while (($#)); do
    case "$1" in
      --help|-h) usage; return ;;
      --dry-run) DRY_RUN=true ;;
      --version) (($# >= 2)) || die '--version requires a stable version'; MEC_VERSION="${2#v}"; shift ;;
      *) die "unknown option: $1" ;;
    esac
    shift
  done
  python3 -c 'import sys; sys.exit(0 if sys.version_info >= (3,12) else "Python 3.12+ required")'
  [[ $(uname -s) == Linux ]] || die 'only Linux is supported'
  [[ -z "$MEC_VERSION" || "$MEC_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die 'version must be stable X.Y.Z'
  for tool in curl python3 sha256sum tar getconf; do command -v "$tool" >/dev/null || die "missing prerequisite: $tool"; done
  local rpm_arch deb_arch rust_target family format release tag asset manager existing glibc
  read -r rpm_arch deb_arch rust_target < <(map_arch "$(uname -m)") || die 'unsupported CPU architecture'
  [[ -n "$rust_target" ]] || die 'unsupported CPU architecture'
  glibc=$(getconf GNU_LIBC_VERSION 2>/dev/null) || die 'glibc is required; musl is unsupported'
  python3 - "$glibc" <<'PY'
import re, sys
m=re.fullmatch(r'glibc (\d+)\.(\d+)',sys.argv[1])
if not m or tuple(map(int,m.groups())) < (2,39): sys.exit('Release binaries require glibc >= 2.39; build from source on older systems')
PY
  if [[ -r /proc/sys/kernel/osrelease ]] && [[ $(tr '[:upper:]' '[:lower:]' < /proc/sys/kernel/osrelease) == *microsoft* ]]; then die 'WSL lacks the required physical msi-ec interface'; fi
  ID=unknown ID_LIKE=""
  [[ ! -r /etc/os-release ]] || source /etc/os-release
  family="$ID ${ID_LIKE:-}"
  format=$(select_format "$ID" "${ID_LIKE:-}")
  manager=""
  case "$format" in
    deb) command -v apt-get >/dev/null && manager=apt-get || format=tar ;;
    rpm) if command -v dnf >/dev/null; then manager=dnf
         elif command -v zypper >/dev/null; then manager=zypper
         else format=tar; fi ;;
  esac
  existing=$(command -v mec || true)
  printf 'Detected: %s / %s / %s\n' "$family" "$rpm_arch" "$glibc"
  if [[ -n "$existing" ]]; then printf 'Existing installation: %s (%s)\n' "$existing" "$("$existing" --version)"; fi
  guidance
  TEMP_DIR=$(mktemp -d)
  trap '[[ -z "$TEMP_DIR" ]] || rm -rf -- "$TEMP_DIR"' EXIT
  release="https://api.github.com/repos/$REPO/releases/latest"
  [[ -z "$MEC_VERSION" ]] || release="https://api.github.com/repos/$REPO/releases/tags/v$MEC_VERSION"
  curl --proto '=https' --tlsv1.2 --fail --silent --show-error --connect-timeout 10 --max-time 30 --max-filesize 1048576 "$release" -o "$TEMP_DIR/release.json"
  tag=$(python3 - "$TEMP_DIR/release.json" "$MEC_VERSION" <<'PY'
import json,re,sys
r=json.load(open(sys.argv[1]))
t=r.get('tag_name','')
if r.get('draft') is not False or r.get('prerelease') is not False or not re.fullmatch(r'v\d+\.\d+\.\d+',t): sys.exit('Not a valid stable release')
if sys.argv[2] and t!='v'+sys.argv[2]: sys.exit('Requested version does not match release metadata')
print(t)
PY
)
  MEC_VERSION="${tag#v}"
  case "$format" in
    deb) asset="mec_${MEC_VERSION}_${deb_arch}.deb" ;;
    rpm) asset="mec-${MEC_VERSION}-1.${rpm_arch}.rpm" ;;
    tar) asset="mec-${rust_target}.tar.gz" ;;
  esac
  python3 - "$TEMP_DIR/release.json" "$asset" <<'PY'
import json,sys
names=[a.get('name') for a in json.load(open(sys.argv[1])).get('assets',[])]
for name in [sys.argv[2],'SHA256SUMS']:
    if names.count(name)!=1: sys.exit('Missing or ambiguous release asset: '+name)
PY
  printf 'Selected stable release: %s\nArtifact: %s\n' "$tag" "$asset"
  if [[ "$DRY_RUN" == true ]]; then
    printf 'DRY RUN: would verify exact SHA256SUMS, then install with %s.\n' "${manager:-user-local portable install}"
    [[ "$format" != tar ]] || printf 'Destination: %s/.local/bin/mec\n' "$HOME"
    return
  fi
  if [[ -n "$existing" ]] && [[ $("$existing" --version) == "mec $MEC_VERSION" ]]; then
    printf 'This stable version is already installed; leaving installation unchanged.\n'
    "$existing" doctor
    return
  fi
  local base="https://github.com/$REPO/releases/download/$tag"
  for file in "$asset" SHA256SUMS; do
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fLSs --connect-timeout 10 --max-time 180 "$base/$file" -o "$TEMP_DIR/$file"
  done
  verify_artifact "$TEMP_DIR/$asset" "$TEMP_DIR/SHA256SUMS" "$asset"
  if [[ "$format" == tar ]]; then
    # Validate names/types before extraction. Only an allowlisted passive payload.
    python3 - "$TEMP_DIR/$asset" "mec-$MEC_VERSION-$rust_target" "$TEMP_DIR/unpacked" <<'PY'
import pathlib,sys,tarfile
archive,top,out=sys.argv[1:]
allowed={top,top+'/mec',top+'/README.md',top+'/LICENSE',top+'/SECURITY.md',top+'/mec.desktop',top+'/mec.svg'}
with tarfile.open(archive,'r:gz') as t:
    members=t.getmembers()
    if len({m.name.rstrip('/') for m in members})!=len(members): sys.exit('Duplicate archive entries')
    for m in members:
        name=m.name.rstrip('/')
        path=pathlib.PurePosixPath(name)
        if path.is_absolute() or '..' in path.parts or str(path)!=name: sys.exit('Unsafe archive path')
        doc=name==top+'/docs' or name.startswith(top+'/docs/')
        if doc:
            if not (m.isdir() or (m.isfile() and path.suffix in {'.md','.png','.svg'})): sys.exit('Unexpected documentation payload')
        elif name not in allowed or not (m.isfile() or (m.isdir() and name==top)): sys.exit('Unsafe or unexpected archive entry')
    if sum(m.name==top+'/mec' and m.isfile() for m in members)!=1: sys.exit('Missing binary')
    t.extractall(out,filter='data')
PY
    local payload="$TEMP_DIR/unpacked/mec-$MEC_VERSION-$rust_target"
    INSTALL_STARTED=true
    mkdir -p "$HOME/.local/bin"
    install -m755 "$payload/mec" "$HOME/.local/bin/mec.new"
    mv -f "$HOME/.local/bin/mec.new" "$HOME/.local/bin/mec"
    existing="$HOME/.local/bin/mec"
    mkdir -p "$HOME/.local/share/doc/mec"
    install -m644 "$payload/README.md" "$payload/LICENSE" "$payload/SECURITY.md" "$HOME/.local/share/doc/mec/"
    if [[ -d "$payload/docs" ]]; then cp -R "$payload/docs" "$HOME/.local/share/doc/mec/"; fi
    if [[ -f "$payload/mec.desktop" && -f "$payload/mec.svg" ]]; then
      mkdir -p "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/scalable/apps"
      install -m644 "$payload/mec.desktop" "$HOME/.local/share/applications/mec.desktop"
      install -m644 "$payload/mec.svg" "$HOME/.local/share/icons/hicolor/scalable/apps/mec.svg"
      printf 'Desktop launcher installed; ensure ~/.local/bin is in your desktop session PATH.\n'
    else printf 'This stable archive has no desktop assets; RC packages add them.\n'; fi
    printf 'Ensure ~/.local/bin is in PATH.\n'
  else
    INSTALL_STARTED=true
    printf 'Installing verified package with %s; sudo may ask for authentication.\n' "$manager"
    if ((EUID == 0)); then "$manager" install "$TEMP_DIR/$asset"
    else command -v sudo >/dev/null || die 'sudo unavailable: install the verified package as administrator'; sudo "$manager" install "$TEMP_DIR/$asset"; fi
    existing=$(command -v mec) || die 'package installed but mec not found in PATH'
  fi
  [[ $("$existing" --version) == "mec $MEC_VERSION" ]] || die 'installed version is shadowed in PATH; inspect existing MEC copies'
  "$existing" --version
  "$existing" doctor
  printf 'Done. Open MEC from a terminal or its desktop launcher.\n'
}
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then main "$@"; fi
