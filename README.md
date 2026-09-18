# MEC — MSI EC Control Center

> A safe, fast, capability-aware terminal control center for MSI laptops on Linux.

MEC is a planned open-source Linux TUI for monitoring and controlling MSI laptops through
the [`msi-ec`](https://github.com/BeardOverflow/msi-ec) Linux kernel module.

## Status

PLAN-004 (safe hardware controls, v0.4.0) is implemented. Running
`cargo run` with no subcommand launches the interactive read-only TUI when
both stdin and stdout are terminals:

- Arrow keys / hjkl / Tab move between screens
- `1`–`6` jump directly to a screen
- `?` toggles help, `q` quits

When stdout is redirected or captured (pipes, scripts, CI), bare `mec` still
prints exactly:

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
- Interactive read-only TUI (`mec` on a terminal): Dashboard, Performance,
  Fans, Battery, Devices, and Diagnostics screens with 1-second polling,
  keyboard navigation, help overlay, and degraded-telemetry presentation
- Typed hardware commands with pure validation against fresh
  support/capability state (`HardwareCommand`)
- Restricted `msi-ec` sysfs writer covering a closed set of known nodes,
  existing-node-only writes, and mandatory readback verification
- Explicit CLI controls that run every command through the full safe
  pipeline (fresh support evaluation, fresh capability discovery,
  validation, verified write)
- Reusable fake sysfs fixtures under `tests/fixtures/`

### CLI controls

Hardware mutation is exposed through explicit CLI controls only:

```text
mec fan mode <MODE>
mec shift <MODE>
mec cooler-boost <on|off>
mec super-battery <on|off>
mec webcam <on|off>
mec webcam-block <on|off>
mec keyboard-backlight <LEVEL>
mec battery limit <END_PERCENT>
```

Each control prints one `MEC control applied: ...` line only after the
complete pipeline, including readback verification, succeeds; failures
print `MEC control failed: ...` to stderr with a non-zero exit.

`mec battery limit 80` sets the `msi-ec` end threshold to 80%, which the
driver represents as the typed pair start = 70 / end = 80 (a fixed
10-point hysteresis). Start and end are not independently configurable.

### Privilege model

Control commands execute with the current process permissions. MEC performs
no sudo/pkexec/polkit/setuid escalation and ships no privileged helper: a
write the OS denies returns a typed failure (`AccessDenied` chain). The
interactive TUI remains unprivileged and read-only. A dedicated
privilege/packaging deployment model remains future work.

Explicitly NOT implemented yet:

- Profiles / transactional apply / rollback (PLAN-005)
- Editable TUI controls / confirmations / command palette (PLAN-006)
- Automatic privileged helper / privilege deployment integration
- Physical hardware write validation
- Packaging/release binaries (PLAN-007)

See the [design](docs/superpowers/specs/2026-09-18-mec-design.md) and
[implementation plan](docs/superpowers/plans/2026-09-18-mec-v1-implementation-plan.md).

## Planned features

- Automatic hardware detection and capability discovery
- Only expose controls the kernel driver actually supports
- Read-only mode for unknown or inconsistent hardware
- CPU/GPU temperatures, fan speeds, and fan/shift modes
- Cooler Boost, Super Battery, battery charge thresholds
- Webcam, Fn/Win key, and keyboard backlight controls
- Keyboard-driven TUI plus a scriptable CLI

Still planned (not implemented):

- Declarative TOML profiles with transactional apply and rollback

## Planned hardware support

- Primary physical test target: MSI GF series laptops
- Other MSI laptops through capabilities exposed by the `msi-ec` kernel module

Read-only behavior has been manually smoke-tested on real MSI hardware, but
hardware WRITE behavior has not yet been physically validated; write
semantics are currently covered by fake/temp sysfs integration tests.

## Building

```bash
cargo build
```

Rust `1.98.1` is pinned in `rust-toolchain.toml`.

## License

MIT — see [LICENSE](LICENSE).
