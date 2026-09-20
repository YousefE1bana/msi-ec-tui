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
