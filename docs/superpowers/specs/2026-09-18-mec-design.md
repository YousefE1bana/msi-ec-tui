# MEC — MSI EC Control Center for Linux

## Design Specification v1

**Project type:** Open-source Linux TUI  
**Primary backend:** `msi-ec` Linux kernel module  
**Primary tested hardware:** MSI GF Series  
**Supported scope:** Any MSI laptop correctly supported by `msi-ec`  
**Language:** Rust  
**UI:** Ratatui + Crossterm  
**License target:** MIT

---

## 1. Product Goal

MEC is a fast, safe, terminal-native control center for MSI laptops running Linux.

It provides hardware monitoring and control through a modern interactive TUI while avoiding assumptions about what features a particular MSI laptop supports.

The application must:

- Work across Linux distributions.
- Detect the laptop and available EC capabilities automatically.
- Expose only supported controls.
- Never blindly write unsupported EC values.
- Provide an excellent keyboard-driven TUI.
- Remain usable from scripts through a CLI.
- Support MSI GF laptops as the primary development/test target.
- Support other MSI laptops whenever the upstream `msi-ec` interface exposes compatible capabilities.

MEC is not tied to one GF63 firmware or one laptop configuration.

---

# 2. Core Principles

## Capability-first

Never assume:

`MSI laptop → feature exists`

Instead:

`detected sysfs capabilities → available MEC features`

Example:

```text
MSI GF63 Thin 11UC

CPU Temperature        Supported
GPU Temperature        Supported
Fan Mode               Supported
Shift Mode             Supported
Cooler Boost           Supported
Battery Threshold      Supported
Keyboard Backlight     Supported
Webcam                  Supported
RGB Keyboard            Unsupported
```

Unsupported controls do not generate broken buttons or arbitrary writes.

## Safe by default

Unknown or suspicious hardware states default to:

```text
READ-ONLY MODE
```

Monitoring remains available.

Hardware modification requires MEC to establish that the required sysfs interface exists and accepts the intended capability.

## TUI-first, core-independent

The hardware layer must not know anything about Ratatui.

```text
TUI ───────────┐
               │
CLI ───────────┼── Application Core
               │          │
Automation ────┘          │
                          ▼
                  Hardware Interface
                          │
                    msi-ec backend
                          │
                        sysfs
```

This allows future GUI, daemon, Waybar integration, or external APIs without rewriting hardware code.

---

# 3. High-Level Architecture

```text
┌──────────────────────────────────────────────┐
│                  Frontends                   │
│                                              │
│        Interactive TUI       CLI             │
└──────────────────────┬───────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────┐
│               Application Core               │
│                                              │
│ state • commands • profiles • events        │
└──────────────────────┬───────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────┐
│             Capability / Safety Layer        │
│                                              │
│ detection • validation • transactions       │
└──────────────────────┬───────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────┐
│                 EC Backend                   │
│                                              │
│ RealSysfsBackend       MockSysfsBackend      │
└──────────────────────┬───────────────────────┘
                       │
                       ▼
             Linux msi-ec / sysfs
```

---

# 4. Repository Structure

```text
mec/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── CONTRIBUTING.md
├── SECURITY.md
│
├── src/
│   ├── main.rs
│   ├── app/
│   │   ├── mod.rs
│   │   ├── state.rs
│   │   ├── action.rs
│   │   └── event.rs
│   ├── hardware/
│   │   ├── mod.rs
│   │   ├── backend.rs
│   │   ├── sysfs.rs
│   │   ├── detector.rs
│   │   ├── capabilities.rs
│   │   ├── device.rs
│   │   └── values.rs
│   ├── safety/
│   │   ├── mod.rs
│   │   ├── validation.rs
│   │   └── transaction.rs
│   ├── monitoring/
│   │   ├── mod.rs
│   │   ├── sensors.rs
│   │   └── history.rs
│   ├── profiles/
│   │   ├── mod.rs
│   │   ├── profile.rs
│   │   ├── presets.rs
│   │   └── storage.rs
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── ui.rs
│   │   ├── theme.rs
│   │   ├── widgets/
│   │   └── screens/
│   │       ├── dashboard.rs
│   │       ├── performance.rs
│   │       ├── fans.rs
│   │       ├── battery.rs
│   │       ├── devices.rs
│   │       ├── profiles.rs
│   │       └── diagnostics.rs
│   ├── cli/
│   │   ├── mod.rs
│   │   └── commands.rs
│   ├── config/
│   │   ├── mod.rs
│   │   └── settings.rs
│   └── diagnostics/
│       ├── mod.rs
│       └── report.rs
├── tests/
│   ├── fixtures/
│   │   ├── gf63/
│   │   ├── partial-device/
│   │   ├── unknown-device/
│   │   └── broken-sysfs/
│   ├── capability_tests.rs
│   ├── sysfs_tests.rs
│   ├── profile_tests.rs
│   └── safety_tests.rs
├── docs/
│   ├── architecture.md
│   ├── hardware-support.md
│   └── development.md
└── packaging/
    ├── deb/
    ├── rpm/
    └── arch/
```

---

# 5. Hardware Backend

```rust
trait EcBackend {
    fn detect_device(&self) -> Result<DeviceInfo>;
    fn capabilities(&self) -> Result<Capabilities>;
    fn snapshot(&self) -> Result<HardwareSnapshot>;
    fn execute(&self, command: HardwareCommand) -> Result<()>;
}
```

Production: `RealSysfsBackend`  
Tests: `MockSysfsBackend`

No application component writes arbitrary paths directly.

---

# 6. Hardware Commands

```rust
enum HardwareCommand {
    SetFanMode(FanMode),
    SetShiftMode(ShiftMode),
    SetCoolerBoost(bool),
    SetSuperBattery(bool),
    SetWebcam(bool),
    SetWebcamBlock(bool),
    SetKeyboardBacklight(u8),
    SetBatteryThreshold(BatteryThreshold),
}
```

Never expose an internal operation equivalent to:

```text
write(path, arbitrary_string)
```

from the UI.

---

# 7. Capability Detection

At startup MEC inspects:

```text
/sys/devices/platform/msi-ec/
```

plus relevant:

```text
/sys/class/power_supply/
/sys/class/leds/
```

Capabilities are constructed dynamically.

```rust
struct Capabilities {
    cpu_temperature: bool,
    gpu_temperature: bool,
    cpu_fan: bool,
    gpu_fan: bool,
    fan_modes: Vec<FanMode>,
    shift_modes: Vec<ShiftMode>,
    cooler_boost: bool,
    super_battery: bool,
    webcam: bool,
    webcam_block: bool,
    fn_key: bool,
    win_key: bool,
    keyboard_backlight: Option<BacklightCapability>,
    battery_thresholds: bool,
}
```

The UI consumes this structure rather than checking sysfs itself.

---

# 8. Dashboard

```text
╭─ MEC ─ MSI EC Control Center ────────────────────────────────╮
│ GF63 Thin 11UC             Linux 6.x        ● EC Connected  │
├────────────────────────────┬─────────────────────────────────┤
│ THERMALS                   │ PERFORMANCE                     │
│                            │                                 │
│ CPU  63°C ███████░░ ↗      │ Shift        Comfort            │
│ GPU  51°C █████░░░░ →      │ Fan          Auto               │
│                            │ Cooler Boost ○ Off              │
│ CPU Fan  42%               │ Super Battery ○ Off            │
│ GPU Fan  31%               │                                 │
├────────────────────────────┼─────────────────────────────────┤
│ BATTERY                    │ DEVICE                          │
│ 77% • Charging             │ Webcam       ● Enabled          │
│ Charge limit 80%           │ Backlight    Level 2            │
│ AC Connected               │ Fn / Win     Standard           │
├────────────────────────────┴─────────────────────────────────┤
│ CPU ▁▂▂▃▄▅▅▄▃▂      GPU ▁▁▂▂▃▄▄▃▂                      │
├──────────────────────────────────────────────────────────────┤
│ F1 Help  P Profiles  F Fans  B Battery  D Doctor  Q Quit    │
╰──────────────────────────────────────────────────────────────╯
```

---

# 9. Navigation

Primary:

```text
↑ ↓ ← →
Enter
Esc
Tab
```

Power users:

```text
h j k l
```

Global shortcuts:

```text
1 Dashboard
2 Performance
3 Fans
4 Battery
5 Devices
6 Profiles
7 Diagnostics

P Command Palette
? Help
Q Quit
```

Mouse support is optional and must never be required.

---

# 10. Performance Control

Expose only supported modes reported by the driver.

Potential shift modes:

```text
Eco
Comfort
Sport
Turbo
```

Potential fan modes:

```text
Auto
Silent
Basic
Advanced
```

MEC must not hard-code their availability.

---

# 11. Fan Management

V1:

- Current mode.
- CPU/GPU fan telemetry.
- Mode switching.
- Cooler Boost.
- Historical fan graph.

Future capability:

- Basic fan speed.
- Advanced fan curve editing.

Fan-curve support must only appear when the backend can safely represent it.

---

# 12. Battery

Battery screen includes:

- Charge percentage.
- AC status.
- Charging state.
- Charge start threshold.
- Charge end threshold.

Presets:

```text
Maximum Lifespan
Balanced
Full Charge
Custom
```

Actual values must respect the interface exposed by the device rather than assuming all firmware uses identical presets.

For the v1 `msi-ec` backend, the two charge-threshold files describe one EC
charge-control state with a fixed 10-percentage-point hysteresis:
`end == start + 10`, with start in `0..=90` and end in `10..=100`.
`BatteryThreshold` is valid by construction only for such pairs; the backend
rejects any other coherent-looking pair as invalid data. This models the
`msi-ec` contract, not every Linux power-supply driver.

---

# 13. Profiles

Profiles use declarative TOML.

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

Profiles are parsed strictly as data.

They are never executed or sourced as shell code.

---

# 14. Built-in Presets

```text
Balanced
Silent
Gaming
Battery Saver
Maximum Cooling
```

Presets remain capability-aware.

---

# 15. Transactional Profile Application

```text
1. Read current state.
2. Validate all requested changes.
3. Create rollback snapshot.
4. Apply changes.
5. Verify results.
6. Roll back safely when appropriate if application fails.
```

Preview:

```text
Gaming

Fan Mode       Auto       → Advanced
Shift Mode     Comfort    → Turbo
Cooler Boost   Off        → On
Battery Limit  80%        → unchanged

Apply? [Enter]
```

---

# 16. Monitoring

Default polling: `1 second`

Configurable:

```text
500 ms
1 s
2 s
5 s
```

Avoid unnecessarily aggressive EC polling.

Maintain bounded in-memory history for graphs.

V1 does not require a database.

---

# 17. Diagnostics

```bash
mec doctor
```

Example:

```text
MEC Doctor

[PASS] MSI laptop detected
[PASS] Linux environment supported
[PASS] msi-ec detected
[PASS] EC sysfs readable
[PASS] Fan controls available
[PASS] Shift controls available
[PASS] Battery thresholds available
[WARN] GPU temperature unavailable

Mode: READY
```

Export:

```bash
mec doctor --export
```

Produces a privacy-conscious compatibility report suitable for GitHub issues.

---

# 18. Unknown Hardware

```text
╭─ Compatibility Warning ─────────────────────────╮
│                                                │
│ MEC cannot safely validate all write features. │
│                                                │
│ READ-ONLY MODE has been enabled.               │
│                                                │
│ Monitoring remains available.                  │
│ No EC changes will be made.                    │
│                                                │
│ [E] Export compatibility report                │
╰────────────────────────────────────────────────╯
```

There must be no hidden automatic fallback to arbitrary EC register writes.

---

# 19. Privilege Model

Long-term target:

```text
unprivileged MEC
      │
      ▼
small privileged interface
      │
      ▼
validated sysfs writes
```

Do not make the entire TUI fundamentally depend on running as root.

V1 may initially use a narrowly scoped privilege elevation mechanism while the privileged helper architecture is completed.

---

# 20. CLI

```bash
mec
mec status
mec doctor
mec profile list
mec profile show gaming
mec profile apply gaming
mec fan mode auto
mec shift turbo
mec cooler-boost on
mec battery status
mec battery limit 80
```

Future machine-readable output:

```bash
mec status --json
```

---

# 21. Configuration

```text
~/.config/mec/config.toml
~/.config/mec/profiles/
```

Example:

```toml
refresh_interval_ms = 1000
theme = "default"
vim_keys = true
mouse = true
```

---

# 22. Themes

```text
MSI Dark
Terminal
Light
```

Semantic roles:

```text
background
foreground
primary
secondary
success
warning
danger
muted
border
```

---

# 23. Testing

Fake sysfs trees represent device configurations:

```text
tests/fixtures/gf63/
tests/fixtures/partial-device/
tests/fixtures/unsupported-device/
tests/fixtures/read-only-device/
tests/fixtures/broken-values/
```

Tests cover detection, capabilities, parsing, missing files, unexpected values, read-only files, unsupported modes, profiles, transactions, rollback, and diagnostics.

---

# 24. GF Series Strategy

GF is the first-class target.

Initial physical validation should prioritize:

```text
GF63 Thin
GF65 Thin
GF66 / Katana-family devices where compatible
```

Product logic must not special-case GF63 unless a documented compatibility quirk genuinely requires it.

Compatibility comes primarily from capabilities.

---

# 25. Distribution

Target artifacts:

```text
mec-x86_64-unknown-linux-gnu.tar.gz
mec-aarch64-unknown-linux-gnu.tar.gz

.deb
.rpm

AUR package
```

Users should not need Rust installed to use release binaries.

---

# 26. CI

```text
cargo fmt --check
cargo clippy
cargo test
cargo build --release

fixture/integration tests
release packaging
checksums
```

No release is considered valid when core safety tests fail.

---

# 27. Scope for v1.0

Included:

```text
Hardware detection
Capability discovery
Read-only fallback
CPU/GPU telemetry
Fan telemetry
Fan modes
Shift modes
Cooler Boost
Super Battery
Battery thresholds
Webcam controls
Fn/Win controls
Keyboard backlight where exposed
Dashboard
Graphs
Profiles
Built-in presets
Diagnostics
CLI
TUI
Configuration
Mock hardware backend
Tests
Linux release binaries
```

---

# 28. Explicitly Deferred

```text
Direct raw EC register access
Unknown-firmware experimentation
Custom kernel driver fork
GUI
Cloud services
Telemetry collection
Remote control
RGB ecosystem integration
Game/process automation
Automatic temperature rules
Plugin marketplace
```

---

# 29. v1.1 Candidate Features

```text
AC ↔ Battery automatic profiles
Process-aware profiles
Thermal rules
Automatic restoration of previous profile
Waybar integration
Shell completion
JSON output
```

---

# 30. Definition of Success

MEC v1 succeeds when a Linux user with a compatible MSI laptop can install MEC, run it, have hardware detected automatically, see exactly which controls are supported, monitor hardware safely, change supported settings, use profiles, and diagnose compatibility issues without manually editing sysfs.

---

# 31. Development Rule

The legacy repository is reference material only.

Reuse:

- Proven feature concepts.
- sysfs knowledge.
- user-facing terminology where still correct.

Rebuild:

- Architecture.
- hardware abstraction.
- validation.
- UI.
- profiles.
- testing.
- diagnostics.
- packaging.

---

# 32. Project Identity

Repository:

```text
msi-ec-tui
```

Product:

```text
MEC
MSI EC Control Center for Linux
```

CLI binary:

```text
mec
```

Tagline:

> A safe, fast, capability-aware terminal control center for MSI laptops on Linux.
