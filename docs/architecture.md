# Architecture and safety

Production has one navigation/action/edit/pending model. Rendering consumes real
LiveHardware snapshots/history, support, capabilities, config, profiles, and
results; it never executes writes. Mouse hit testing shares renderer geometry.
Mouse and keyboard converge on AppAction and the same editor/review state.
Only explicit confirmation consumes pending intent once.

Typed HardwareCommand intent reaches SafeTuiExecutor and the existing safety
executor: fresh support and capability validation, restricted write boundary,
serialization, mandatory readback, and bounded EC settling verification.
Profile intent reaches safety::apply_profile and its existing preflight,
transaction, verified application, and rollback. UI previews reuse production
planners; they do not duplicate transaction rules or track a fictional active
profile. CLI mutation commands use the same safe core without a TUI dialog.

Config persists display/input preferences only. About is a utility section in
AppState; it consumes no primary numeric slot. The update worker has no hardware
or executor handles, uses a fixed HTTPS endpoint, rejects redirects, limits the
response to 64 KiB and total request to five seconds, and starts only after an
explicit request. Package version metadata is authoritative for CLI and About.

The original project identity is preserved here as heritage:

```text
 __  __ ____ ___   _____ ____    ____            _             _
|  \/  / ___|_ _| | ____/ ___|  / ___|___  _ __ | |_ _ __ ___ | |
| |\/| \___ \| |  |  _|| |     | |   / _ \| '_ \| __| '__/ _ \| |
| |  | |___) | |  | |__| |___  | |__| (_) | | | | |_| | | (_) | |
|_|  |_|____/___| |_____\____|  \____\___/|_| |_|\__|_|  \___/|_|
```
