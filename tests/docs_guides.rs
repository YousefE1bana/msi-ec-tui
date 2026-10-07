//! Lightweight checks for the install/upgrade/uninstall guides: presence,
// README linkage, exact config paths, and no false publication, signature,
// or privilege-helper claims.

use std::path::PathBuf;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_dir().join(relative))
        .unwrap_or_else(|_| panic!("missing {relative}"))
}

#[test]
fn install_upgrade_uninstall_guides_exist_and_link() {
    for guide in [
        "docs/installation.md",
        "docs/upgrade.md",
        "docs/uninstall.md",
    ] {
        let text = read(guide);
        assert!(!text.trim().is_empty(), "{guide} must not be empty");
    }
    let readme = read("README.md");
    for guide in [
        "docs/installation.md",
        "docs/upgrade.md",
        "docs/uninstall.md",
    ] {
        assert!(readme.contains(guide), "README must link {guide}");
    }
}

#[test]
fn guides_keep_config_paths_and_binary_name() {
    let install = read("docs/installation.md");
    let upgrade = read("docs/upgrade.md");
    let uninstall = read("docs/uninstall.md");
    for text in [&install, &upgrade, &uninstall] {
        assert!(text.contains("~/.config/mec/config.toml"));
        assert!(text.contains("~/.config/mec/profiles/"));
        assert!(text.contains("mec --version") || text.contains("/usr/bin/mec"));
    }
    // Upgrade path explicitly teaches post-upgrade verification.
    assert!(upgrade.contains("mec --version"));
    assert!(upgrade.contains("mec doctor"));
    // No stale binary name outside the GitHub URL.
    for (label, text) in [
        ("install", &install),
        ("upgrade", &upgrade),
        ("uninstall", &uninstall),
    ] {
        for line in text.lines() {
            if line.contains("msi-ec-tui") {
                assert!(
                    line.contains("github.com"),
                    "{label} must not use a stale binary name: {line:?}"
                );
            }
        }
    }
}

#[test]
fn guides_make_no_false_claims() {
    let install = read("docs/installation.md");
    let upgrade = read("docs/upgrade.md");
    let uninstall = read("docs/uninstall.md");
    // No AUR publication claim: every guide states none exists yet.
    for (label, text) in [
        ("install", &install),
        ("upgrade", &upgrade),
        ("uninstall", &uninstall),
    ] {
        let lowered = text.to_lowercase();
        assert!(
            lowered.contains("not yet")
                || lowered.contains("none exists")
                || lowered.contains("publication is not claimed"),
            "{label} must not imply the AUR entry is published"
        );
    }
    // No signature claim: docs deny signatures ("there are no ... release
    // signatures") and never claim signed artifacts. Operates on full text
    // because the denial sentence may wrap across lines.
    for (label, text) in [
        ("install", &install),
        ("upgrade", &upgrade),
        ("uninstall", &uninstall),
    ] {
        let lowered = text.to_lowercase();
        let normalized: String = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            !normalized.contains("gpg")
                && !normalized.contains("signed release")
                && !normalized.contains("signature file")
                && !normalized.contains("verify the signature"),
            "{label} must not claim signatures exist"
        );
    }
    assert!(
        install
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("there are no cryptographic release signatures"),
        "install guide must state checksums exist instead of signatures"
    );
    // No privilege-helper invention: privilege-helper mentions are denials
    // only ("AUR helper" tooling wording is unaffected).
    for (label, text) in [
        ("install", &install),
        ("upgrade", &upgrade),
        ("uninstall", &uninstall),
    ] {
        for line in text.lines() {
            let lowered = line.to_lowercase();
            if lowered.contains("privilege helper") || lowered.contains("privileged helper") {
                assert!(
                    lowered.contains("no "),
                    "{label} must not invent a privilege helper: {line:?}"
                );
            }
        }
    }
    // Public installation entry points and notes follow the authoritative version.
    let version = env!("CARGO_PKG_VERSION");
    let readme = read("README.md");
    assert!(readme.contains(&format!("Stable release: v{version}")));
    assert!(readme.contains(&format!("docs/releases/v{version}.md")));
    assert!(readme.contains("/main/install.sh"));
    assert!(!readme.contains("has not been published"));
    assert!(install.contains(&format!("--version {version}")));
    assert!(upgrade.contains(&format!("mec_{version}_amd64.deb")));
    assert!(upgrade.contains(&format!("mec-{version}-1.x86_64.rpm")));
    assert!(read("install.sh").contains(&format!("--version {version}")));
}
