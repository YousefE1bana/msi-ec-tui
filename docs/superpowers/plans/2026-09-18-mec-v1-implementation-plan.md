# MEC v1 — Master Implementation Plan

> **For agentic workers:** Execute one task at a time. Never batch unrelated tasks. Every task must pass its stated tests before the next task starts. Do not redesign architecture during implementation without stopping and reporting the proposed change.

**Goal:** Build `mec`, a safe, capability-aware Rust TUI/CLI for controlling MSI laptops supported by the `msi-ec` Linux kernel interface.

**Repository:** `msi-ec-tui`

**Binary:** `mec`

**Architecture:** Frontends (`TUI`, `CLI`) consume an application core. The core communicates only through typed hardware interfaces. Linux sysfs access is isolated behind `EcBackend`; tests use fake sysfs trees and a mock backend. Arbitrary EC/register writes are forbidden.

**Tech Stack:** Rust 2024, Ratatui, Crossterm, Clap, Serde, TOML, thiserror, tracing.

**Initial toolchain:** Rust `1.98.1`.

**Spec:** `docs/superpowers/specs/2026-09-18-mec-design.md`

## Global Constraints

1. Linux only for v1.
2. MSI GF Series is the primary physical test target.
3. Other MSI laptops are supported through discovered `msi-ec` capabilities.
4. Never perform raw EC register access.
5. Never expose arbitrary sysfs path/value writes to UI or CLI.
6. Unsupported or inconsistent hardware must remain usable in read-only mode.
7. TUI code must never read or write sysfs directly.
8. CLI code must never read or write sysfs directly.
9. Profiles are data, never executable shell content.
10. A failed multi-setting profile application must support verified rollback.
11. `realtime_fan_speed` is represented as a percentage/raw EC fan value, not RPM.
12. The main TUI must remain usable without root privileges.
13. Hardware write privileges stay isolated behind the write boundary.
14. Tests must never require real MSI hardware.
15. Do not port the legacy Bash architecture.
16. No v1 process automation, temperature automation, RGB ecosystem, GUI, cloud, telemetry, or direct EC fallback.

---

# Plan Decomposition

```text
PLAN-001  Foundation + Hardware Core
          ↓
PLAN-002  Read-Only CLI + Monitoring
          ↓
PLAN-003  Read-Only TUI
          ↓
PLAN-004  Safe Hardware Writes
          ↓
PLAN-005  Profiles + Transactions
          ↓
PLAN-006  Full TUI UX + Diagnostics
          ↓
PLAN-007  Packaging + Release
          ↓
        v1.0
```

Each plan gets its own implementation checkpoint.

Do not ask an agent to implement all seven plans in one run.

---

# PLAN-001 — Foundation + Hardware Core

## Completion Gate

At completion:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

must all pass.

Additionally:

```bash
cargo run -- doctor
```

must be able to inspect a fake sysfs tree without requiring MSI hardware.

No hardware writes exist at this stage.

---

## Task 1 — Bootstrap the Rust Project

### Files

Create:

```text
Cargo.toml
rust-toolchain.toml
src/main.rs
src/lib.rs
.gitignore
LICENSE
README.md
SECURITY.md
CONTRIBUTING.md
```

### Required package identity

```toml
[package]
name = "mec"
version = "0.1.0"
edition = "2024"
rust-version = "1.98"
license = "MIT"
description = "A safe capability-aware terminal control center for MSI laptops on Linux"
```

### Add dependencies

```bash
cargo add clap --features derive
cargo add serde --features derive
cargo add serde_json
cargo add toml
cargo add thiserror
cargo add tracing
cargo add tracing-subscriber
cargo add directories

cargo add --dev tempfile
cargo add --dev assert_cmd
cargo add --dev predicates
```

Do not add Ratatui yet.

### `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.98.1"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

### Minimal executable

```rust
fn main() {
    println!("MEC — MSI EC Control Center");
}
```

### Verify

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run
```

Expected output:

```text
MEC — MSI EC Control Center
```

### Commit

```bash
git add .
git commit -m "chore: bootstrap MEC Rust project"
```

---

# Task 2 — Define Hardware Domain Types

Create:

```text
src/hardware/mod.rs
src/hardware/backend.rs
src/hardware/device.rs
src/hardware/values.rs
src/hardware/capabilities.rs
```

Implement an initially read-only `EcBackend`, typed `DeviceInfo`, dynamic validated `FanMode` and `ShiftMode` newtypes, `TemperatureCelsius`, `FanPercent`, and `HardwareSnapshot`.

Validation requirements:

```text
Fan/shift mode:
- non-empty
- trimmed
- no newline
- no NUL
- maximum 64 bytes

Temperature: 0..=100
Fan raw percentage: 0..=150
```

`FanPercent` must never be represented as RPM.

Tests must cover invalid values and injection-like malformed mode strings.

Commit:

```bash
git commit -m "feat: define hardware domain model"
```

---

# Task 3 — Introduce System Path Abstraction

Create:

```text
src/hardware/paths.rs
```

Use a configurable root so tests can map Linux paths into a fixture tree.

Production root:

```text
/
```

Test example:

```text
/tmp/fake-root
```

Centralize access to:

```text
/sys/class/dmi/id/
/sys/devices/platform/msi-ec/
/sys/class/power_supply/
/sys/class/leds/
```

Reject unsafe path components containing traversal or separators.

Commit:

```bash
git commit -m "feat: add testable system path abstraction"
```

---

# Task 4 — Implement Safe Sysfs Reading

Create:

```text
src/hardware/sysfs.rs
```

Implement typed sysfs reading for strings and integers, max 4096-byte reads, typed errors, invalid UTF-8 detection, and directory listing.

Tests:

```text
normal string
trailing newline
integer
missing file
invalid integer
oversized input
```

Commit:

```bash
git commit -m "feat: add safe sysfs reader"
```

---

# Task 5 — Detect MSI Device Identity

Create:

```text
src/hardware/detector.rs
tests/device_detection.rs
```

Read Linux DMI identity and optional `msi-ec/fw_version`.

Accept common MSI manufacturer forms.

Never infer support from the letters `GF` in the product name.

Test:

```text
MSI GF63
MSI non-GF laptop
non-MSI machine
missing optional fields
```

Commit:

```bash
git commit -m "feat: detect MSI device identity"
```

---

# Task 6 — Discover `msi-ec` Capabilities

Implement capability discovery using actual sysfs presence and the upstream available-mode files.

Do not synthesize modes.

Discover:

```text
CPU/GPU temperature
CPU/GPU fan values
fan modes
shift modes
cooler boost
super battery
webcam
webcam block
fn key
win key
battery thresholds
keyboard backlight
```

Unknown future valid mode names must be preserved.

Commit:

```bash
git commit -m "feat: discover MSI EC capabilities dynamically"
```

---

# Task 7 — Define Compatibility / Read-Only State

Create:

```text
src/hardware/support.rs
tests/support_mode.rs
```

Model:

```rust
pub enum SupportMode {
    Ready,
    ReadOnly(ReadOnlyReason),
}
```

Reasons include:

```text
NonMsiHardware
MsiEcUnavailable
MsiEcUnreadable
InconsistentInterface
```

Missing optional features are not failures.

A coherent unknown MSI model can be READY.

Commit:

```bash
git commit -m "feat: add conservative compatibility state"
```

---

# Task 8 — Implement Read-Only Hardware Snapshot

Create:

```text
src/hardware/msi_ec.rs
tests/snapshot.rs
```

Implement read-only backend snapshot collection.

Parse booleans strictly from expected backend values.

Fan values are percentage/raw values, not RPM.

Commit:

```bash
git commit -m "feat: implement read-only MSI EC backend"
```

---

# Task 9 — Build Reusable Fake Sysfs Fixtures

Create:

```text
tests/fixtures/gf63/
tests/fixtures/partial-device/
tests/fixtures/unknown-device/
tests/fixtures/broken-sysfs/
```

Representative full fixture includes DMI, `msi-ec`, LEDs, and battery threshold nodes.

Example values:

```text
CPU temperature = 63
GPU temperature = 51
CPU fan = 42
GPU fan = 31

fan_mode = auto
shift_mode = comfort
```

Fixtures describe ABI shapes, not laptop-specific hacks.

Commit:

```bash
git commit -m "test: add reusable MSI sysfs fixtures"
```

---

# Task 10 — Add the First CLI Diagnostic Surface

Create:

```text
src/cli/mod.rs
src/cli/args.rs
src/diagnostics/mod.rs
src/diagnostics/doctor.rs
```

Commands:

```bash
mec doctor
mec --sys-root ./tests/fixtures/gf63 doctor
```

Expected fixture output contains:

```text
MEC Doctor
Mode: READY
```

Broken fixture must report READ-ONLY.

Use `assert_cmd` for integration tests.

Commit:

```bash
git commit -m "feat: add MEC doctor command"
```

---

# CHECKPOINT A — PLAN-001 Gate

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo run -- --sys-root tests/fixtures/gf63 doctor
cargo run -- --sys-root tests/fixtures/broken-sysfs doctor
```

Expected:

```text
Formatting: PASS
Clippy:     PASS
Tests:      PASS

GF fixture:      READY
Broken fixture:  READ-ONLY
```

Check that sysfs paths are centralized, fan telemetry is never labeled RPM, and no hardware mutation implementation exists yet.

Stop after Checkpoint A.

---

# PLAN-002 — Read-Only CLI + Monitoring

Goal:

```text
mec status
mec monitor
mec status --json
```

Scope:

- HardwareSnapshot presentation
- battery state
- bounded monitoring history
- poll interval configuration
- human-readable status
- machine-readable JSON
- graceful error degradation
- Ctrl+C shutdown

No writes.

---

# PLAN-003 — Read-Only TUI

Add Ratatui and Crossterm only now.

Implement:

- AppState
- event loop
- Dashboard
- Performance
- Fans
- Battery
- Devices
- Diagnostics
- semantic themes
- keyboard navigation
- Vim navigation
- help overlay
- responsive layouts
- rendering tests

Deliverable:

```bash
mec
```

launches a read-only TUI.

---

# PLAN-004 — Safe Hardware Writes

Introduce typed `HardwareCommand`, command validation, privilege boundary, validated sysfs writing, and readback verification.

Every write follows:

```text
Capability check
      ↓
Value validation
      ↓
Privilege boundary
      ↓
Write
      ↓
Readback
      ↓
Verified result
```

No arbitrary generic path/value interface may escape the backend layer.

---

# PLAN-005 — Profiles + Transactions

Implement:

- TOML profile schema
- parser
- validation
- built-in presets
- previews
- transaction plans
- snapshot before apply
- ordered execution
- readback verification
- rollback
- rollback verification

Built-ins:

```text
Balanced
Silent
Gaming
Battery Saver
Maximum Cooling
```

---

# PLAN-006 — Full TUI UX + Diagnostics

Implement:

- profiles screen
- editable controls
- confirmation dialogs
- command palette
- notifications
- temperature sparklines
- fan history graph
- MSI Dark
- Terminal
- Light
- diagnostics export
- privacy review
- compatibility report

---

# PLAN-007 — Packaging + Release

Build:

- GitHub Actions
- release binaries
- SHA256 checksums
- `.deb`
- `.rpm`
- AUR packaging
- installation docs
- upgrade docs
- uninstall docs

Targets:

```text
x86_64-unknown-linux-gnu
aarch64-unknown-linux-gnu
```

Required CI:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build --release
```

---

# Version Milestones

```text
v0.1.0  Hardware foundation + doctor
v0.2.0  Read-only CLI + monitoring
v0.3.0  Read-only TUI
v0.4.0  Safe controls
v0.5.0  Profiles + transactions
v0.9.0  Feature-complete release candidate
v1.0.0  Public stable release
```

---

# OpenCode Execution Policy

One OpenCode session should normally receive one task plus only the relevant interfaces, tests, and completion gate.

Never prompt:

```text
Build MEC v1 from this entire spec.
```

Every implementation task ends with:

```text
1. Run targeted tests.
2. Run cargo fmt.
3. Run cargo clippy.
4. Show changed files.
5. Explain deviations.
6. Commit only when all gates pass.
7. Stop.
```

---

# Development Branch Strategy

```text
main
feat/001-hardware-core
feat/002-monitoring-cli
feat/003-readonly-tui
feat/004-hardware-controls
feat/005-profiles
feat/006-tui-polish
feat/007-release
```

Prefer squash merges into `main`.

The first implementation target is:

```text
PLAN-001
Task 1
```

Do not begin TUI implementation before Checkpoint A is green.
