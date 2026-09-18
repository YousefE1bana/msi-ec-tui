# MEC Sysfs Fixtures

Reusable fake system roots for tests and (from Task 10) `mec doctor --sys-root`.
Each directory behaves as a fake `/`, so hardware paths live under `sys/`.
Use via `SystemPaths::new("tests/fixtures/<name>")`.

Fixtures describe sysfs ABI shapes only — never model-specific support rules.

- `gf63` — full coherent MSI GF63-style interface (`SupportMode::Ready`).
- `partial-device` — coherent subset; missing optionals stay unsupported (`Ready`).
- `unknown-device` — unknown MSI model with coherent capabilities, including a
  future-valid fan mode (`Ready` without any model allowlist).
- `broken-sysfs` — valid MSI identity but a half-paired `fan_mode` interface,
  intentionally `ReadOnly(InconsistentInterface)`.
