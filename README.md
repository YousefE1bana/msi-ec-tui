# MEC — MSI EC Control Center

> A safe, fast, capability-aware terminal control center for MSI laptops on Linux.

MEC is an open-source Linux terminal control center for monitoring and
safely controlling supported MSI laptops through the
[`msi-ec`](https://github.com/BeardOverflow/msi-ec) Linux kernel module.

## Status

MEC v1.0.0 is the first published stable release. The current
package/release line is 1.0.1, a patch release fixing false hardware
verification failures and the `mec --version` flag. PLAN-006
(interactive TUI) is implemented, and PLAN-007 release packaging is
implemented: x86_64 and ARM64 release archives with `SHA256SUMS`, `.deb`
and `.rpm` packages built natively per architecture, reproducible AUR
(`mec-bin`) package generation from the released checksums, and
install/upgrade/uninstall guides.
Running `cargo run` with no subcommand launches the interactive TUI when
both stdin and stdout are terminals:

- `1`–`7` jump directly to a screen (Dashboard, Performance, Fans,
  Battery, Devices, Profiles, Diagnostics)
- Arrow keys / Tab move between screens; `h`/`j`/`k`/`l` too unless
  `vim_keys = false` is configured
- `P` opens the command palette (screens, notifications, themes)
- `Enter` edits / accepts / confirms depending on context;
  `Esc` cancels an edit or confirmation, or closes an overlay
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
- Privacy-conscious `mec doctor --export` compatibility report for
  GitHub issues (no serials, hostnames, network data, profile/config
  contents, or live telemetry)
- `mec status` with human-readable and `--json` output
- Runtime battery state (charge, status, AC presence)
- `mec monitor` with validated 500ms/1s/2s/5s polling,
  bounded in-memory history, Ctrl+C shutdown, and graceful
  snapshot-error degradation with recovery notices
- Seven-screen interactive TUI (`mec` on a terminal): Dashboard,
  Performance, Fans, Battery, Devices, Profiles, and Diagnostics with
  configurable polling, contextual selection/editing, and an explicit
  confirmation step before any mutation
- Safe TUI hardware commands through the existing safety APIs, and TUI
  profile preview/apply through the transactional pipeline
- Custom profile catalog (built-ins plus `~/.config/mec/profiles/`)
- Command palette (`P`) with screens, notification history, and theme
  switching
- Bounded notification history (16 entries) behind the result banner
- Temperature history sparklines (Dashboard) and fan percentage/raw
  history graphs (Fans); fan values are never RPM
- Named themes MSI Dark / Terminal / Light with runtime switching
- Persistent `~/.config/mec/config.toml` settings (refresh interval,
  theme, vim keys) with strict validation and atomic saves
- Responsive Full / Compact / Tiny modes with degraded-telemetry
  presentation
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

Hardware mutation is exposed through explicit CLI controls and through
confirmed TUI actions (same safe pipeline either way):

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

Profiles can be applied from the CLI or, after an explicit on-screen
confirmation, from the TUI Profiles screen. Both paths resolve through
the same safe transactional pipeline.

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
- profile transactions for the same hardware root are serialized with a
  Linux advisory lock held for the full apply/rollback lifetime, so two
  concurrent `mec profile apply` processes cannot interleave

### TUI configuration

TUI settings persist in:

```text
~/.config/mec/config.toml
```

```toml
refresh_interval_ms = 1000
theme = "msi-dark"
vim_keys = true
```

- `refresh_interval_ms` must be one of `500`, `1000`, `2000`, `5000`
- `theme` is one of `msi-dark`, `terminal`, `light` (`default` is
  accepted as an alias for `msi-dark` and normalized on save)
- `vim_keys = false` unmaps `h`/`j`/`k`/`l`; arrows, Tab, digits, `P`,
  `?`, and quit shortcuts always work
- unknown fields (including `mouse`, which MEC does not implement),
  wrong types, and malformed documents are rejected; a missing file
  loads defaults without creating anything; an invalid file falls back
  to defaults with a startup notice while monitoring still launches
- saves are atomic (temp file plus rename) with user-only permissions
- theme changes from the command palette persist across launches

### Privilege model

Control commands execute with the current process permissions. MEC performs
no sudo/pkexec/polkit/setuid escalation and ships no privileged helper: a
write the OS denies returns a typed failure (`AccessDenied` chain). The
interactive TUI runs unprivileged; supported `READY` controls execute only
after an explicit confirmation, while `READ-ONLY` compatibility mode
prohibits writes entirely. A dedicated privilege/packaging deployment
model remains future work.

Explicitly NOT completed yet:

- Automatic privileged helper / privilege deployment integration
- Physical validation of the remaining write paths (battery threshold,
  shift mode, fan mode, profile transaction, TUI mutation)
- Optional advanced fan-curve features not representable through the
  safe typed commands
- GUI, cloud, or telemetry features (out of v1 scope)

See the [design](docs/superpowers/specs/2026-09-18-mec-design.md) and
[implementation plan](docs/superpowers/plans/2026-09-18-mec-v1-implementation-plan.md).

## Planned hardware support

- Primary physical test target: MSI GF series laptops
- Other MSI laptops through capabilities exposed by the `msi-ec` kernel module

Read behavior has been manually smoke-tested on real MSI hardware.
Physical write validation has been completed on MSI GF63 Thin 11UC for
keyboard backlight, webcam, webcam block, and Cooler Boost; other write
paths remain covered by fake/temp sysfs integration tests and await
physical validation.

## Installation

Prebuilt `.deb`, `.rpm`, and portable `.tar.gz` artifacts (x86_64 and
ARM64) plus an AUR binary packaging workflow are documented in:

- [Installation](docs/installation.md)
- [Upgrade](docs/upgrade.md)
- [Uninstall](docs/uninstall.md)

## Building

```bash
cargo build
```

Rust `1.98.1` is pinned in `rust-toolchain.toml`.

## License

MIT — see [LICENSE](LICENSE).
