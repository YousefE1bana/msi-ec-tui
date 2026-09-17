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

The current bootstrap only prints a banner; it does not access hardware.
MEC is planned to interact with embedded-controller state exposed through the
`msi-ec` kernel interface. Future security-sensitive areas include:

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
