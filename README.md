# MEC — MSI EC Control Center

> A safe, fast, capability-aware terminal control center for MSI laptops on Linux.

MEC is a planned open-source Linux TUI for monitoring and controlling MSI laptops through
the [`msi-ec`](https://github.com/BeardOverflow/msi-ec) Linux kernel module.

## Status

Only PLAN-001 Task 1 (Rust project bootstrap) is implemented. Running `cargo run`
prints exactly:

```text
MEC — MSI EC Control Center
```

There is no hardware/sysfs access, CLI command handling, profile support, or TUI
yet. See the [design](docs/superpowers/specs/2026-09-18-mec-design.md) and
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

The bootstrap has no hardware support or physical validation.

## Building

```bash
cargo build
```

Rust `1.98.1` is pinned in `rust-toolchain.toml`.

## License

MIT — see [LICENSE](LICENSE).
