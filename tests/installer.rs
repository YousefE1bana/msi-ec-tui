//! Offline installer contracts: no host package installation or privilege calls.
use std::path::Path;
use std::process::{Command, Output};
fn installer() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh")
}
fn sourced(code: &str, args: &[&str]) -> Output {
    Command::new("bash")
        .args([
            "-c",
            &format!("source \"$1\"; shift; {code}"),
            "installer-test",
        ])
        .arg(installer())
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn architecture_mapping_rejects_guesses() {
    for (arch, expected) in [
        ("x86_64", "x86_64 amd64 x86_64-unknown-linux-gnu"),
        ("amd64", "x86_64 amd64 x86_64-unknown-linux-gnu"),
        ("aarch64", "aarch64 arm64 aarch64-unknown-linux-gnu"),
        ("arm64", "aarch64 arm64 aarch64-unknown-linux-gnu"),
    ] {
        let output = sourced("map_arch \"$1\"", &[arch]);
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }
    for arch in ["i686", "armv7l", "riscv64"] {
        assert!(!sourced("map_arch \"$1\"", &[arch]).status.success());
    }
}
#[test]
fn distro_format_is_explicit() {
    for (id, like, expected) in [
        ("ubuntu", "debian", "deb"),
        ("debian", "", "deb"),
        ("fedora", "", "rpm"),
        ("opensuse", "suse", "rpm"),
        ("arch", "", "tar"),
        ("endeavouros", "arch", "tar"),
    ] {
        let output = sourced("select_format \"$1\" \"$2\"", &[id, like]);
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }
}
#[test]
fn checksum_requires_one_exact_name_and_valid_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let artifact = dir.path().join("asset");
    let sums = dir.path().join("SHA256SUMS");
    std::fs::write(&artifact, b"MEC fixture").unwrap();
    let hash = Command::new("sha256sum").arg(&artifact).output().unwrap();
    let hash = String::from_utf8(hash.stdout).unwrap();
    let hash = hash.split_whitespace().next().unwrap();
    for (body, valid) in [
        (format!("{hash}  asset\n"), true),
        (format!("{hash}  another\n"), false),
        (format!("{hash}  asset\n{hash}  asset\n"), false),
        (format!("{}  asset\n", "0".repeat(64)), false),
        (format!("{hash}  ../asset\n"), false),
    ] {
        std::fs::write(&sums, body).unwrap();
        let output = sourced(
            "verify_artifact \"$1\" \"$2\" asset",
            &[artifact.to_str().unwrap(), sums.to_str().unwrap()],
        );
        assert_eq!(output.status.success(), valid);
    }
}
#[test]
fn help_and_invalid_arguments_never_need_network() {
    assert!(
        Command::new("bash")
            .arg(installer())
            .arg("--help")
            .status()
            .unwrap()
            .success()
    );
    for args in [
        vec!["--unknown"],
        vec!["--version"],
        vec!["--version", "../evil"],
        vec!["--version", "1.1.0-rc.1"],
    ] {
        assert!(
            !Command::new("bash")
                .arg(installer())
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
}
#[test]
fn dry_run_preserves_pin_despite_os_release_version_and_never_invokes_sudo() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let curl = dir.path().join("curl");
    let sudo = dir.path().join("sudo");
    let marker = dir.path().join("sudo-called");
    std::fs::write(&curl,"#!/bin/bash\nwhile (($#)); do if [[ $1 == -o ]]; then out=$2; shift; fi; shift; done\ncp \"$MOCK_RELEASE\" \"$out\"\n").unwrap();
    std::fs::write(&sudo, "#!/bin/bash\ntouch \"$MOCK_SUDO_MARKER\"\nexit 99\n").unwrap();
    for file in [&curl, &sudo] {
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let json = dir.path().join("release.json");
    std::fs::write(&json,r#"{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[{"name":"mec_1.2.3_amd64.deb"},{"name":"mec_1.2.3_arm64.deb"},{"name":"mec-1.2.3-1.x86_64.rpm"},{"name":"mec-1.2.3-1.aarch64.rpm"},{"name":"mec-x86_64-unknown-linux-gnu.tar.gz"},{"name":"mec-aarch64-unknown-linux-gnu.tar.gz"},{"name":"SHA256SUMS"}]}"#).unwrap();
    let output = Command::new("bash")
        .arg(installer())
        .args(["--dry-run", "--version", "1.2.3"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.path().display(),
                std::env::var("PATH").unwrap()
            ),
        )
        .env("MOCK_RELEASE", json)
        .env("MOCK_SUDO_MARKER", &marker)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Selected stable release: v1.2.3"));
    assert!(text.contains("DRY RUN"));
    assert!(!marker.exists());
}
#[test]
fn desktop_launcher_is_passive_and_svg_contains_no_external_assets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let desktop = std::fs::read_to_string(root.join("packaging/desktop/mec.desktop")).unwrap();
    assert!(desktop.contains("\nExec=mec\n"));
    assert!(desktop.contains("\nTerminal=true\n"));
    assert!(desktop.contains("\nIcon=mec\n"));
    for forbidden in ["sudo", "pkexec", "/home/", "sh -c"] {
        assert!(!desktop.contains(forbidden));
    }
    let svg = std::fs::read_to_string(root.join("packaging/desktop/mec.svg")).unwrap();
    assert!(svg.contains("viewBox=\"0 0 256 256\""));
    for forbidden in ["href=", "<script", "<image", "<text", "dragon"] {
        assert!(!svg.contains(forbidden));
    }
}

#[test]
fn same_version_repeated_run_only_checks_readiness() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    for (file, body) in [
        (
            "mec",
            "#!/bin/bash\nif [[ $1 == --version ]]; then echo 'mec 1.2.3'; else echo doctor >> \"$MOCK_CALLS\"; fi\n",
        ),
        (
            "sudo",
            "#!/bin/bash\necho sudo >> \"$MOCK_CALLS\"; exit 99\n",
        ),
        (
            "curl",
            "#!/bin/bash\necho curl >> \"$MOCK_CALLS\"\nwhile (($#)); do if [[ $1 == -o ]]; then out=$2; shift; fi; shift; done\ncp \"$MOCK_RELEASE\" \"$out\"\n",
        ),
    ] {
        let path = dir.path().join(file);
        std::fs::write(&path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let release = dir.path().join("release.json");
    std::fs::write(&release,r#"{"tag_name":"v1.2.3","draft":false,"prerelease":false,"assets":[{"name":"mec-x86_64-unknown-linux-gnu.tar.gz"},{"name":"mec-aarch64-unknown-linux-gnu.tar.gz"},{"name":"SHA256SUMS"}]}"#).unwrap();
    let calls = dir.path().join("calls");
    for _ in 0..2 {
        let output = Command::new("bash")
            .args([
                "-c",
                "source \"$1\"; select_format() { echo tar; }; main --version 1.2.3",
                "installer-test",
            ])
            .arg(installer())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    dir.path().display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("MOCK_RELEASE", &release)
            .env("MOCK_CALLS", &calls)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("leaving installation unchanged"));
    }
    // One metadata request and doctor per invocation, no artifact download or sudo.
    assert_eq!(
        std::fs::read_to_string(calls).unwrap(),
        "curl\ndoctor\ncurl\ndoctor\n"
    );
}
