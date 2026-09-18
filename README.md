# MEC — MSI EC Control Center

> A safe, fast, capability-aware terminal control center for MSI laptops on Linux.

MEC is a planned open-source Linux TUI for monitoring and controlling MSI laptops through
the [`msi-ec`](https://github.com/BeardOverflow/msi-ec) Linux kernel module.

## Status

PLAN-001 (hardware foundation) is implemented. Running `cargo run` with no
subcommand still prints exactly:

```text
MEC — MSI EC Control Center
```

Implemented in this tree:

- MSI identity detection from Linux DMI
- Dynamic `msi-ec` capability discovery (no model allowlists)
- Conservative `READY` / `READ-ONLY` compatibility policy
- Read-only `msi-ec` backend with validated hardware snapshots
- `mec doctor [--sys-root PATH]` diagnostics (read-only)
- Read-only `mec status` with human-readable and `--json` output
- Runtime battery state (charge, status, AC presence)
- Read-only `mec monitor` with validated 500ms/1s/2s/5s polling,
  bounded in-memory history, Ctrl+C shutdown, and graceful
  snapshot-error degradation with recovery notices
- Reusable fake sysfs fixtures under `tests/fixtures/`

Explicitly NOT implemented yet:

- Hardware writes of any kind
- Profiles / transactional apply
- TUI
- Physical hardware validation
- Packaging/release binaries

See the [design](docs/superpowers/specs/2026-09-18-mec-design.md) and
[implementation plan](docs/superpowers/plans/2026-09-18-mec-v1-implementation-plan.md).

## Planned features

- Automatic hardware detection and capability discovery
- Only expose controls the kernel driver actually supports
- Read-only mode for unknown or inconsistent hardware
- CPU/GPU temperatures, fan speeds, and fan/shift modes
- Cooler Boost, Super Battery, battery charge thresholds
- Webcam, Fn/Win key, and keyboard backlight controls
- Declarative TOML profiles with transactional apply and rollback
- Keyboard-driven TUI plus a scriptable CLI

## Planned hardware support

- Primary physical test target: MSI GF series laptops
- Other MSI laptops through capabilities exposed by the `msi-ec` kernel module

No physical hardware validation has been performed; all hardware behavior is
currently verified against fake sysfs fixtures and unit tests.

## Building

```bash
cargo build
```

Rust `1.98.1` is pinned in `rust-toolchain.toml`.

## License

MIT — see [LICENSE](LICENSE).
