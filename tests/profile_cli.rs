//! Profile CLI through the public binary only: isolated TempDir fake
//! sysfs roots via `--sys-root` and isolated config trees via
//! `XDG_CONFIG_HOME`. Never touches real `/sys` or real home config.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

fn msi_identity(root: &Path) {
    let dmi = root.join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(
        dmi.join("sys_vendor"),
        b"Micro-Star International Co., Ltd.\n",
    )
    .unwrap();
    fs::write(dmi.join("product_name"), b"Test MSI Laptop\n").unwrap();
}

fn ec_file(root: &Path, name: &str, contents: &[u8]) {
    let dir = root.join("sys/devices/platform/msi-ec");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), contents).unwrap();
}

fn cooler_root(root: &Path) {
    msi_identity(root);
    ec_file(root, "cooler_boost", b"off\n");
}

fn gaming_root(root: &Path) {
    msi_identity(root);
    ec_file(root, "shift_mode", b"eco\n");
    ec_file(root, "available_shift_modes", b"eco\nturbo\n");
    ec_file(root, "fan_mode", b"auto\n");
    ec_file(root, "available_fan_modes", b"auto\nadvanced\n");
    ec_file(root, "cooler_boost", b"off\n");
    ec_file(root, "super_battery", b"on\n");
}

fn custom_profile(config: &Path, slug: &str, contents: &[u8]) {
    let dir = config.join("mec/profiles");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{slug}.toml")), contents).unwrap();
}

fn mec() -> Command {
    Command::cargo_bin("mec").unwrap()
}

#[test]
fn parses_profile_list() {
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["profile", "list"])
        .assert()
        .success();
}

#[test]
fn parses_profile_show_gaming() {
    let sys = tempdir().unwrap();
    gaming_root(sys.path());
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "gaming",
        ])
        .assert()
        .success();
}

#[test]
fn parses_profile_apply_gaming_noop_or_changed() {
    let sys = tempdir().unwrap();
    gaming_root(sys.path());
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "gaming",
        ])
        .assert()
        .success();
}

#[test]
fn missing_profile_argument_rejected() {
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["profile", "show"])
        .assert()
        .failure();
}

#[test]
fn extra_argument_rejected() {
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["profile", "show", "gaming", "extra"])
        .assert()
        .failure();
}

#[test]
fn list_shows_five_builtins_in_order_with_none() {
    let config = tempdir().unwrap();
    let output = mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["profile", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        text,
        "Built-in profiles:\n  balanced          Balanced\n  silent            Silent\n  gaming            Gaming\n  battery-saver     Battery Saver\n  maximum-cooling   Maximum Cooling\nCustom profiles:\n  (none)\n"
    );
}

#[test]
fn list_shows_sorted_customs_and_hides_reserved_and_invalid() {
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "work",
        b"name = \"Work\"\n\n[device]\nkeyboard_backlight = 1\n",
    );
    custom_profile(
        config.path(),
        "quiet-night",
        b"name = \"Quiet\"\n\n[device]\nkeyboard_backlight = 1\n",
    );
    custom_profile(
        config.path(),
        "gaming",
        b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
    );
    custom_profile(
        config.path(),
        "Work",
        b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
    );
    fs::write(config.path().join("mec/profiles/notes.txt"), b"hi").unwrap();
    let output = mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["profile", "list"])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.ends_with("Custom profiles:\n  quiet-night\n  work\n"));
    assert!(!text.contains("notes"));
}

#[test]
fn list_needs_no_hardware() {
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["--sys-root", "/nonexistent-fake-root", "profile", "list"])
        .assert()
        .success();
}

#[test]
fn show_custom_renders_exact_name_and_fields() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "work",
        b"name = \"My Work\"\n\n[performance]\ncooler_boost = true\n\n[battery]\ncharge_end_threshold = 80\n\n[device]\nkeyboard_backlight = 2\n",
    );
    let output = mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "work",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.starts_with("Profile: My Work\nSource: custom\nSlug: work\n"));
    assert!(text.contains("cooler_boost = true\n"));
    assert!(text.contains("[battery]\ncharge_end_threshold = 80\n"));
    assert!(!text.contains("charge_control_start_threshold"));
    assert!(text.contains("[device]\nkeyboard_backlight = 2\n"));
    // No mutation.
    assert_eq!(
        fs::read(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap(),
        b"off\n"
    );
}

#[test]
fn show_custom_omits_absent_sections() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "tiny",
        b"name = \"Tiny\"\n\n[device]\nkeyboard_backlight = 1\n",
    );
    let output = mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "tiny",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains("[performance]"));
    assert!(!text.contains("[battery]"));
    assert!(text.contains("[device]"));
}

#[test]
fn show_malformed_custom_fails_nonzero() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    custom_profile(config.path(), "broken", b"name = [unclosed\n");
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "broken",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC profile failed"));
}

#[test]
fn show_builtin_resolves_and_omits_unsupported() {
    let sys = tempdir().unwrap();
    msi_identity(sys.path());
    ec_file(sys.path(), "cooler_boost", b"off\n");
    let config = tempdir().unwrap();
    let output = mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "maximum-cooling",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.starts_with("Profile: Maximum Cooling\nSource: built-in\nSlug: maximum-cooling\n")
    );
    // No fan capability: only cooler_boost remains.
    assert!(!text.contains("fan_mode"));
    assert!(text.contains("cooler_boost = true\n"));
    assert_eq!(
        fs::read(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap(),
        b"off\n"
    );
}

#[test]
fn show_unavailable_builtin_fails_nonzero() {
    let sys = tempdir().unwrap();
    msi_identity(sys.path());
    fs::create_dir_all(sys.path().join("sys/devices/platform/msi-ec")).unwrap();
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "gaming",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC profile failed"));
}

#[test]
fn apply_custom_success_reports_exact_counts() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "cool",
        b"name = \"Cool\"\n\n[performance]\ncooler_boost = true\n",
    );
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "cool",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "MEC profile applied: Cool (1 changed, 0 unchanged)\n",
        ));
    assert_eq!(
        fs::read(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap(),
        b"on\n"
    );
}

#[test]
fn apply_custom_noop_reports_exact_text() {
    let sys = tempdir().unwrap();
    msi_identity(sys.path());
    ec_file(sys.path(), "cooler_boost", b"on\n");
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "cool",
        b"name = \"Cool\"\n\n[performance]\ncooler_boost = true\n",
    );
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "cool",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "MEC profile already applied: Cool (0 changed, 1 unchanged)\n",
        ));
}

#[test]
fn apply_custom_unsupported_fails_nonzero() {
    let sys = tempdir().unwrap();
    msi_identity(sys.path());
    fs::create_dir_all(sys.path().join("sys/devices/platform/msi-ec")).unwrap();
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "cool",
        b"name = \"Cool\"\n\n[performance]\ncooler_boost = true\n",
    );
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "cool",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC profile failed"));
}

#[test]
fn apply_builtin_gaming_reports_counts() {
    let sys = tempdir().unwrap();
    gaming_root(sys.path());
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "gaming",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "MEC profile applied: Gaming (4 changed, 0 unchanged)\n",
        ));
    assert_eq!(
        fs::read(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap(),
        b"on\n"
    );
}

#[test]
fn apply_builtin_respects_capability_omission() {
    let sys = tempdir().unwrap();
    msi_identity(sys.path());
    ec_file(sys.path(), "cooler_boost", b"off\n");
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "maximum-cooling",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "MEC profile applied: Maximum Cooling (1 changed, 0 unchanged)\n",
        ));
}

#[test]
fn apply_after_capability_loss_fails_closed() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "cool",
        b"name = \"Cool\"\n\n[performance]\ncooler_boost = true\n",
    );
    let sys_arg = sys.path().to_string_lossy().to_string();
    // Baseline show succeeds against the present capability.
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["--sys-root", &sys_arg, "profile", "show", "cool"])
        .assert()
        .success();
    // Remove the capability; the fixed custom request must now fail closed.
    fs::remove_file(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["--sys-root", &sys_arg, "profile", "apply", "cool"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC profile failed"));
}

#[test]
fn apply_non_msi_cannot_write() {
    let sys = tempdir().unwrap();
    let dmi = sys.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("sys_vendor"), b"Dell Inc.\n").unwrap();
    fs::write(dmi.join("product_name"), b"XPS 15\n").unwrap();
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "cool",
        b"name = \"Cool\"\n\n[performance]\ncooler_boost = true\n",
    );
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "apply",
            "cool",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC profile failed"));
}

#[test]
fn unknown_profile_fails_on_stderr_nonzero() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "nope",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC profile failed"));
}

#[test]
fn failures_go_to_stderr_not_stdout() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    let output = mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args([
            "--sys-root",
            &sys.path().to_string_lossy(),
            "profile",
            "show",
            "nope",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().is_empty());
    assert!(!String::from_utf8(output.stderr).unwrap().is_empty());
}

#[test]
fn previous_show_grants_no_authorization() {
    let sys = tempdir().unwrap();
    cooler_root(sys.path());
    let config = tempdir().unwrap();
    custom_profile(
        config.path(),
        "cool",
        b"name = \"Cool\"\n\n[performance]\ncooler_boost = true\n",
    );
    let sys_arg = sys.path().to_string_lossy().to_string();
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["--sys-root", &sys_arg, "profile", "show", "cool"])
        .assert()
        .success();
    // Showing changed nothing; applying still performs the write.
    assert_eq!(
        fs::read(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap(),
        b"off\n"
    );
    mec()
        .env("XDG_CONFIG_HOME", config.path())
        .args(["--sys-root", &sys_arg, "profile", "apply", "cool"])
        .assert()
        .success();
    assert_eq!(
        fs::read(sys.path().join("sys/devices/platform/msi-ec/cooler_boost")).unwrap(),
        b"on\n"
    );
}
