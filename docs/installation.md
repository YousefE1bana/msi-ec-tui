# Installation

MEC is distributed as prebuilt release artifacts plus an AUR binary
packaging workflow. Release binaries need no Rust toolchain; only
building from source does.

Release artifacts for each version (example `0.9.0`):

- `mec-x86_64-unknown-linux-gnu.tar.gz` (portable, x86_64)
- `mec-aarch64-unknown-linux-gnu.tar.gz` (portable, ARM64)
- `mec_0.9.0_amd64.deb` (Debian/Ubuntu family, x86_64)
- `mec_0.9.0_arm64.deb` (Debian/Ubuntu family, ARM64)
- `mec-0.9.0-1.x86_64.rpm` (RPM family, x86_64)
- `mec-0.9.0-1.aarch64.rpm` (RPM family, ARM64)
- `SHA256SUMS` (checksums covering all six files above)

Pick the install path that matches your system. The three paths are
alternatives: package installation, portable binary use, or building
from source.

## 1. Verify checksums first

Every install path below starts from the same release files. Verify
them before installing. There are SHA256 checksums; there are no
cryptographic release signatures.

```sh
sha256sum -c SHA256SUMS
```

Every line must report `OK`. Do not install files that fail
verification.

## 2. Debian/Ubuntu family (`.deb`)

Install the package matching your architecture with your normal package
tool, for example:

```sh
sudo apt install ./mec_0.9.0_amd64.deb
```

(ARM64 systems use `mec_0.9.0_arm64.deb` instead.)

This installs `/usr/bin/mec` plus documentation under
`/usr/share/doc/mec/`. The package is a passive file payload: it does
not modify `/sys` permissions, does not install driver or permission
workarounds, and does not create users, groups, services, or hooks.

Confirm with:

```sh
mec --version
mec doctor
```

## 3. RPM family (`.rpm`)

Install the package matching your architecture with your normal package
tool, for example:

```sh
sudo dnf install ./mec-0.9.0-1.x86_64.rpm
```

(ARM64 systems use `mec-0.9.0-1.aarch64.rpm` instead.)

This installs the same `/usr/bin/mec` plus documentation under
`/usr/share/doc/mec/`, with the same passive-payload guarantees as the
`.deb` path above.

Confirm with:

```sh
mec --version
mec doctor
```

Per-user data lives at `~/.config/mec/config.toml` and
`~/.config/mec/profiles/`. Installing, upgrading, or removing a package
never creates, overwrites, or deletes these paths; they are yours.
See [Upgrade](upgrade.md) and [Uninstall](uninstall.md) for details.

## 4. Portable `.tar.gz`

No installation and no administrator privileges are needed. Extract the
archive for your architecture and run the binary directly:

```sh
tar -xzf mec-x86_64-unknown-linux-gnu.tar.gz
./mec-0.9.0-x86_64-unknown-linux-gnu/mec --help
./mec-0.9.0-x86_64-unknown-linux-gnu/mec --sys-root tests/fixtures/gf63 status
```

Each archive contains exactly one top-level directory
(`mec-<version>-<target>/`) with `mec`, `README.md`, `LICENSE`, and
`SECURITY.md`. To "install" it system-wide, copy the `mec` binary to a
directory on your `PATH`; nothing else is required.

## 5. AUR binary packaging workflow

There is not yet a published AUR entry for MEC, and this guide does not
claim one exists. What the repository provides is a reproducible
generator that turns a released `SHA256SUMS` into AUR metadata:

- Template: `packaging/arch/PKGBUILD.template`
- Generator: `scripts/generate-aur-package.sh`
- Output: `PKGBUILD` and `.SRCINFO` for package `mec-bin`

The generated package installs the prebuilt GitHub release tarball for
your architecture (`mec-x86_64-unknown-linux-gnu.tar.gz` on x86_64,
`mec-aarch64-unknown-linux-gnu.tar.gz` on ARM64) as `/usr/bin/mec` plus
documentation under `/usr/share/doc/mec/`. It never rebuilds MEC, so an
AUR user does not need Rust. See [Upgrade](upgrade.md) and
[Uninstall](uninstall.md) for the once-published lifecycle.

## 6. Building from source

Source builds need the pinned toolchain in `rust-toolchain.toml`
(currently Rust 1.98.1) and otherwise follow the standard workflow:

```sh
cargo build --release
./target/release/mec --help
```

Release binaries never need this; use it only for development.

## 7. Hardware and privilege reality

MEC talks to supported MSI laptops through the upstream Linux `msi-ec`
interface. MEC does not install the kernel module and does not change
how it is loaded: if the interface is missing or incompatible, MEC
stays read-only or unavailable as applicable instead of working around
the hardware. Do not attempt to load an incompatible driver
configuration to force it.

Privilege model, as implemented:

- MEC contains no sudo/pkexec/polkit/setuid helper of any kind.
- The TUI is an ordinary user-space program, not something that must
  run as root.
- Reads work according to current permissions; writes use
  current-process permissions and denied writes fail with
  `AccessDenied`.
- Do not run MEC permanently as root to work around permissions.
- Packages do not change `/sys` permissions and do not install
  permission workarounds.

Physical write behavior has not been completed through physical
validation yet; writes are covered by fake sysfs integration tests.
