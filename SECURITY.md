# Security Policy

## Reporting a vulnerability

Do **not** open a public GitHub issue for security vulnerabilities.

Report privately by email:

- Yousef Osama <y3usef.osama@gmail.com>

Include a description of the issue, the affected version or commit, and steps to
reproduce if possible. You will receive an initial response as soon as practical.

## Supported versions

Only the latest release of MEC receives security fixes.

## Scope

The current implementation performs READ-ONLY inspection of:

- DMI identity (`sys_vendor`, `product_name`, and related fields)
- `msi-ec`/sysfs capability interfaces (presence and available-mode lists)
- Supported telemetry/state nodes (temperatures, fan values, modes, booleans)
- Linux class interfaces used for battery thresholds and keyboard backlight

The current code has:

- no hardware write implementation
- no raw ECebug register access
- no privileged write helper
- no profile-apply execution

Future security-sensitive areas include:

- Any path that allows writing values to sysfs paths MEC has not validated as
  supported by the detected hardware.
- Any bypass of capability checks, read-only mode, or the transactional
  apply/rollback flow.
- Arbitrary path or value injection through CLI arguments, profile files, or
  configuration files.
- Privilege boundary issues (MEC aims to keep the TUI usable unprivileged).

## Hardening principles

- Unsupported or inconsistent hardware defaults to read-only mode.
- No raw EC register access, ever.
- Profiles and configuration are parsed strictly as data and never executed.
- Writes go through a validated, capability-checked, verified write boundary.
