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

MEC inspects DMI identity, discovered msi-ec capabilities, live telemetry,
battery thresholds, and keyboard backlight interfaces. Supported typed mutations
use fresh support/capability validation, a restricted sysfs write boundary,
mandatory readback with bounded EC settling retries, and serialization.
Profiles use preflight, transactional application, and rollback.

MEC does not implement raw EC access, arbitrary path/value writes, a privileged
helper, shell execution from profiles, or automatic privilege escalation.
The TUI uses current-process permissions. READY means supported hardware;
it does not grant OS write permission. Unknown or inconsistent hardware fails
closed to READ-ONLY. Mouse and keyboard converge at the same confirmation flow.

Security-sensitive areas include path injection, capability/support bypasses,
readback or rollback regressions, privilege boundaries, and untrusted profile,
configuration, or release metadata. Update discovery is opt-in, TLS verified,
size/time bounded, and sends no hardware telemetry. The installer verifies the
exact selected SHA256SUMS entry. Checksums provide integrity, not publisher
signature authentication; GitHub release access remains a trust dependency.

## Hardening principles

- Unsupported or inconsistent hardware defaults to read-only mode.
- No raw EC register access, ever.
- Profiles and configuration are parsed strictly as data and never executed.
- Writes go through a validated, capability-checked, verified write boundary.
