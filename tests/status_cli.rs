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
        .stdout(predicates::str::contains("Start Threshold: 70%"))
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
fn broken_snapshot_degrades_cleanly() {
    mec()
        .args(["--sys-root", &fixture_arg("broken-sysfs"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Status"))
        .stdout(predicates::str::contains(
            "Device: Broken Interface Test Fixture",
        ))
        .stdout(predicates::str::contains("Mode: READ-ONLY"))
        .stdout(predicates::str::contains("CPU Temperature: N/A"))
        .stdout(predicates::str::contains("CPU Fan: N/A"))
        .stdout(predicates::str::contains("Charge: N/A"))
        .stdout(predicates::str::contains("panicked").not())
        .stderr(predicates::str::contains("MEC status degraded:"))
        .stderr(predicates::str::contains("panicked").not());
}

#[test]
fn healthy_status_has_empty_stderr() {
    for fixture in ["gf63", "partial-device", "unknown-device"] {
        mec()
            .args(["--sys-root", &fixture_arg(fixture), "status"])
            .assert()
            .success()
            .stderr(predicates::str::is_empty());
    }
}

#[test]
fn identity_failure_stays_fatal() {
    mec()
        .args(["--sys-root", "/nonexistent-mec-fixture-root", "status"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC status unavailable:"));
}

fn status_json_output(args: &[String]) -> serde_json::Value {
    let output = mec().args(args).assert().success().get_output().clone();
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("MEC Status"),
        "JSON output must not contain human-readable prose"
    );
    serde_json::from_slice(&output.stdout).expect("stdout must be valid JSON")
}

fn gf_json_args() -> Vec<String> {
    vec![
        "--sys-root".to_owned(),
        fixture_arg("gf63"),
        "status".to_owned(),
        "--json".to_owned(),
    ]
}

fn gf_json() -> serde_json::Value {
    let owned = gf_json_args();
    status_json_output(&owned)
}

#[test]
fn status_help_lists_json_flag() {
    mec()
        .args(["status", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains("--json"));
}

#[test]
fn gf_status_json_exits_successfully() {
    mec().args(gf_json_args()).assert().success();
}

#[test]
fn gf_status_json_is_single_document() {
    let output = mec()
        .args(gf_json_args())
        .assert()
        .success()
        .get_output()
        .clone();
    let text = String::from_utf8(output.stdout).expect("stdout must be UTF-8");
    assert!(!text.contains("MEC Status"));
    let value: serde_json::Value = text.parse().expect("exactly one JSON document");
    assert!(value.is_object());
}

#[test]
fn gf_status_json_device() {
    let json = gf_json();
    assert_eq!(
        json["device"]["manufacturer"],
        "Micro-Star International Co., Ltd."
    );
    assert_eq!(json["device"]["product_name"], "GF63 Thin 11UC");
    assert_eq!(json["device"]["board_name"], "MS-16R6");
    assert_eq!(json["device"]["bios_version"], "E16R6IMS.10B");
    assert_eq!(json["device"]["ec_firmware_version"], "16R6EMS1.107");
}

#[test]
fn gf_status_json_mode_ready() {
    assert_eq!(gf_json()["mode"], "READY");
}

#[test]
fn gf_status_json_thermals() {
    let json = gf_json();
    assert_eq!(json["thermals"]["cpu_celsius"], 63);
    assert_eq!(json["thermals"]["gpu_celsius"], 51);
}

#[test]
fn gf_status_json_fans() {
    let json = gf_json();
    assert_eq!(json["fans"]["cpu_percent"], 42);
    assert_eq!(json["fans"]["gpu_percent"], 31);
    assert_eq!(json["fans"]["mode"], "auto");
}

#[test]
fn gf_status_json_performance() {
    let json = gf_json();
    assert_eq!(json["performance"]["shift_mode"], "comfort");
    assert_eq!(json["performance"]["cooler_boost"], false);
    assert_eq!(json["performance"]["super_battery"], false);
}

#[test]
fn gf_status_json_battery() {
    let json = gf_json();
    assert_eq!(json["battery"]["percentage"], 77);
    assert_eq!(json["battery"]["status"], "Charging");
    assert_eq!(json["battery"]["ac_connected"], true);
    assert_eq!(json["battery"]["start_threshold_percent"], 70);
    assert_eq!(json["battery"]["end_threshold_percent"], 80);
}

#[test]
fn gf_status_json_devices() {
    let json = gf_json();
    assert_eq!(json["devices"]["webcam"], true);
    assert_eq!(json["devices"]["webcam_block"], false);
    assert_eq!(json["devices"]["keyboard_backlight"], 2);
}

#[test]
fn gf_status_json_uses_native_numbers() {
    let json = gf_json();
    for pointer in [
        "/thermals/cpu_celsius",
        "/thermals/gpu_celsius",
        "/fans/cpu_percent",
        "/fans/gpu_percent",
        "/battery/percentage",
        "/battery/start_threshold_percent",
        "/battery/end_threshold_percent",
        "/devices/keyboard_backlight",
    ] {
        assert!(
            json.pointer(pointer)
                .is_some_and(serde_json::Value::is_number),
            "{pointer} must be a JSON number"
        );
    }
}

#[test]
fn gf_status_json_uses_native_booleans() {
    let json = gf_json();
    for pointer in [
        "/performance/cooler_boost",
        "/performance/super_battery",
        "/battery/ac_connected",
        "/devices/webcam",
        "/devices/webcam_block",
    ] {
        assert!(
            json.pointer(pointer)
                .is_some_and(serde_json::Value::is_boolean),
            "{pointer} must be a JSON boolean"
        );
    }
}

#[test]
fn gf_status_json_has_no_na_placeholders() {
    let output = mec()
        .args(gf_json_args())
        .assert()
        .success()
        .get_output()
        .clone();
    let text = String::from_utf8(output.stdout).expect("stdout must be UTF-8");
    assert!(!text.contains("N/A"));
    assert!(!text.contains("°C"));
}

#[test]
fn partial_status_json_reports_nulls() {
    let output = mec()
        .args([
            "--sys-root",
            &fixture_arg("partial-device"),
            "status",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    for pointer in [
        "/thermals/gpu_celsius",
        "/fans/cpu_percent",
        "/fans/mode",
        "/battery/percentage",
        "/battery/ac_connected",
        "/devices/keyboard_backlight",
    ] {
        assert_eq!(
            json.pointer(pointer),
            Some(&serde_json::Value::Null),
            "{pointer} must be explicit null"
        );
    }
    assert_eq!(json["devices"]["webcam"], false);
}

#[test]
fn unknown_status_json_preserves_names() {
    let output = mec()
        .args([
            "--sys-root",
            &fixture_arg("unknown-device"),
            "status",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["device"]["product_name"], "Future MSI Model XYZ-999");
    assert_eq!(json["fans"]["mode"], "adaptive_ai");
}

#[test]
fn relative_sys_root_status_json_works() {
    mec()
        .current_dir(manifest_dir())
        .args(["--sys-root", "./tests/fixtures/gf63", "status", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("\"READY\""));
}

#[test]
fn plain_status_remains_human_readable() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status"])
        .assert()
        .success()
        .stdout(predicates::str::contains("MEC Status"))
        .stdout(predicates::str::contains("Device: GF63 Thin 11UC"))
        .stdout(predicates::str::contains("Mode: READY"))
        .stdout(predicates::str::contains("CPU Temperature: 63°C"))
        .stdout(predicates::str::contains("CPU Fan: 42%"))
        .stdout(predicates::str::contains("Charge: 77%"))
        .stdout(predicates::str::contains("AC: Connected"));
}

#[test]
fn bare_mec_banner_unchanged_by_json_task() {
    mec()
        .assert()
        .success()
        .stdout("MEC — MSI EC Control Center\n");
}

#[test]
fn doctor_unaffected_by_json_task() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Mode: READY"));
}

#[test]
fn broken_status_json_degrades_cleanly() {
    let output = mec()
        .args([
            "--sys-root",
            &fixture_arg("broken-sysfs"),
            "status",
            "--json",
        ])
        .assert()
        .success()
        .stderr(predicates::str::contains("MEC status degraded:"))
        .stderr(predicates::str::contains("panicked").not())
        .get_output()
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["device"]["product_name"],
        "Broken Interface Test Fixture"
    );
    assert_eq!(json["mode"], "READ-ONLY");
    for pointer in [
        "/thermals/cpu_celsius",
        "/thermals/gpu_celsius",
        "/fans/cpu_percent",
        "/fans/mode",
        "/battery/percentage",
        "/battery/ac_connected",
        "/devices/webcam",
    ] {
        assert_eq!(
            json.pointer(pointer),
            Some(&serde_json::Value::Null),
            "{pointer} must be explicit null"
        );
    }
    assert!(!String::from_utf8_lossy(&output.stdout).contains("MEC status degraded:"));
}

#[test]
fn healthy_json_has_empty_stderr() {
    mec()
        .args(["--sys-root", &fixture_arg("gf63"), "status", "--json"])
        .assert()
        .success()
        .stderr(predicates::str::is_empty());
}

#[test]
fn identity_failure_json_stays_fatal() {
    mec()
        .args([
            "--sys-root",
            "/nonexistent-mec-fixture-root",
            "status",
            "--json",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("MEC status unavailable:"));
}

#[test]
fn json_flag_is_scoped_to_status() {
    mec()
        .args(["--json", "doctor", "--sys-root", &fixture_arg("gf63")])
        .assert()
        .failure();
}
