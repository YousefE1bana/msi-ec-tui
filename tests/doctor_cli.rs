//! CLI integration tests for `mec doctor` against committed fixtures.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use tempfile::tempdir;

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

fn mec() -> Command {
    Command::cargo_bin("mec").unwrap()
}

#[test]
fn no_arguments_prints_banner_exactly() {
    mec()
        .assert()
        .success()
        .stdout("MEC — MSI EC Control Center\n");
}

#[test]
fn help_exposes_doctor_and_sys_root() {
    mec()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("doctor").and(predicates::str::contains("--sys-root")));
}

#[test]
fn gf_fixture_doctor_exits_successfully() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success();
}

#[test]
fn gf_fixture_reports_ready_identity() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("GF63 Thin 11UC"))
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn gf_fixture_reports_fan_controls_available() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Fan controls available"));
}

#[test]
fn gf_fixture_reports_shift_controls_available() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Shift controls available"));
}

#[test]
fn gf_fixture_reports_battery_thresholds_available() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Battery thresholds available"));
}

#[test]
fn gf_fixture_reports_keyboard_backlight_available() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Keyboard backlight available"));
}

#[test]
fn partial_fixture_reports_ready() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("Modern 15 Test Fixture"))
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn partial_fixture_warns_on_missing_optionals() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("[WARN]").and(predicates::str::contains("Mode: READY")));
}

#[test]
fn unknown_fixture_exits_successfully() {
    mec()
        .args(["--sys-root", &fixture_arg("unknown-device"), "doctor"])
        .assert()
        .success();
}

#[test]
fn unknown_fixture_remains_ready() {
    mec()
        .args(["--sys-root", &fixture_arg("unknown-device"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"))
        .stdout(predicates::str::contains("Future MSI Model XYZ-999"));
}

#[test]
fn unknown_fixture_preserves_product_name() {
    mec()
        .args(["--sys-root", &fixture_arg("unknown-device"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Future MSI Model XYZ-999"));
}

#[test]
fn broken_fixture_exits_successfully() {
    mec()
        .args(["--sys-root", &fixture_arg("broken-sysfs"), "doctor"])
        .assert()
        .success();
}

#[test]
fn broken_fixture_reports_read_only() {
    mec()
        .args(["--sys-root", &fixture_arg("broken-sysfs"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("Mode: READ-ONLY"));
}

#[test]
fn broken_fixture_reports_inconsistent_reason() {
    mec()
        .args(["--sys-root", &fixture_arg("broken-sysfs"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Reason: Inconsistent hardware interface",
        ));
}

#[test]
fn broken_fixture_does_not_claim_non_msi() {
    mec()
        .args(["--sys-root", &fixture_arg("broken-sysfs"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Non-MSI hardware").not());
}

#[test]
fn relative_sys_root_works_from_repo_root() {
    mec()
        .current_dir(manifest_dir())
        .args(["--sys-root", "./tests/fixtures/gf63", "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn unknown_subcommand_fails_like_clap() {
    mec().arg("definitely-not-a-command").assert().failure();
}

#[test]
fn doctor_needs_no_real_hardware() {
    // A nonexistent root must still produce a diagnosis, never a panic.
    mec()
        .args(["--sys-root", "/nonexistent-mec-fixture-root", "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("Mode: READ-ONLY"));
}

#[test]
fn missing_sys_root_reports_unavailable_without_coherence_claim() {
    mec()
        .args(["--sys-root", "/nonexistent-mec-fixture-root", "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("Mode: READ-ONLY"))
        .stdout(predicates::str::contains("msi-ec interface unavailable"))
        .stdout(predicates::str::contains("[PASS] EC interface coherent").not());
}

#[test]
fn unreadable_sys_root_reports_unreadable_without_coherence_claim() {
    let root = tempdir().unwrap();
    let dmi = root.path().join("sys/class/dmi/id");
    std::fs::create_dir_all(&dmi).unwrap();
    std::fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
    std::fs::write(dmi.join("product_name"), b"Doctor Fixture\n").unwrap();
    // Regular file where the driver directory belongs: the root cannot be
    // enumerated, so no coherence claim may follow.
    let ec = root.path().join("sys/devices/platform/msi-ec");
    std::fs::create_dir_all(ec.parent().unwrap()).unwrap();
    std::fs::write(&ec, b"not a directory\n").unwrap();
    let sys_root = root.path().to_string_lossy().into_owned();
    mec()
        .args(["--sys-root", &sys_root, "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("Mode: READ-ONLY"))
        .stdout(predicates::str::contains("msi-ec interface unreadable"))
        .stdout(predicates::str::contains("[PASS] EC interface coherent").not());
}

#[test]
fn export_prints_compatibility_report_only() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor", "--export"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Compatibility Report"))
        .stdout(predicates::str::contains("MEC Doctor").not())
        .stdout(predicates::str::contains("GF63 Thin 11UC"))
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn export_read_only_fixture_exits_successfully() {
    mec()
        .args([
            "--sys-root",
            &fixture_arg("broken-sysfs"),
            "doctor",
            "--export",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Compatibility Report"))
        .stdout(predicates::str::contains("Mode: READ-ONLY"));
}

#[test]
fn export_contains_privacy_declaration() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor", "--export"])
        .assert()
        .success()
        .stdout(predicates::str::contains("No serial number collected"))
        .stdout(predicates::str::contains("RPM").not());
}

#[test]
fn plain_doctor_output_unchanged_by_export_flag() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Doctor"))
        .stdout(predicates::str::contains("MEC Compatibility Report").not());
}
