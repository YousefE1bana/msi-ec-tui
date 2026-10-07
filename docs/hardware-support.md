# Hardware support and driver setup

MEC discovers the Linux msi-ec interface and checks the detected device/EC,
advertised modes, controls, and telemetry. Unsupported or inconsistent hardware
fails closed. READY does not mean every possible control exists or that the
current user can write it. Fn/Win key entries are informational where no typed
mutation exists. Battery health, cycles, voltage, wear, RPM, and an active-profile
identity are not fabricated.

The physical validation target is MSI GF63 Thin 11UC / EC 16R6EMS1.107, with
upstream msi-ec 0.13 via DKMS. Other MSI models need their own support evaluation
and testing; package compatibility alone proves no hardware support.

If `/sys/devices/platform/msi-ec` is missing, run `mec doctor` and inspect your
kernel module availability. Follow the official
[BeardOverflow/msi-ec instructions](https://github.com/BeardOverflow/msi-ec) for
its current supported hardware and DKMS procedure. Use your distro's package
manager for build tools, DKMS, and **matching running-kernel headers**:
Ubuntu/Debian uses apt; Fedora uses dnf and kernel-devel; Arch uses pacman and
matching linux-headers; openSUSE uses zypper and matching kernel development
packages. Package names/kernel variants differ; check your distro documentation.
No single unattended cross-distro kernel installer is provided.

With Secure Boot, use your distro's module-signing/MOK enrollment process.
Enrollment may require an interactive firmware screen and reboot. Do not bypass
signing by disabling security. After reboot, check the module/interface and rerun
`mec doctor`. Existing working DKMS installations need no replacement.

For physical tests and restoration evidence, see [physical validation](physical-validation.md).
