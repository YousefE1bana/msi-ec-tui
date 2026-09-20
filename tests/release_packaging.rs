//! Release packaging tests that need no cross hardware and no full
//! multi-arch builds: script argument validation, checksum file behavior,
//! and static script hygiene. A native archive smoke runs separately as a
//! manual gate (see the PLAN-007 task text), not inside `cargo test`.

use std::path::PathBuf;
use std::process::Command;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script(name: &str) -> PathBuf {
    manifest_dir().join("scripts").join(name)
}

#[test]
fn archive_script_rejects_unsupported_target() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("bash")
        .arg(script("build-release-archive.sh"))
        .arg("riscv64-unknown-linux-gnu")
        .arg(dir.path())
        .output()
        .expect("archive script runs");
    assert!(!status.status.success());
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(stderr.contains("unsupported target"));
}

#[test]
fn archive_script_rejects_missing_target() {
    let status = Command::new("bash")
        .arg(script("build-release-archive.sh"))
        .output()
        .expect("archive script runs");
    assert!(!status.status.success());
}

#[test]
fn checksums_use_basename_only_sorted_entries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("mec-aarch64-unknown-linux-gnu.tar.gz"),
        b"fake-b",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mec-x86_64-unknown-linux-gnu.tar.gz"),
        b"fake-a",
    )
    .unwrap();
    let status = Command::new("bash")
        .arg(script("generate-checksums.sh"))
        .arg(dir.path())
        .arg("mec-x86_64-unknown-linux-gnu.tar.gz")
        .arg("mec-aarch64-unknown-linux-gnu.tar.gz")
        .status()
        .expect("checksum script runs");
    assert!(status.success());
    let sums = std::fs::read_to_string(dir.path().join("SHA256SUMS")).unwrap();
    let lines: Vec<&str> = sums.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].ends_with("mec-aarch64-unknown-linux-gnu.tar.gz"));
    assert!(lines[1].ends_with("mec-x86_64-unknown-linux-gnu.tar.gz"));
    for line in &lines {
        assert!(!line.contains('/'), "basename only: {line:?}");
    }
}

#[test]
fn checksums_exclude_themselves_and_stay_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("mec-x86_64-unknown-linux-gnu.tar.gz"),
        b"fake",
    )
    .unwrap();
    let run = || {
        Command::new("bash")
            .arg(script("generate-checksums.sh"))
            .arg(dir.path())
            .status()
            .expect("checksum script runs")
    };
    assert!(run().success());
    let first = std::fs::read_to_string(dir.path().join("SHA256SUMS")).unwrap();
    assert!(!first.contains("SHA256SUMS"));
    assert!(run().success());
    let second = std::fs::read_to_string(dir.path().join("SHA256SUMS")).unwrap();
    assert_eq!(first, second);
    // The file verifies with standard tooling.
    let check = Command::new("bash")
        .arg("-c")
        .arg("sha256sum -c SHA256SUMS")
        .current_dir(dir.path())
        .status()
        .expect("sha256sum runs");
    assert!(check.success());
}

#[test]
fn checksums_reject_empty_artifact_directory() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new("bash")
        .arg(script("generate-checksums.sh"))
        .arg(dir.path())
        .output()
        .expect("checksum script runs");
    assert!(!status.status.success());
}

#[test]
fn checksums_reject_missing_required_artifact() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("mec-x86_64-unknown-linux-gnu.tar.gz"),
        b"fake",
    )
    .unwrap();
    let status = Command::new("bash")
        .arg(script("generate-checksums.sh"))
        .arg(dir.path())
        .arg("mec-aarch64-unknown-linux-gnu.tar.gz")
        .output()
        .expect("checksum script runs");
    assert!(!status.status.success());
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(stderr.contains("missing"));
}

#[test]
fn release_scripts_stay_minimal_and_unprivileged() {
    for name in [
        "build-release-archive.sh",
        "generate-checksums.sh",
        "build-deb.sh",
        "build-rpm.sh",
    ] {
        let text = std::fs::read_to_string(script(name)).unwrap();
        assert!(text.contains("set -euo pipefail"), "{name}");
        let code: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "sudo",
            "curl",
            "wget",
            "pkexec",
            "modprobe",
            "chmod /sys",
            "chown /sys",
            "systemctl",
            "udevadm",
        ] {
            assert!(!code.contains(forbidden), "{name} contains {forbidden:?}");
        }
    }
    let archive = std::fs::read_to_string(script("build-release-archive.sh")).unwrap();
    assert!(archive.contains("--locked"));
    assert!(archive.contains("x86_64-unknown-linux-gnu"));
    assert!(archive.contains("aarch64-unknown-linux-gnu"));
}

#[test]
fn package_scripts_reject_unsupported_targets() {
    let dir = tempfile::tempdir().unwrap();
    let fake_binary = dir.path().join("mec");
    std::fs::write(&fake_binary, b"fake").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for name in ["build-deb.sh", "build-rpm.sh"] {
        let status = Command::new("bash")
            .arg(script(name))
            .arg(&fake_binary)
            .arg("riscv64-unknown-linux-gnu")
            .arg(dir.path().join("out"))
            .output()
            .expect("package script runs");
        assert!(!status.status.success(), "{name} must reject target");
        let stderr = String::from_utf8_lossy(&status.stderr);
        assert!(stderr.contains("unsupported target"), "{name}");
    }
}

#[test]
fn package_scripts_reject_non_executable_input() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent-mec");
    for name in ["build-deb.sh", "build-rpm.sh"] {
        let status = Command::new("bash")
            .arg(script(name))
            .arg(&missing)
            .arg("x86_64-unknown-linux-gnu")
            .arg(dir.path().join("out"))
            .output()
            .expect("package script runs");
        assert!(!status.status.success(), "{name} must reject input");
    }
}

#[test]
fn deb_and_rpm_arch_mappings_are_documented() {
    let deb = std::fs::read_to_string(script("build-deb.sh")).unwrap();
    assert!(deb.contains("x86_64-unknown-linux-gnu) DEB_ARCH=amd64"));
    assert!(deb.contains("aarch64-unknown-linux-gnu) DEB_ARCH=arm64"));
    let rpm = std::fs::read_to_string(script("build-rpm.sh")).unwrap();
    assert!(rpm.contains("x86_64-unknown-linux-gnu) RPM_ARCH=x86_64"));
    assert!(rpm.contains("aarch64-unknown-linux-gnu) RPM_ARCH=aarch64"));
}

#[test]
fn package_metadata_has_no_lifecycle_hooks() {
    let manifest = manifest_dir();
    let control = std::fs::read_to_string(manifest.join("packaging/deb/control.template")).unwrap();
    assert!(control.contains("Package: mec"));
    assert!(control.contains("@VERSION@"));
    assert!(control.contains("@ARCH@"));
    let spec = std::fs::read_to_string(manifest.join("packaging/rpm/mec.spec")).unwrap();
    assert!(spec.contains("Name: mec"));
    assert!(spec.contains("@VERSION@"));
    assert!(spec.contains("@ARCH@"));
    for (label, text) in [("deb", &control), ("rpm", &spec)] {
        let code: String = text
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for hook in [
            "preinst", "postinst", "prerm", "postrm", "%pre", "%post", "%preun", "%postun",
            "systemd", "udev", "modprobe",
        ] {
            assert!(!code.contains(hook), "{label} metadata contains {hook:?}");
        }
    }
    assert!(spec.contains("/usr/bin/mec"));
}

#[test]
fn package_metadata_installs_no_config_or_sys_paths() {
    let manifest = manifest_dir();
    let control = std::fs::read_to_string(manifest.join("packaging/deb/control.template")).unwrap();
    let spec = std::fs::read_to_string(manifest.join("packaging/rpm/mec.spec")).unwrap();
    let deb_script = std::fs::read_to_string(script("build-deb.sh")).unwrap();
    let rpm_script = std::fs::read_to_string(script("build-rpm.sh")).unwrap();
    for text in [&control, &spec, &deb_script, &rpm_script] {
        for forbidden in ["/etc", "/sys", "/usr/local", "/home", "/root"] {
            let hits: Vec<&str> = text
                .lines()
                .filter(|line| {
                    let body = line.trim_start();
                    if body.starts_with('#') {
                        return false;
                    }
                    line.contains(forbidden)
                })
                .collect();
            assert!(hits.is_empty(), "forbidden path {forbidden:?} in: {hits:?}");
        }
    }
}

#[test]
fn archive_script_uses_valid_sort_option() {
    let text = std::fs::read_to_string(script("build-release-archive.sh")).unwrap();
    let code: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("--sort=name"),
        "must probe real GNU tar option"
    );
    assert!(
        !code.contains("--sort-name"),
        "invalid sort flag must not appear in code"
    );
}

#[test]
fn archive_script_probes_full_ownership_set_and_deterministic_gzip() {
    let text = std::fs::read_to_string(script("build-release-archive.sh")).unwrap();
    let code: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        code.contains("tar --owner=0 --group=0 --numeric-owner"),
        "ownership flags must be probed as one set"
    );
    assert!(
        code.contains("--owner=0 --group=0 --numeric-owner)"),
        "probed set must match appended set"
    );
    assert!(
        code.contains("gzip -n"),
        "gzip output must be deterministic"
    );
}

#[test]
fn package_scripts_verify_internal_metadata() {
    let rpm = std::fs::read_to_string(script("build-rpm.sh")).unwrap();
    assert!(rpm.contains("--target"), "rpmbuild needs explicit target");
    assert!(
        rpm.contains("\"$RPM_ARCH\"") || rpm.contains("$RPM_ARCH"),
        "rpm target must be the mapped arch"
    );
    for token in ["%{NAME}", "%{VERSION}", "%{RELEASE}", "%{ARCH}"] {
        assert!(rpm.contains(token), "rpm must verify {token}");
    }
    let deb = std::fs::read_to_string(script("build-deb.sh")).unwrap();
    for field in ["Package", "Version", "Architecture"] {
        assert!(
            deb.contains("dpkg-deb -f") && deb.contains(field),
            "deb must verify {field}"
        );
    }
}

fn staged_payload(root: &std::path::Path, topdir: &str, marker: &[u8]) {
    let payload = root.join(topdir);
    std::fs::create_dir_all(&payload).unwrap();
    let mut binary = Vec::from(marker);
    binary.extend_from_slice(b":fake-mec-binary");
    std::fs::write(payload.join("mec"), &binary).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(payload.join("mec"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    std::fs::write(payload.join("README.md"), b"# fake\n").unwrap();
    std::fs::write(payload.join("LICENSE"), b"fake\n").unwrap();
    std::fs::write(payload.join("SECURITY.md"), b"fake\n").unwrap();
}

fn build_from_stage(stage: &std::path::Path, out: &std::path::Path, target: &str) {
    let status = Command::new("bash")
        .arg(script("build-release-archive.sh"))
        .arg("--from-stage")
        .arg(stage)
        .arg(target)
        .arg(out)
        .env("SOURCE_DATE_EPOCH", "1234567890")
        .current_dir(manifest_dir())
        .status()
        .expect("archive script runs");
    assert!(status.success());
}

#[test]
fn identical_staged_input_yields_identical_archive_bytes() {
    let version = env!("CARGO_PKG_VERSION");
    let target = "x86_64-unknown-linux-gnu";
    let topdir = format!("mec-{version}-{target}");
    let work = tempfile::tempdir().unwrap();
    let mut digests = Vec::new();
    for (tag, stamp) in [("a", "@1111111111"), ("b", "@1999999999")] {
        let stage = work.path().join(format!("stage-{tag}"));
        staged_payload(&stage, &topdir, b"same");
        // Different input mtimes must not leak into the archive.
        let touch = Command::new("touch")
            .arg("-d")
            .arg(stamp)
            .arg(stage.join(&topdir).join("mec"))
            .status()
            .expect("touch runs");
        assert!(touch.success());
        let out = work.path().join(format!("out-{tag}"));
        build_from_stage(&stage, &out, target);
        let archive = out.join(format!("mec-{target}.tar.gz"));
        let bytes = std::fs::read(&archive).unwrap();
        digests.push(sha256_hex(&bytes));
    }
    assert_eq!(digests[0], digests[1]);
}

fn sha256_hex(bytes: &[u8]) -> String {
    // No new dependencies: shell out to the same sha256sum the release
    // scripts rely on.
    use std::io::Write;
    let mut child = Command::new("sha256sum")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("sha256sum runs");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(bytes)
        .expect("write");
    let output = child.wait_with_output().expect("sha256sum output");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("utf8")
        .split_whitespace()
        .next()
        .expect("digest")
        .to_owned()
}

#[test]
fn archive_members_are_exact() {
    let version = env!("CARGO_PKG_VERSION");
    let target = "x86_64-unknown-linux-gnu";
    let topdir = format!("mec-{version}-{target}");
    let work = tempfile::tempdir().unwrap();
    let stage = work.path().join("stage");
    staged_payload(&stage, &topdir, b"x");
    let out = work.path().join("out");
    build_from_stage(&stage, &out, target);
    let archive = out.join(format!("mec-{target}.tar.gz"));
    let listing = Command::new("tar")
        .arg("-tzvf")
        .arg(&archive)
        .output()
        .expect("tar lists");
    assert!(listing.status.success());
    let text = String::from_utf8(listing.stdout).expect("utf8");
    let mut entries = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let perms = parts.next().expect("perms");
        let path = parts.last().expect("path");
        entries.push((perms.to_owned(), path.to_owned()));
    }
    let mut paths: Vec<&str> = entries.iter().map(|(_, p)| p.as_str()).collect();
    paths.sort_unstable();
    let mut expected = vec![
        format!("{topdir}/"),
        format!("{topdir}/LICENSE"),
        format!("{topdir}/README.md"),
        format!("{topdir}/SECURITY.md"),
        format!("{topdir}/mec"),
    ];
    expected.sort_unstable();
    assert_eq!(paths, expected);
    for (perms, path) in &entries {
        assert!(!path.starts_with('/'), "no absolute path: {path:?}");
        assert!(
            !path.split('/').any(|part| part == ".."),
            "no traversal: {path:?}"
        );
        if path.ends_with("/mec") {
            assert!(perms.starts_with("-rwx"), "mec executable: {perms:?}");
        } else if !path.ends_with('/') {
            assert!(perms.starts_with("-rw-"), "docs regular: {perms:?}");
        }
    }
}

#[test]
fn release_workflow_builds_each_arch_on_its_native_runner() {
    let text = std::fs::read_to_string(manifest_dir().join(".github/workflows/release-build.yml"))
        .unwrap();
    assert!(text.contains("runs-on: ${{ matrix.runner }}"));
    assert!(text.contains("runner: ubuntu-24.04"));
    assert!(text.contains("runner: ubuntu-24.04-arm"));
    assert!(text.contains("target: x86_64-unknown-linux-gnu"));
    assert!(text.contains("target: aarch64-unknown-linux-gnu"));
    assert!(text.contains("deb_arch: amd64"));
    assert!(text.contains("deb_arch: arm64"));
    assert!(text.contains("rpm_arch: x86_64"));
    assert!(text.contains("rpm_arch: aarch64"));
    // The x86_64 entry must not resolve to the ARM runner and vice versa.
    let x86_pos = text
        .find("target: x86_64-unknown-linux-gnu")
        .expect("x86 entry");
    let arm_runner = text.find("ubuntu-24.04-arm").expect("arm runner");
    assert!(x86_pos < arm_runner);
}

#[test]
fn release_workflow_needs_no_cross_linker() {
    let text = std::fs::read_to_string(manifest_dir().join(".github/workflows/release-build.yml"))
        .unwrap();
    assert!(!text.contains("gcc-aarch64-linux-gnu"));
    assert!(!text.contains("CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER"));
}

#[test]
fn release_workflow_executes_both_binaries_natively() {
    let text = std::fs::read_to_string(manifest_dir().join(".github/workflows/release-build.yml"))
        .unwrap();
    assert!(text.contains("Execute native binary"));
    assert!(!text.contains("x86_64 only"));
    assert!(!text.contains("x86-only"));
    // Validation consumes matrix metadata instead of a second mapping.
    assert!(text.contains("${{ matrix.deb_arch }}"));
    assert!(text.contains("${{ matrix.rpm_arch }}"));
}

#[test]
fn rpm_builder_fails_closed_on_foreign_host_arch() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // Hermetic rpm double: claims an ARM host regardless of hardware.
    let fake_rpm = bin.join("rpm");
    std::fs::write(&fake_rpm, "#!/bin/bash\necho aarch64\n").unwrap();
    std::fs::set_permissions(&fake_rpm, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fake_binary = dir.path().join("mec");
    std::fs::write(&fake_binary, b"fake").unwrap();
    std::fs::set_permissions(&fake_binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let status = Command::new("bash")
        .arg(script("build-rpm.sh"))
        .arg(&fake_binary)
        .arg("x86_64-unknown-linux-gnu")
        .arg(dir.path().join("out"))
        .env("PATH", path)
        .output()
        .expect("package script runs");
    assert!(!status.status.success());
    let stderr = String::from_utf8_lossy(&status.stderr);
    assert!(
        stderr.contains("requires a native x86_64 builder"),
        "unexpected stderr: {stderr:?}"
    );
}

#[test]
fn rpm_builder_guard_passes_through_on_matching_host() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // Hermetic rpm double: claims the requested host; the script must get
    // past the guard (it then fails later for unrelated local reasons,
    // but never with the native-builder error).
    let fake_rpm = bin.join("rpm");
    std::fs::write(&fake_rpm, "#!/bin/bash\necho x86_64\n").unwrap();
    std::fs::set_permissions(&fake_rpm, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fake_binary = dir.path().join("mec");
    std::fs::write(&fake_binary, b"fake").unwrap();
    std::fs::set_permissions(&fake_binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let status = Command::new("bash")
        .arg(script("build-rpm.sh"))
        .arg(&fake_binary)
        .arg("x86_64-unknown-linux-gnu")
        .arg(dir.path().join("out"))
        .env("PATH", path)
        .output()
        .expect("package script runs");
    let stderr = String::from_utf8_lossy(&status.stderr);
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(
        !stderr.contains("requires a native") && !stdout.contains("requires a native"),
        "guard must pass on matching host: {stderr:?}"
    );
}

#[test]
fn rpm_builder_queries_native_host_cpu() {
    let text = std::fs::read_to_string(script("build-rpm.sh")).unwrap();
    let code: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(code.contains("rpm --eval") && code.contains("_host_cpu"));
}
