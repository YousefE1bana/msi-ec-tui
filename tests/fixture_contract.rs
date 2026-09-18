//! Contract tests for the committed fake sysfs fixtures.
//!
//! These tests use the fixture directories in place, resolved relative to
//! `CARGO_MANIFEST_DIR`, so they never depend on the working directory.

use std::path::PathBuf;

use mec::hardware::{
    EcBackend, LinuxSysfsReader, MsiEcBackend, ReadOnlyReason, SupportEvaluator, SupportMode,
    SystemPaths,
};

fn fixture_root(name: &str) -> SystemPaths {
    let root: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "fixtures", name]
        .iter()
        .collect();
    assert!(root.is_dir(), "missing fixture: {}", root.display());
    SystemPaths::new(root)
}

fn backend(name: &str) -> MsiEcBackend<LinuxSysfsReader> {
    MsiEcBackend::new(fixture_root(name), LinuxSysfsReader)
}

fn support_mode(name: &str) -> SupportMode {
    SupportEvaluator::new(fixture_root(name), LinuxSysfsReader).evaluate()
}

#[test]
fn gf63_fixture_is_ready() {
    assert_eq!(support_mode("gf63"), SupportMode::Ready);
}

#[test]
fn gf63_device_identity() {
    let device = backend("gf63").detect_device().unwrap();
    assert_eq!(device.manufacturer, "Micro-Star International Co., Ltd.");
    assert_eq!(device.product_name, "GF63 Thin 11UC");
    assert_eq!(device.board_name, Some("MS-16R6".to_owned()));
    assert_eq!(device.bios_version, Some("E16R6IMS.10B".to_owned()));
    assert_eq!(device.ec_firmware_version, Some("16R6EMS1.107".to_owned()));
}

#[test]
fn gf63_major_capabilities_are_present() {
    let capabilities = backend("gf63").capabilities().unwrap();
    assert!(capabilities.cpu_temperature);
    assert!(capabilities.gpu_temperature);
    assert!(capabilities.cpu_fan);
    assert!(capabilities.gpu_fan);
    assert!(!capabilities.fan_modes.is_empty());
    assert!(!capabilities.shift_modes.is_empty());
    assert!(capabilities.cooler_boost);
    assert!(capabilities.super_battery);
    assert!(capabilities.webcam);
    assert!(capabilities.webcam_block);
    assert!(capabilities.fn_key);
    assert!(capabilities.win_key);
    assert!(capabilities.keyboard_backlight.is_some());
    assert!(capabilities.battery_thresholds);
}

#[test]
fn gf63_snapshot_matches_representative_values() {
    let snapshot = backend("gf63").snapshot().unwrap();
    assert_eq!(snapshot.cpu_temperature.unwrap().get(), 63);
    assert_eq!(snapshot.gpu_temperature.unwrap().get(), 51);
    assert_eq!(snapshot.cpu_fan.unwrap().get(), 42);
    assert_eq!(snapshot.gpu_fan.unwrap().get(), 31);
    assert_eq!(snapshot.fan_mode.unwrap().as_str(), "auto");
    assert_eq!(snapshot.shift_mode.unwrap().as_str(), "comfort");
    assert_eq!(snapshot.cooler_boost, Some(false));
    assert_eq!(snapshot.super_battery, Some(false));
    assert_eq!(snapshot.webcam, Some(true));
    assert_eq!(snapshot.webcam_block, Some(false));
    assert_eq!(snapshot.keyboard_backlight, Some(2));
    assert_eq!(snapshot.battery_start_threshold, Some(50));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}

#[test]
fn partial_fixture_is_ready() {
    assert_eq!(support_mode("partial-device"), SupportMode::Ready);
}

#[test]
fn partial_fixture_exposes_only_its_subset() {
    let snapshot = backend("partial-device").snapshot().unwrap();
    assert_eq!(snapshot.cpu_temperature.unwrap().get(), 57);
    assert_eq!(snapshot.webcam, Some(false));
    assert_eq!(snapshot.gpu_temperature, None);
    assert_eq!(snapshot.cpu_fan, None);
    assert_eq!(snapshot.gpu_fan, None);
    assert_eq!(snapshot.fan_mode, None);
    assert_eq!(snapshot.shift_mode, None);
    assert_eq!(snapshot.cooler_boost, None);
    assert_eq!(snapshot.super_battery, None);
    assert_eq!(snapshot.webcam_block, None);
    assert_eq!(snapshot.keyboard_backlight, None);
    assert_eq!(snapshot.battery_start_threshold, None);
    assert_eq!(snapshot.battery_end_threshold, None);
}

#[test]
fn unknown_model_fixture_is_ready() {
    assert_eq!(support_mode("unknown-device"), SupportMode::Ready);
}

#[test]
fn unknown_model_identity_is_preserved() {
    let device = backend("unknown-device").detect_device().unwrap();
    assert_eq!(device.manufacturer, "Micro-Star International");
    assert_eq!(device.product_name, "Future MSI Model XYZ-999");
}

#[test]
fn unknown_model_capabilities_need_no_model_logic() {
    let capabilities = backend("unknown-device").capabilities().unwrap();
    assert!(capabilities.cpu_temperature);
    assert!(capabilities.cpu_fan);
    assert!(capabilities.cooler_boost);
    assert!(!capabilities.gpu_temperature);
    assert!(!capabilities.webcam);
}

#[test]
fn unknown_model_future_fan_mode_survives() {
    let capabilities = backend("unknown-device").capabilities().unwrap();
    let names: Vec<&str> = capabilities
        .fan_modes
        .iter()
        .map(mec::hardware::FanMode::as_str)
        .collect();
    assert_eq!(names, vec!["auto", "adaptive_ai"]);
    let snapshot = backend("unknown-device").snapshot().unwrap();
    assert_eq!(snapshot.fan_mode.unwrap().as_str(), "adaptive_ai");
}

#[test]
fn broken_fixture_is_read_only_inconsistent() {
    assert_eq!(
        support_mode("broken-sysfs"),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn broken_fixture_reaches_coherence_layer() {
    // Identity and transport succeed, so the verdict must come from the
    // capability-coherence policy rather than an earlier layer.
    let device = backend("broken-sysfs").detect_device().unwrap();
    assert_eq!(device.manufacturer, "MSI");
    assert_eq!(device.product_name, "Broken Interface Test Fixture");
    assert_ne!(
        support_mode("broken-sysfs"),
        SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware)
    );
    assert_ne!(
        support_mode("broken-sysfs"),
        SupportMode::ReadOnly(ReadOnlyReason::UnverifiedHardwareIdentity)
    );
    assert_ne!(
        support_mode("broken-sysfs"),
        SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable)
    );
}
