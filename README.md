# MEC — MSI EC Control Center

> A safe, fast, capability-aware terminal control center for MSI laptops on Linux.

MEC is an open-source Linux terminal control center for monitoring and
safely controlling supported MSI laptops through the
[`msi-ec`](https://github.com/BeardOverflow/msi-ec) Linux kernel module.

## Status

PLAN-005 (profiles + transactions, v0.5.0) is implemented. Running
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
- Strict declarative TOML profiles with typed validation (`Profile`)
- Capability-aware profile previews (`ProfilePlanner::preview`) with
  changed-vs-unchanged detection against the current hardware snapshot
- Transaction planning from the current snapshot: changed settings only,
  in a stable order
- Ordered transactional application through the existing verified
  `HardwareCommand` write boundary, with rollback in reverse order and
  verified rollback results on failure
- Safe high-level `apply_profile` composition (single sysfs root,
  current-process permissions, no elevation)
- Five capability-aware built-in presets (see below); unsupported
  settings are omitted, never forced
- Read-only custom-profile discovery/loading from
  `~/.config/mec/profiles/` (see below)
- Profile CLI: `mec profile list`, `mec profile show <PROFILE>`,
  `mec profile apply <PROFILE>`
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

### Profile CLI

Profile mutation is CLI-driven; the interactive TUI remains read-only.

```text
mec profile list
mec profile show <PROFILE>
mec profile apply <PROFILE>
```

`mec profile list` lists the five built-ins first, then valid
custom-profile slugs in lexical order. It performs no hardware writes.

`mec profile show <PROFILE>` resolves and displays a profile in a
deterministic normalized view. It performs no hardware writes.

`mec profile apply <PROFILE>` resolves a profile and applies it only
through the safe transactional pipeline. Failures print
`MEC profile failed: ...` to stderr with a non-zero exit.

A PROFILE argument is a canonical slug: an exact built-in slug wins,
otherwise it must name a custom profile in the store.

### Built-in presets

```text
balanced          Balanced
silent            Silent
gaming            Gaming
battery-saver     Battery Saver
maximum-cooling   Maximum Cooling
```

Built-ins are capability-aware: they resolve against the currently
discovered hardware capabilities and may omit settings the hardware does
not expose. Resolution is data, not authorization — `apply_profile`
re-evaluates support, capabilities, and the hardware snapshot before any
write.

### Custom profile storage

Custom profiles live in:

```text
~/.config/mec/profiles/
```

A custom slug maps to `<slug>.toml` directly beneath that directory.
Current rules:

- lowercase ASCII letters, digits, and hyphens; 1..=64 bytes; first and
  last characters alphanumeric
- built-in slugs (`balanced`, `silent`, `gaming`, `battery-saver`,
  `maximum-cooling`) are reserved and can never shadow a built-in
- only direct-child regular `.toml` files are discovered; foreign
  extensions, directories, hidden/temp files, invalid names, and reserved
  names are ignored
- symlink profile files are ignored by listing and rejected on load
- maximum profile file size is 64 KiB
- malformed profiles fail with a typed parse error; listing does not
  parse file contents, so a malformed file is still listed and fails on
  load/show/apply
- MEC currently only reads custom profiles; there is no profile
  save/delete CLI and the store never creates the directory itself

### Profile data safety

Profiles are declarative TOML DATA ONLY. They are never sourced as shell,
executed, environment-expanded, or treated as command hooks; they contain
no raw sysfs paths and no raw EC register programs. A profile may request
only the fields implemented by the schema.

Example (every section optional, at least one setting required):

```toml
name = "Gaming"

[performance]
shift_mode = "turbo"
fan_mode = "advanced"
cooler_boost = true
super_battery = false

[battery]
charge_end_threshold = 80

[device]
keyboard_backlight = 2
```

`charge_end_threshold = 80` maps to the existing `msi-ec` typed 70/80
threshold pair. There is no independent start-threshold profile field.

### Transaction semantics

Conceptual flow:

```text
Profile
  ↓
fresh support/capability evaluation
  ↓
preview / validation
  ↓
current hardware snapshot
  ↓
transaction plan
  ↓
changed settings only
  ↓
ordered verified writes
  ↓
success
```

On failure:

```text
failure
  ↓
rollback
  ↓
reverse order
  ↓
verified rollback result
```

Notes:

- unchanged requested settings are not rewritten
- unsupported custom-profile requests fail closed
- `READ-ONLY` support state prevents profile mutation
- writes continue to use the closed typed `HardwareCommand`/write
  boundary with mandatory readback verification
- rollback failures are surfaced with typed errors; rollback is
  best-effort application-level compensation, not an atomic
  kernel-level transaction

### Privilege model

Control commands execute with the current process permissions. MEC performs
no sudo/pkexec/polkit/setuid escalation and ships no privileged helper: a
write the OS denies returns a typed failure (`AccessDenied` chain). The
interactive TUI remains unprivileged and read-only. A dedicated
privilege/packaging deployment model remains future work.

Explicitly NOT implemented yet:

- Editable TUI controls / confirmations / command palette / profile
  screen (PLAN-006)
- Automatic privileged helper / privilege deployment integration
- Physical hardware write validation
- Packaging/release binaries (PLAN-007)

See the [design](docs/superpowers/specs/2026-09-18-mec-design.md) and
[implementation plan](docs/superpowers/plans/2026-09-18-mec-v1-implementation-plan.md).

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
