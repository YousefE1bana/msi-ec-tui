# v1.1 release-candidate validation record

This is a **historical pre-release validation snapshot**, recorded before the
v1.1.0 version bump and publication. At this checkpoint Cargo and the published
stable release were **1.0.1**; no push, tag, release, PR, AUR publication or
default-theme promotion had been performed. Present-tense results below describe
that checkpoint. See [v1.1.0 release notes](releases/v1.1.0.md) for this release.

Starting revision: `7395a4db8e35579c22c540517b3f5b979a541b96`, branch
`feat/v1.1-tui-redesign`, clean worktree. P1/P2/P3 history was preserved. All
implementation and review in this pass were performed by the same agent.

## Checkpoint evidence

| Item | Result / evidence |
|---|---|
| 1. Starting state | Expected P3 HEAD, branch and clean state verified before editing |
| 2. Physical matrix | Seven individual controls plus Silent profile, keyboard and mouse: PASS; [exact matrix](physical-validation.md) |
| 3. Restoration | Independent comparison of all nine captured control fields: exact match |
| 4. Keyboard | Shift, Fan Mode, Boost, battery end, Webcam, Block, Backlight, Silent: verified and restored |
| 5. Mouse | Same seven controls and Silent: verified and restored |
| 6. Cancel | Profile Review → Cancel through both inputs: zero underlying changes; individual staging/review also zero changes |
| 7. Profiles | Real selected Silent, two changes / one same; existing transaction used; no invented ACTIVE profile |
| 8. Bugs found | Clipped Dashboard menu routes, palette overflow after new entries, ignored input starving transient expiry, stale Settings mouse copy, About underlay, glued table cells, missing native/Arch runtime requirements |
| 9. Fixes | Shared visible menu geometry, palette viewport/hit mapping, expiry on ignored input, truthful copy, About clear, bounded table cells; explicit glibc/libgcc requirements |
| 10. Visual fixes | Secondary metadata drops before menu routes; long names cannot displace values; layouts and default theme retained |
| 11. Screenshots | Eight main views, selected profile, review, Help, palette, About states, two themes; all eight views plus About/review at each smaller size |
| 12. Capture | Installed KDE Spectacle active-window captures of actual production MEC in Konsole, live physical telemetry; no generated UI images |
| 13. Installer | Inspect-first Bash, strict flags, stable GitHub discovery, exact SHA256, native packages or validated portable payload |
| 14. Distro paths | apt-get for Debian/Ubuntu; dnf/zypper for RPM families; user-local tar fallback; two explicit CPU mappings |
| 15. Driver | Detect and guide to upstream; no automatic kernel code installation |
| 16. Secure Boot | Report where discoverable; manual distro DKMS signing/MOK/reboot guidance; no security bypass |
| 17. Privileges | Existing current-process permissions; READY does not imply write access; normal desktop launch can be read-only for writes |
| 18. Desktop | Passive Terminal=true, Exec=mec, Icon=mec; native and user-local standard application/icon locations |
| 19. Compatibility | [Matrix](compatibility.md) distinguishes physical, configured CI, expected and unverified/unsupported paths |
| 20. Internal docs | docs/superpowers removed from current tree and public links; history retained |
| 21. Bootstrap file | Internal README-FIRST removed; useful guidance replaced by public docs |
| 22. Public docs | Installation, upgrade, uninstall, troubleshooting, support, physical validation, architecture, profiles, config, themes, gallery, compatibility and this record |
| 23. README | Original logo, badges, real Dashboard, value/safety, inspect-first install, requirements, controls, CLI/profiles, themes, public docs, credits |
| 24. README image | docs/assets/screenshots/dashboard.png, physical READY device |
| 25. Candidate A | Arctic Midnight (`arctic`): navy surfaces, ice-cyan structure, calm white values |
| 26. Candidate B | Graphite Violet (`graphite`): charcoal surfaces, restrained violet structure, cool telemetry |
| 27. Theme images | dashboard-arctic.png and dashboard-graphite.png in assets/screenshots |
| 28. Packaging | Desktop/icon/public docs in deb, RPM, tar and conditional Arch template; exact archive validation in CI |
| 29. Cleanliness | No public private home/worktree paths, internal planning or prototype assets; fixture terminology and historical release notes remain truthful |
| 30–37. Gates | See recorded final gate table below |
| 38. Commits | Focused UI, distribution and documentation commits; final hashes are in git log / owner handoff |
| 39. Final state | Clean worktree after those commits; ignored build/private QA outputs are not shipped |
| 40. Remaining release conditions | Owner review; observed eco/Super Battery coupling; distribution/permission limits below; no release publication authorized |

## Safety and privilege conclusion

The following protected files are byte-identical to the starting revision:

- src/hardware/msi_ec_write.rs
- src/hardware/write_boundary.rs
- src/safety/control.rs
- src/safety/executor.rs
- src/safety/profile_transaction.rs
- src/profiles/storage.rs
- src/profiles/transaction.rs

The bodies of `execute_pending` and `open_profile_confirmation` are unchanged.
Mouse and keyboard still converge before execution. There is no new hardware
executor, write API, transaction planner, arbitrary path/value interface or
mouse-specific authority. Renderers only present production state. The opt-in
network worker receives a version string and returns release metadata, with no
hardware/executor handles.

Physical write QA required owner authentication because controls are root-owned.
The desktop entry intentionally runs with normal user permissions. This pass
adds no sudo/pkexec/polkit/setuid helper, background privileged service or broad
hardware permission rules. The installer uses administrator authentication only
for normal native package installation. A future privilege/deployment design
requires separate owner approval; this checkpoint does not solve it implicitly.

## Branding and About

The original geometric MEC lettermark uses near-black, cyan and teal, with a
control bar/status dot; it is readable as an application icon without vendor
trademark artwork. Canonical icon: `packaging/desktop/mec.svg`. Documentation
lockup: `assets/branding/mec-lockup.svg`. Raster export:
`assets/branding/mec-icon-256.png`. Desktop package icon:
`/usr/share/icons/hicolor/scalable/apps/mec.svg` (portable user equivalent under
~/.local/share). XML and desktop syntax were validated; no external image/font
resources or scripts are embedded in the icon.

Open **8 Settings → Enter / ABOUT MEC**, or **P → About MEC**. Esc / Back returns
to the underlying screen; numeric screen routing remains 1–8. About displays
Yousef Osama, github.com/YousefE1bana, the repository, MIT, Rust and upstream
BeardOverflow/msi-ec. Current version comes from `CARGO_PKG_VERSION`, not a
hardcoded future release. Device/EC/support are real production values. The
screenshot is `assets/screenshots/mec-about.png`.

## Explicit update discovery

Only clicking CHECK FOR UPDATES / activating its focused card, or running
`mec update-check`, starts a check. Opening About, navigating and starting MEC
make no update request. No automatic install or telemetry upload exists.
The fixed source is GitHub's official
`https://api.github.com/repos/YousefE1bana/msi-ec-tui/releases/latest` endpoint;
requests verify TLS, refuse redirects, use a five-second total timeout and a
64 KiB response limit. The TUI worker is nonblocking and duplicate checks are
suppressed while in flight. Completion is polled on every input event.

Stable semantic versions accept an optional v prefix and compare semver
precedence, ignoring build metadata; drafts, prereleases and malformed metadata
are rejected. Offline/timeout/rate-limit failure displays a persistent failed
check status without affecting hardware mode, edits or transient result expiry.
Live explicit CLI/About checks returned latest stable 1.0.1 / up to date.
Automated comparisons and mocked failure/receiver tests perform no networking.

## Distribution limits

Native amd64 Debian package and x86_64 GNU archive payloads were built and
inspected locally, without installation. Release CI is configured for native
Ubuntu 24.04 x86_64/arm64 builders, tar/deb/RPM, SHA256SUMS and generated Arch
metadata. This pass did not run hosted CI, build RPM locally, validate an arm64
machine or install across distro package managers. RPM tooling and ShellCheck
were not installed on this host; no coverage is invented.

The release binary's observed highest GLIBC symbol requirement is 2.39; installer
runtime checks reject older glibc and musl/WSL/unknown CPUs. Older systems may
need a source build and compatible driver. Non-MSI/unsupported firmware remains
fail-closed. The installer fetches stable 1.0.1, not this unpublished interface.
The branch raw installer URL becomes available only after an authorized push.
SHA256 provides integrity against the downloaded checksum manifest, not publisher
signature authentication. No signing claim is made.

The observed eco/Super Battery coupling is documented in [physical QA](physical-validation.md).
Cooler Boost state/readback passed; acoustic/airflow confirmation was not made.
QA input targeting and screenshot-terminal sizing mistakes were corrected in
private tooling; incomplete harness runs were not represented as product passes.

## Final quality gates

| Gate | Result |
|---|---|
| cargo fmt --check | PASS |
| cargo clippy --all-targets --all-features -- -D warnings | PASS |
| cargo test | 1,748 passed / 0 failed (1,353 library + 395 integration) |
| cargo build --release | PASS; native target build also PASS |
| cargo package --allow-dirty --no-verify | PASS; development prototypes/internal plans excluded |
| git diff --check | PASS |
| bash -n install.sh | PASS |
| ShellCheck | Not installed; skipped, not represented as a pass |
| Installer tests | 7 offline tests PASS, including exact checksum failure, CPU/distro mapping, dry-run pin and repeated same-version invocation |
| Docs guide tests | 3 PASS |
| Release packaging/workflow tests | 36 PASS, including runtime requirements, reproducibility, exact archive byte validation and changed/unapproved payload rejection |
| Desktop validation | desktop-file-validate PASS; original SVG XML valid |
| Real installer dry-run | PASS, selected stable v1.0.1 amd64 .deb; no sudo/install |
| Native artifacts | amd64 .deb built/inspected; x86_64 archive built and matched binary/assets/public docs |
| Safety fixtures | Existing keyboard/mouse/confirmation/profile/rollback/READ-ONLY/unavailable/zero-area regressions PASS as part of the complete suite |

New coverage adds 25 tests over the P3 baseline of 1,723: utility routing,
semver/release validation, network-result isolation, expiry under ignored input,
theme persistence, menu/palette visibility and hit mapping, bounded Unicode table
cells, About underlay/zero-area safety, CLI options, installer behavior and archive
payload integrity and runtime requirements. Tests use mocks, temporary trees and TestBackend, never physical
writes. Persistent diagnostic state remains separate from transient notices;
success/failure expiry stays three/six seconds.
