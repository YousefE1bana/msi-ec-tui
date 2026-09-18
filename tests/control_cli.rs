//! CLI integration tests for `mec` hardware controls.
//!
//! Write-capable tests NEVER touch `/`, the tracked fixtures, or real
//! hardware: every one builds a minimal fake sysfs tree under a fresh
//! `TempDir` and passes it via `--sys-root`.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use tempfile::{TempDir, tempdir};

fn mec() -> Command {
    Command::cargo_bin("mec").unwrap()
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_arg(name: &str) -> String {
    manifest_dir()
        .join("tests")
        .join("fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// Test-only minimal fake root: MSI identity plus an enumerable but empty
/// driver root, extended per test with only the needed capability files.
struct FakeRoot {
    _dir: TempDir,
    root: PathBuf,
}

impl FakeRoot {
    fn ready() -> Self {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let dmi = root.join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).unwrap();
        fs::write(
            dmi.join("sys_vendor"),
            b"Micro-Star International Co., Ltd.\n",
        )
        .unwrap();
        fs::write(dmi.join("product_name"), b"Test MSI Laptop\n").unwrap();
        fs::create_dir_all(root.join("sys/devices/platform/msi-ec")).unwrap();
        Self { _dir: dir, root }
    }

    fn arg(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    fn ec_file(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.root.join("sys/devices/platform/msi-ec").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        path
    }

    fn read_ec(&self, relative: &str) -> Vec<u8> {
        fs::read(self.root.join("sys/devices/platform/msi-ec").join(relative)).unwrap()
    }

    fn enable_fan_modes(&self, current: &str, available: &[&str]) {
        self.ec_file("fan_mode", format!("{current}\n").as_bytes());
        self.ec_file(
            "available_fan_modes",
            (available.join("\n") + "\n").as_bytes(),
        );
    }
}

#[test]
fn top_help_lists_all_controls() {
    let assert = mec().arg("--help").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    for name in [
        "fan",
        "shift",
        "cooler-boost",
        "super-battery",
        "webcam",
        "webcam-block",
        "keyboard-backlight",
        "battery",
    ] {
        assert!(stdout.contains(name), "top help must list {name}");
    }
}

#[test]
fn fan_help_lists_mode() {
    mec()
        .args(["fan", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("mode"));
}

#[test]
fn battery_help_lists_limit() {
    mec()
        .args(["battery", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("limit"));
}

#[test]
fn on_off_controls_reject_third_value() {
    mec()
        .args(["cooler-boost", "maybe"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("MEC control applied").not());
}

#[test]
fn keyboard_backlight_rejects_outside_u8() {
    mec()
        .args(["keyboard-backlight", "999"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("MEC control applied").not());
}

#[test]
fn battery_limit_rejects_single_digit() {
    mec()
        .args(["battery", "limit", "9"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("MEC control applied").not());
}

#[test]
fn battery_limit_rejects_above_hundred() {
    mec()
        .args(["battery", "limit", "101"])
        .assert()
        .failure()
        .stdout(predicates::str::contains("MEC control applied").not());
}

#[test]
fn fan_mode_writes_through_with_exact_output() {
    let root = FakeRoot::ready();
    root.enable_fan_modes("silent", &["auto", "silent"]);
    mec()
        .args(["--sys-root", &root.arg(), "fan", "mode", "auto"])
        .assert()
        .success()
        .stdout("MEC control applied: fan mode = auto\n")
        .stderr("");
    assert_eq!(root.read_ec("fan_mode"), b"auto\n");
}

#[test]
fn unadvertised_fan_mode_fails_before_mutation() {
    let root = FakeRoot::ready();
    root.enable_fan_modes("auto", &["auto"]);
    mec()
        .args(["--sys-root", &root.arg(), "fan", "mode", "turbo"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::starts_with("MEC control failed:"));
    assert_eq!(root.read_ec("fan_mode"), b"auto\n");
}

#[test]
fn shift_mode_writes_through() {
    let root = FakeRoot::ready();
    root.ec_file("shift_mode", b"eco\n");
    root.ec_file("available_shift_modes", b"eco\ncomfort\n");
    mec()
        .args(["--sys-root", &root.arg(), "shift", "comfort"])
        .assert()
        .success()
        .stdout("MEC control applied: shift mode = comfort\n")
        .stderr("");
    assert_eq!(root.read_ec("shift_mode"), b"comfort\n");
}

#[test]
fn cooler_boost_on_writes_through() {
    let root = FakeRoot::ready();
    let target = root.ec_file("cooler_boost", b"off\n");
    mec()
        .args(["--sys-root", &root.arg(), "cooler-boost", "on"])
        .assert()
        .success()
        .stdout("MEC control applied: cooler boost = on\n")
        .stderr("");
    assert_eq!(fs::read(&target).unwrap(), b"on\n");
}

#[test]
fn cooler_boost_off_writes_through() {
    let root = FakeRoot::ready();
    let target = root.ec_file("cooler_boost", b"on\n");
    mec()
        .args(["--sys-root", &root.arg(), "cooler-boost", "off"])
        .assert()
        .success()
        .stdout("MEC control applied: cooler boost = off\n")
        .stderr("");
    assert_eq!(fs::read(&target).unwrap(), b"off\n");
}

#[test]
fn super_battery_writes_both_states() {
    for (state, start) in [("on", "off\n"), ("off", "on\n")] {
        let root = FakeRoot::ready();
        let target = root.ec_file("super_battery", start.as_bytes());
        mec()
            .args(["--sys-root", &root.arg(), "super-battery", state])
            .assert()
            .success()
            .stdout(format!("MEC control applied: super battery = {state}\n"))
            .stderr("");
        let expected = format!("{state}\n");
        assert_eq!(fs::read(&target).unwrap(), expected.as_bytes());
    }
}

#[test]
fn webcam_writes_both_states() {
    for (state, start) in [("on", "off\n"), ("off", "on\n")] {
        let root = FakeRoot::ready();
        let target = root.ec_file("webcam", start.as_bytes());
        mec()
            .args(["--sys-root", &root.arg(), "webcam", state])
            .assert()
            .success()
            .stdout(format!("MEC control applied: webcam = {state}\n"))
            .stderr("");
        let expected = format!("{state}\n");
        assert_eq!(fs::read(&target).unwrap(), expected.as_bytes());
    }
}

#[test]
fn webcam_block_writes_both_states() {
    for (state, start) in [("on", "off\n"), ("off", "on\n")] {
        let root = FakeRoot::ready();
        let target = root.ec_file("webcam_block", start.as_bytes());
        mec()
            .args(["--sys-root", &root.arg(), "webcam-block", state])
            .assert()
            .success()
            .stdout(format!("MEC control applied: webcam block = {state}\n"))
            .stderr("");
        let expected = format!("{state}\n");
        assert_eq!(fs::read(&target).unwrap(), expected.as_bytes());
    }
}

#[test]
fn keyboard_backlight_writes_through() {
    let root = FakeRoot::ready();
    let dir = root.root.join("sys/class/leds/msiacpi::kbd_backlight");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("brightness"), b"1\n").unwrap();
    fs::write(dir.join("max_brightness"), b"3\n").unwrap();
    mec()
        .args(["--sys-root", &root.arg(), "keyboard-backlight", "2"])
        .assert()
        .success()
        .stdout("MEC control applied: keyboard backlight = 2\n")
        .stderr("");
    assert_eq!(fs::read(dir.join("brightness")).unwrap(), b"2\n");
}

#[test]
fn keyboard_backlight_above_max_fails_before_mutation() {
    let root = FakeRoot::ready();
    let dir = root.root.join("sys/class/leds/msiacpi::kbd_backlight");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("brightness"), b"1\n").unwrap();
    fs::write(dir.join("max_brightness"), b"3\n").unwrap();
    mec()
        .args(["--sys-root", &root.arg(), "keyboard-backlight", "4"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::starts_with("MEC control failed:"));
    assert_eq!(fs::read(dir.join("brightness")).unwrap(), b"1\n");
}

#[test]
fn battery_limit_writes_end_only_with_exact_output() {
    let root = FakeRoot::ready();
    let entry = root.root.join("sys/class/power_supply/BAT0");
    fs::create_dir_all(&entry).unwrap();
    fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
    fs::write(entry.join("charge_control_end_threshold"), b"60\n").unwrap();
    mec()
        .args(["--sys-root", &root.arg(), "battery", "limit", "80"])
        .assert()
        .success()
        .stdout("MEC control applied: battery limit = 80%\n")
        .stderr("");
    assert_eq!(
        fs::read(entry.join("charge_control_start_threshold")).unwrap(),
        b"70\n"
    );
    assert_eq!(
        fs::read(entry.join("charge_control_end_threshold")).unwrap(),
        b"80\n"
    );
}

#[test]
fn unsupported_battery_thresholds_fail_without_creating_tree() {
    let root = FakeRoot::ready();
    mec()
        .args(["--sys-root", &root.arg(), "battery", "limit", "80"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::starts_with("MEC control failed:"));
    assert!(
        !root.root.join("sys/class/power_supply").exists(),
        "failing control must not create sysfs trees"
    );
}

#[test]
fn readonly_identity_fails_without_mutation() {
    let dir = tempdir().unwrap();
    let dmi = dir.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("sys_vendor"), b"Dell Inc.\n").unwrap();
    fs::write(dmi.join("product_name"), b"XPS 15\n").unwrap();
    let ec = dir.path().join("sys/devices/platform/msi-ec");
    fs::create_dir_all(&ec).unwrap();
    fs::write(ec.join("cooler_boost"), b"off\n").unwrap();
    let arg = dir.path().to_string_lossy().into_owned();
    mec()
        .args(["--sys-root", &arg, "cooler-boost", "on"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::starts_with("MEC control failed:"));
    assert_eq!(fs::read(ec.join("cooler_boost")).unwrap(), b"off\n");
}

#[test]
fn missing_msi_ec_root_fails_without_creating_tree() {
    let dir = tempdir().unwrap();
    let dmi = dir.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
    fs::write(dmi.join("product_name"), b"Test MSI Laptop\n").unwrap();
    let arg = dir.path().to_string_lossy().into_owned();
    mec()
        .args(["--sys-root", &arg, "cooler-boost", "on"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::starts_with("MEC control failed:"));
    assert!(
        !dir.path().join("sys/devices").exists(),
        "failing control must not create sysfs trees"
    );
}

#[test]
fn missing_target_cannot_be_created_through_cli() {
    let root = FakeRoot::ready();
    mec()
        .args(["--sys-root", &root.arg(), "fan", "mode", "auto"])
        .assert()
        .failure()
        .stderr(predicates::str::starts_with("MEC control failed:"));
    assert!(
        !root
            .root
            .join("sys/devices/platform/msi-ec/fan_mode")
            .exists(),
        "failing control must not create the missing node"
    );
}

#[test]
fn boundary_failure_produces_typed_stderr() {
    // A directory where the node belongs passes presence discovery and
    // validation, but the restricted write fails deterministically.
    let root = FakeRoot::ready();
    fs::create_dir_all(root.root.join("sys/devices/platform/msi-ec/fan_mode")).unwrap();
    root.ec_file("available_fan_modes", b"auto\n");
    mec()
        .args(["--sys-root", &root.arg(), "fan", "mode", "auto"])
        .assert()
        .failure()
        .stdout("")
        .stderr(
            predicates::str::starts_with("MEC control failed:")
                .and(predicates::str::contains("hardware write failed")),
        );
}

#[test]
fn bare_binary_still_prints_exact_banner() {
    mec()
        .assert()
        .success()
        .stdout("MEC — MSI EC Control Center\n");
}

#[test]
fn status_still_works() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Status"))
        .stdout(predicates::str::contains("MEC control applied").not());
}

#[test]
fn status_json_still_works() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("start_threshold_percent"));
}

#[test]
fn doctor_still_works() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn monitor_help_still_works() {
    mec()
        .args(["monitor", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("interval"));
}

#[test]
fn incomplete_control_exits_without_tui() {
    // A control missing its required argument must end in a Clap usage
    // error, never in an interactive session.
    mec()
        .args(["fan"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Usage"));
}
