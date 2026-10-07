#!/usr/bin/env python3
"""Offline release payload validation against the current source and tested binary."""
import hashlib
from pathlib import Path
import re
import sys
import tarfile

archive, target, version = sys.argv[1:]
if target not in {"x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"} or not re.fullmatch(r"\d+\.\d+\.\d+", version):
    sys.exit("Unsupported target/version")
top = f"mec-{version}-{target}"
files = {
    f"{top}/mec": Path(f"target/{target}/release/mec"),
    **{f"{top}/{name}": Path(name) for name in ["README.md", "LICENSE", "SECURITY.md"]},
    f"{top}/mec.desktop": Path("packaging/desktop/mec.desktop"),
    f"{top}/mec.svg": Path("packaging/desktop/mec.svg"),
}
dirs = {top, f"{top}/docs"}
for source in Path("docs").rglob("*"):
    if source.is_symlink(): sys.exit("Documentation must not contain symlinks")
    name = f"{top}/{source.as_posix()}"
    if source.is_dir(): dirs.add(name)
    elif source.is_file(): files[name] = source
    else: sys.exit("Unexpected documentation node")

def digest(reader):
    h = hashlib.sha256()
    for chunk in iter(lambda: reader.read(1048576), b""): h.update(chunk)
    return h.digest()

with tarfile.open(archive, "r:gz") as payload:
    members = payload.getmembers()
    if len(members) != len(files) + len(dirs) or {m.name.rstrip("/") for m in members} != set(files) | dirs:
        sys.exit("Archive does not match the exact approved payload")
    for member in members:
        name = member.name.rstrip("/")
        if name in dirs:
            if not member.isdir(): sys.exit("Expected a directory")
            continue
        if not member.isfile(): sys.exit("Expected a regular file")
        if name.endswith("/mec") and not member.mode & 0o111: sys.exit("Binary is not executable")
        with files[name].open("rb") as source:
            if digest(source) != digest(payload.extractfile(member)): sys.exit(f"Payload bytes differ: {name}")
print("Archive matches tested binary, desktop assets, and public documentation")
