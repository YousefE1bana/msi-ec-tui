# Installation

Stable is v1.0.1; the v1.1 UI on this branch is not published. See the
[README](../README.md) for the inspect-first development installer. From a checkout:

```sh
bash install.sh --help
bash install.sh --dry-run
bash install.sh --version 1.0.1 --dry-run
bash install.sh
```

Prerequisites: Linux, curl, Python 3.12+, sha256sum, tar, getconf, and glibc >=2.39.
The installer detects distro/CPU/package manager, an existing MEC, msi-ec, and
Secure Boot where available. Unsupported architecture, musl, WSL, missing assets,
malformed release data, duplicate checksum entries, and failed integrity checks
stop installation. It requests administrator authentication only for native
package installation. Same stable version leaves the binary unchanged and runs
doctor. Dry-run performs release discovery but neither installs nor calls sudo.

Native families use apt-get (.deb), dnf/zypper (.rpm), with a portable fallback
where no matching package manager is available. This is package-format mapping,
not proof of hardware/distro support; consult [compatibility](compatibility.md).
Tar installs to ~/.local/bin/mec. Put that directory in your desktop session PATH.
Future RC tar assets carry a launcher/icon; v1.0.1 tar assets do not.
The RC native packages install `/usr/share/applications/mec.desktop` and
`/usr/share/icons/hicolor/scalable/apps/mec.svg`. The launcher uses `Exec=mec`,
`Terminal=true`, and your current permissions. No service or permission rules
are installed.

For manual installation, download the appropriate asset and SHA256SUMS from the
[same stable release](https://github.com/YousefE1bana/msi-ec-tui/releases/latest).
Verify its **exact filename** with `sha256sum --check` before invoking your native
package manager. The installer does this automatically and never installs after
a checksum failure. SHA256 is integrity checking, not publisher authentication.

Build from source with the pinned Rust toolchain:

```sh
cargo build --release --locked
./target/release/mec doctor
./target/release/mec
```

Read [hardware support](hardware-support.md) for driver/DKMS/MOK setup and
[troubleshooting](troubleshooting.md) for permission failures. No driver is
silently installed. Unknown hardware must remain READ-ONLY.

Config is `~/.config/mec/config.toml`; profiles are `~/.config/mec/profiles/`.
Installations do not overwrite user data. AUR publication is not claimed.
There are no cryptographic release signatures in the v1.0.1 asset set.

After installing, run `mec --version` and `mec doctor` before opening the TUI.
