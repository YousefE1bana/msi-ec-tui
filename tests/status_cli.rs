//! CLI integration tests for `mec status` against committed fixtures.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;

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
fn status_help_succeeds() {
    mec()
        .args(["status", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Status").or(predicates::str::contains("status")));
}

#[test]
fn top_help_lists_status() {
    mec()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicates::str::contains("status"));
}

#[test]
fn gf_fixture_status_exits_successfully() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success();
}

#[test]
fn gf_fixture_status_header() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Status"));
}

#[test]
fn gf_fixture_status_identity() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Device: GF63 Thin 11UC"));
}

#[test]
fn gf_fixture_status_mode_ready() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn gf_fixture_status_temperatures() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("CPU Temperature: 63°C"))
        .stdout(predicates::str::contains("GPU Temperature: 51°C"));
}

#[test]
fn gf_fixture_status_fan_values() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("CPU Fan: 42%"))
        .stdout(predicates::str::contains("GPU Fan: 31%"));
}

#[test]
fn gf_fixture_status_has_no_rpm_terminology() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("RPM").not())
        .stdout(predicates::str::contains("rpm").not());
}

#[test]
fn gf_fixture_status_modes() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Fan Mode: auto"))
        .stdout(predicates::str::contains("Shift Mode: comfort"));
}

#[test]
fn gf_fixture_status_booleans() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Cooler Boost: Off"))
        .stdout(predicates::str::contains("Super Battery: Off"))
        .stdout(predicates::str::contains("Webcam: On"))
        .stdout(predicates::str::contains("Webcam Block: Off"));
}

#[test]
fn gf_fixture_status_backlight() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Keyboard Backlight: 2"));
}

#[test]
fn gf_fixture_status_battery() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Charge: 77%"))
        .stdout(predicates::str::contains("State: Charging"))
        .stdout(predicates::str::contains("AC: Connected"))
        .stdout(predicates::str::contains("Start Threshold: 50%"))
        .stdout(predicates::str::contains("End Threshold: 80%"));
}

#[test]
fn partial_fixture_status_exits_successfully() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Device: Modern 15 Test Fixture"))
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn partial_fixture_status_identity_and_mode() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Device: Modern 15 Test Fixture"))
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn partial_fixture_status_cpu_temperature() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("CPU Temperature: 57°C"));
}

#[test]
fn partial_fixture_status_webcam() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Webcam: Off"));
}

#[test]
fn partial_fixture_unsupported_fields_render_na() {
    mec()
        .args(["--sys-root", &fixture_arg("partial-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("GPU Temperature: N/A"))
        .stdout(predicates::str::contains("CPU Fan: N/A"))
        .stdout(predicates::str::contains("Fan Mode: N/A"))
        .stdout(predicates::str::contains("Keyboard Backlight: N/A"))
        .stdout(predicates::str::contains("Charge: N/A"))
        .stdout(predicates::str::contains("AC: N/A"));
}

#[test]
fn unknown_fixture_status_exits_successfully() {
    mec()
        .args(["--sys-root", &fixture_arg("unknown-device"), "status"])
        .assert()
        .success();
}

#[test]
fn unknown_fixture_status_preserves_product_name() {
    mec()
        .args(["--sys-root", &fixture_arg("unknown-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Future MSI Model XYZ-999"));
}

#[test]
fn unknown_fixture_status_preserves_future_fan_mode() {
    mec()
        .args(["--sys-root", &fixture_arg("unknown-device"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Fan Mode: adaptive_ai"));
}

#[test]
fn relative_sys_root_status_works() {
    mec()
        .current_dir(manifest_dir())
        .args(["--sys-root", "./tests/fixtures/gf63", "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn bare_mec_remains_exact_banner() {
    mec()
        .assert()
        .success()
        .stdout("MEC — MSI EC Control Center\n");
}

#[test]
fn doctor_remains_available() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn broken_snapshot_fails_cleanly() {
    mec()
        .args(["--sys-root", &fixture_arg("broken-sysfs"), "status"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("MEC status unavailable:"))
        .stderr(predicates::str::contains("panicked").not());
}
