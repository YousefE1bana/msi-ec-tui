# Physical validation — v1.1 candidate

Validated on 2026-10-07 using the production release binary, real hardware
telemetry and existing typed commands. No prototype state was used.

Device: **MSI GF63 Thin 11UC**, EC **16R6EMS1.107**. Host: Ubuntu 26.04.1
x86_64, KDE Wayland, kernel 7.0.0-31-generic, upstream msi-ec 0.13 via DKMS.
Secure Boot was enabled with the existing signed driver. This pass did not
install a driver, change signing, enroll a key, or change hardware permissions.

## Results

The owner interactively authenticated a controlled QA terminal. Keyboard key
sequences and SGR mouse events entered the actual Crossterm production TUI in a
PTY. Before final confirmation, the exposed state was compared with its dynamic
baseline. After Apply, both MEC's production status and the underlying exposed
state were checked. Restoration used MEC typed CLI commands, never arbitrary
sysfs writes. A guarded cleanup restored the captured values on exit/failure.

| Control | Captured baseline → test target | Keyboard | Mouse | Restored |
|---|---|---|---|---|
| Shift Mode | comfort → turbo | PASS | PASS | comfort |
| Fan Mode | auto → silent | PASS | PASS | auto |
| Cooler Boost | Off → On | PASS | PASS | Off |
| Battery end threshold | 60 → 70; start 50 → 60 | PASS | PASS | start 50 / end 60 |
| Webcam | Off → On | PASS | PASS | Off |
| Webcam Block | Off → On | PASS | PASS | Off |
| Keyboard Backlight | level 3 → 0 | PASS | PASS | 3 |
| Silent profile | comfort/auto → eco/silent; Boost remains Off | PASS | PASS | full baseline |
| Review → Cancel | selected Silent, opened review, canceled | PASS | PASS | zero changes |

Each individual control's staging and review produced **zero exposed hardware
changes**. Profile selection and review also produced zero changes. Only the
explicit final Apply authorized execution. Both input methods converged on
`TuiApp::execute_pending` and the existing executor. Profile application retained
`SafeTuiExecutor → safety::apply_profile → transaction/preflight/rollback`.

Final independent comparison of all nine restoration fields matched the first
captured baseline exactly: comfort, auto, Boost Off, Super Battery Off, battery
50/60, Webcam Off, Webcam Block Off, Backlight 3. These numbers record this test;
they must never be reused as another machine's restoration targets. Battery
percentage/temperature change naturally and are not restoration targets.

## Observations and limits

On this laptop, applying Silent's eco Shift Mode also made **Super Battery read
On**. Silent requests Shift Mode, Fan Mode and Cooler Boost only; its review
correctly showed two changes and one unchanged value, with no Super Battery
command. The installed driver was inspected without modification. Firmware or
hardware coupling is a possible explanation, not a proven cause. Super Battery
was explicitly restored to its captured Off state. Owners should inspect the
complete current state after applying a profile; this observation must be
considered when using eco mode; it is disclosed in the v1.1.0 release notes.

Cooler Boost passed state/readback checks. Fan telemetry is percentage/raw data,
not RPM, and does not prove acoustic response or airflow. This pass makes no
sensory confirmation claim. Fn/Win Key rows remain informational where no typed
mutation exists. No physical rollback failure injection was attempted; controlled
automated tests continue covering transaction failure and rollback.

`--version`, doctor, privacy-conscious doctor export, status, JSON status and a
three-sample monitor run succeeded on the real device. All eight screens, Help,
palette, review and About were exercised. READ-ONLY and missing/degraded states
were checked through existing fixtures rather than fabricating a physical mode.
Success results were observed after the real applications. An unprivileged
Webcam Apply was rejected at the real write boundary with an access-denied
result; the underlying value was unchanged and the failure notice expired.
Persistent errors also have automated coverage.

The private QA harness initially selected a no-op Balanced profile and later
matched the word Apply in a review table instead of the explicit button. Both
runs stopped without an unintended application and restored the baseline. A
focused final runner verified Silent selection, exact Apply-button targeting,
Cancel and restoration. Those harness failures are not counted as product passes.

## Visual evidence

[Production screenshot gallery](screenshots.md) contains actual window captures
using the installed KDE Spectacle utility, production MEC and live telemetry.
Screenshots were checked at 160×50, 120×35, 100×30 and 80×24. Fixture renders are
used only in tests; they are not presented as physical screenshots. No raw EC
register access, ec_sys, helper execution or permission bypass was added.
