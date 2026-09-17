# Contributing to MEC

Thank you for your interest in contributing to MEC!

## Getting started

```bash
git clone https://github.com/YousefE1bana/msi-ec-tui.git
cd msi-ec-tui
cargo build
cargo test
```

Rust `1.98.1` is pinned in `rust-toolchain.toml`; rustup will fetch it
automatically.

## Development gates

Every contribution must pass:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

CI is planned for a later task; it is not configured in this bootstrap.

## Ground rules

1. **Capability-first.** Never assume a feature exists on a given MSI laptop.
   Features appear only when the kernel driver exposes them.
2. **Read-only fallback.** Unknown or inconsistent hardware must remain usable
   for monitoring with writes disabled.
3. **No raw EC access.** All hardware interaction goes through the typed
   hardware backend; arbitrary sysfs path/value writes must never reach the UI
   or CLI.
4. **Frontends stay clean.** TUI and CLI code must never read or write sysfs
   directly.
5. **Profiles are data.** TOML profiles are parsed as data, never executed.
6. **Tests need no hardware.** Tests use fake sysfs trees and mock backends;
   never require real MSI hardware.
7. **Linux only** for v1.

## Pull requests

- Keep changes focused; one logical change per pull request.
- Add or update tests for behavior changes.
- Update documentation when behavior or interfaces change.
- Run the gates above before submitting.

## Reporting issues

For bugs, include your distribution, kernel version, laptop model, and the
output of `mec doctor --export` (once available), after reviewing it for
private information.

For security issues, see [SECURITY.md](SECURITY.md) — do not open a public
issue.
