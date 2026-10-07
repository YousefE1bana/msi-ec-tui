# Troubleshooting and privileges

Run `mec doctor` and `mec status` first. Review `mec doctor --export` before
sharing; it omits arbitrary paths/private machine identifiers by design.
N/A means unavailable telemetry; DEGRADED means the latest sample failed.
READ-ONLY is a real support verdict, not a demo switch.

READY hardware may still reject writes because Linux owns sysfs nodes as root.
MEC uses current-process permissions and has no automatic elevation. The desktop
launcher stays unprivileged. An administrator can deliberately launch the
trusted built/installed MEC binary in a terminal for a controlled session; this
is explicit full-process privilege and not a least-privilege deployment solution.
Always review a requested change, record its baseline, and restore after QA.

No world-writable chmod, setuid binary, password storage, or generic privileged
path/value helper is recommended. A production least-privilege group/udev/helper
policy needs a separate reviewed checkpoint: sysfs permissions and recreation
vary by driver/kernel, and a rule must not authorize unrelated controls. This
pass deliberately ships no permission-changing rules or services.

If colors look monochrome, check whether `NO_COLOR` is set and whether your
terminal supports true color. Candidate themes use the same semantic status
colors. Normal UI never uses reverse-video selection.

Update checking is explicit in Settings → About or `mec update-check`. Offline,
rate-limit, timeout, or invalid-response errors affect only the update card;
monitoring and control remain independent. No update is automatically installed.

## Missing or unsupported msi-ec

If `/sys/devices/platform/msi-ec` or its firmware/mode interfaces are missing,
MEC cannot authorize controls. An in-tree driver may lack a firmware definition
available upstream. GF63 Thin 11UC / 16R6EMS1.107 reached READY in this validation
with upstream msi-ec 0.13 via DKMS; that is evidence for this device, not a promise
for every firmware. Follow [driver setup](hardware-support.md), use matching
kernel headers and inspect doctor again. Do not force arbitrary EC addresses or
replace a working signed module just to change the UI.

## Writes rejected or readback mismatch

An AccessDenied / hardware write access denied result means the current process
could not write the supported node. The state remains unchanged for the tested
unprivileged Webcam case. READY does not override this permission check. Review
notification history for the actual error; don't repeatedly apply blindly.

Some EC values settle after a successful write. v1.0.1 added bounded readback
settling retries, documented in [its release notes](releases/v1.0.1.md).
Verification remains mandatory. A timeout/mismatch remains an error; it must
never be described as verified success. Collect doctor export and the requested,
current and returned values for a report; avoid private environment/path data.

## Fan and profile observations

Cooler Boost can read On without a corresponding realtime_fan_speed percentage
jump. Percentage/raw telemetry is not RPM or proof of airflow. State/readback
verification and human acoustic observation are different evidence.

On the validation laptop, Silent's eco Shift Mode was accompanied by Super
Battery reading On even though the profile does not request that setting. See
[the full record](physical-validation.md). Inspect the entire real state after a
profile and restore the exact captured baseline when testing; Balanced is not
an automatic restoration target.
