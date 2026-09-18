//! Capability discovery against fake `msi-ec`, power-supply, and LED trees.

use std::fs;
use std::os::unix::fs::symlink;

use tempfile::{TempDir, tempdir};

use mec::hardware::{
    BacklightCapability, Capabilities, CapabilityDetector, CapabilityDiscoveryError,
    LinuxSysfsReader, SystemPaths,
};

struct Fixture {
    root: TempDir,
    paths: SystemPaths,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        Self {
            paths: SystemPaths::new(root.path()),
            root,
        }
    }

    fn detector(&self) -> CapabilityDetector<LinuxSysfsReader> {
        CapabilityDetector::new(self.paths.clone(), LinuxSysfsReader)
    }

    fn ec_dir(&self) -> std::path::PathBuf {
        self.root.path().join("sys/devices/platform/msi-ec")
    }

    fn ensure_ec_root(&self) {
        fs::create_dir_all(self.ec_dir()).unwrap();
    }

    /// Creates an msi-ec interface file. Presence-only probes accept empty files.
    fn ec_file(&self, relative: &str, contents: &[u8]) {
        let path = self.ec_dir().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// Creates a real directory entry under a class root.
    fn class_dir(&self, class: &str, name: &str) -> std::path::PathBuf {
        let path = self.root.path().join("sys/class").join(class).join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// Creates a symlink entry under a class root pointing at a real directory.
    fn class_symlink(&self, class: &str, name: &str, target: &std::path::Path) {
        let class_dir = self.root.path().join("sys/class").join(class);
        fs::create_dir_all(&class_dir).unwrap();
        symlink(target, class_dir.join(name)).unwrap();
    }

    fn write_in(&self, entry: &std::path::Path, name: &str, contents: &[u8]) {
        fs::write(entry.join(name), contents).unwrap();
    }

    /// Populates the complete fan/shift mode pairs with upstream-style lists.
    fn enable_mode_pairs(&self) {
        self.ensure_ec_root();
        self.ec_file("fan_mode", b"auto\n");
        self.ec_file("available_fan_modes", b"auto\nsilent\nbasic\nadvanced\n");
        self.ec_file("shift_mode", b"comfort\n");
        self.ec_file("available_shift_modes", b"eco\ncomfort\nsport\nturbo\n");
    }

    /// Populates the GF-style baseline: every documented simple EC capability.
    fn enable_full_simple_interface(&self) {
        self.ensure_ec_root();
        for relative in [
            "cpu/realtime_temperature",
            "cpu/realtime_fan_speed",
            "gpu/realtime_temperature",
            "gpu/realtime_fan_speed",
            "cooler_boost",
            "super_battery",
            "webcam",
            "webcam_block",
            "fn_key",
            "win_key",
        ] {
            self.ec_file(relative, b"0\n");
        }
    }
}

fn expected_full_gf_style() -> Capabilities {
    Capabilities {
        cpu_temperature: true,
        gpu_temperature: true,
        cpu_fan: true,
        gpu_fan: true,
        fan_modes: modes(["auto", "silent", "basic", "advanced"]),
        shift_modes: shift_modes(["eco", "comfort", "sport", "turbo"]),
        cooler_boost: true,
        super_battery: true,
        webcam: true,
        webcam_block: true,
        fn_key: true,
        win_key: true,
        keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
        battery_thresholds: true,
    }
}

fn modes<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<mec::hardware::FanMode> {
    names
        .into_iter()
        .map(|name| mec::hardware::FanMode::try_from(name).unwrap())
        .collect()
}

fn shift_modes<const N: usize>(names: [&str; N]) -> Vec<mec::hardware::ShiftMode> {
    names
        .into_iter()
        .map(mec::hardware::ShiftMode::try_from)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn full_gf_style_interface_discovers_all_capabilities() {
    let fixture = Fixture::new();
    fixture.enable_full_simple_interface();
    fixture.enable_mode_pairs();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"40\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    fixture.write_in(&backlight, "max_brightness", b"3\n");

    assert_eq!(
        fixture.detector().discover().unwrap(),
        expected_full_gf_style()
    );
}

#[test]
fn partial_interface_exposes_only_present_capabilities() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("cpu/realtime_temperature", b"0\n");
    fixture.ec_file("fn_key", b"0\n");
    let discovered = fixture.detector().discover().unwrap();
    assert!(discovered.cpu_temperature);
    assert!(discovered.fn_key);
    assert!(!discovered.gpu_temperature);
    assert!(!discovered.gpu_fan);
    assert!(!discovered.cpu_fan);
    assert!(!discovered.cooler_boost);
    assert!(!discovered.super_battery);
    assert!(!discovered.webcam);
    assert!(!discovered.webcam_block);
    assert!(!discovered.win_key);
    assert_eq!(discovered.fan_modes, Vec::new());
    assert_eq!(discovered.shift_modes, Vec::new());
    assert_eq!(discovered.keyboard_backlight, None);
    assert!(!discovered.battery_thresholds);
}

#[test]
fn missing_msi_ec_root_yields_empty_ec_capabilities() {
    let fixture = Fixture::new();
    let discovered = fixture.detector().discover().unwrap();
    assert_eq!(discovered, Capabilities::default());
}

#[test]
fn absent_fan_pair_yields_empty_fan_modes() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    assert_eq!(fixture.detector().discover().unwrap().fan_modes, Vec::new());
}

#[test]
fn fan_modes_preserve_upstream_order() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"auto\nsilent\nfuture_mode\n");
    assert_eq!(
        fixture.detector().discover().unwrap().fan_modes,
        modes(["auto", "silent", "future_mode"])
    );
}

#[test]
fn unknown_future_fan_mode_survives() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"turbo_liquid_v9\n");
    assert_eq!(
        fixture.detector().discover().unwrap().fan_modes,
        modes(["turbo_liquid_v9"])
    );
}

#[test]
fn fan_mode_without_available_list_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InconsistentInterface { capability, .. }) => {
            assert_eq!(capability, "fan_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn available_fan_modes_without_fan_mode_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("available_fan_modes", b"auto\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InconsistentInterface { capability, .. }) => {
            assert_eq!(capability, "fan_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn empty_fan_mode_list_is_rejected() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidModes { capability, .. }) => {
            assert_eq!(capability, "fan_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn internal_blank_fan_mode_is_rejected() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"auto\n\nsilent\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidModes { capability, .. }) => {
            assert_eq!(capability, "fan_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn duplicate_fan_modes_are_rejected() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"auto\nauto\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidModes { capability, .. }) => {
            assert_eq!(capability, "fan_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn invalid_fan_mode_name_is_rejected() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"auto\n silent\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidModes { capability, .. }) => {
            assert_eq!(capability, "fan_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn shift_modes_parse_successfully() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("shift_mode", b"comfort\n");
    fixture.ec_file("available_shift_modes", b"eco\ncomfort\nsport\nturbo\n");
    assert_eq!(
        fixture.detector().discover().unwrap().shift_modes,
        shift_modes(["eco", "comfort", "sport", "turbo"])
    );
}

#[test]
fn shift_pair_half_present_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("available_shift_modes", b"eco\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InconsistentInterface { capability, .. }) => {
            assert_eq!(capability, "shift_mode");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn unknown_future_shift_mode_survives() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    fixture.ec_file("shift_mode", b"comfort\n");
    fixture.ec_file("available_shift_modes", b"eco\nhybrid_ai_v2\n");
    let discovered = fixture.detector().discover().unwrap();
    assert_eq!(
        discovered
            .shift_modes
            .iter()
            .map(mec::hardware::ShiftMode::as_str)
            .collect::<Vec<_>>(),
        vec!["eco", "hybrid_ai_v2"]
    );
}

#[test]
fn missing_power_supply_root_disables_thresholds() {
    let fixture = Fixture::new();
    fixture.enable_full_simple_interface();
    fixture.enable_mode_pairs();
    assert!(!fixture.detector().discover().unwrap().battery_thresholds);
}

#[test]
fn power_supply_entry_without_thresholds_is_ignored() {
    let fixture = Fixture::new();
    fixture.class_dir("power_supply", "AC");
    assert!(!fixture.detector().discover().unwrap().battery_thresholds);
}

#[test]
fn real_directory_with_both_thresholds_is_supported() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT1");
    fixture.write_in(&battery, "charge_control_start_threshold", b"40\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    assert!(fixture.detector().discover().unwrap().battery_thresholds);
}

#[test]
fn symlinked_power_supply_entry_with_thresholds_is_supported() {
    let fixture = Fixture::new();
    let real = fixture.root.path().join("devices/BAT0");
    fs::create_dir_all(&real).unwrap();
    fixture.write_in(&real, "charge_control_start_threshold", b"40\n");
    fixture.write_in(&real, "charge_control_end_threshold", b"80\n");
    fixture.class_symlink("power_supply", "BAT0", &real);
    assert!(fixture.detector().discover().unwrap().battery_thresholds);
}

#[test]
fn start_threshold_only_is_inconsistent() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"40\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InconsistentInterface { capability, .. }) => {
            assert_eq!(capability, "battery_thresholds");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn end_threshold_only_is_inconsistent() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InconsistentInterface { capability, .. }) => {
            assert_eq!(capability, "battery_thresholds");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn missing_led_root_yields_no_backlight() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.detector().discover().unwrap().keyboard_backlight,
        None
    );
}

#[test]
fn exact_backlight_directory_is_detected() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    fixture.write_in(&backlight, "max_brightness", b"3\n");
    assert_eq!(
        fixture.detector().discover().unwrap().keyboard_backlight,
        Some(BacklightCapability { max_brightness: 3 })
    );
}

#[test]
fn exact_backlight_symlink_is_detected() {
    let fixture = Fixture::new();
    let real = fixture.root.path().join("devices/leds/kbd");
    fs::create_dir_all(&real).unwrap();
    fixture.write_in(&real, "brightness", b"2\n");
    fixture.write_in(&real, "max_brightness", b"3\n");
    fixture.class_symlink("leds", "msiacpi::kbd_backlight", &real);
    assert_eq!(
        fixture.detector().discover().unwrap().keyboard_backlight,
        Some(BacklightCapability { max_brightness: 3 })
    );
}

#[test]
fn unrelated_led_entries_are_ignored() {
    let fixture = Fixture::new();
    let unrelated = fixture.class_dir("leds", "platform::capslock");
    fixture.write_in(&unrelated, "brightness", b"1\n");
    fixture.write_in(&unrelated, "max_brightness", b"3\n");
    assert_eq!(
        fixture.detector().discover().unwrap().keyboard_backlight,
        None
    );
}

#[test]
fn backlight_max_brightness_is_captured() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"0\n");
    fixture.write_in(&backlight, "max_brightness", b"2\n");
    assert_eq!(
        fixture.detector().discover().unwrap().keyboard_backlight,
        Some(BacklightCapability { max_brightness: 2 })
    );
}

#[test]
fn zero_backlight_max_brightness_is_rejected() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"0\n");
    fixture.write_in(&backlight, "max_brightness", b"0\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidBacklight { .. }) => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn backlight_entry_missing_brightness_is_rejected() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "max_brightness", b"3\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidBacklight { .. }) => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn backlight_entry_missing_max_brightness_is_rejected() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::InvalidBacklight { .. }) => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn unparsable_backlight_max_is_typed_error() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    fixture.write_in(&backlight, "max_brightness", b"many\n");
    // The file exists but cannot be parsed as a number: a sysfs read failure,
    // distinct from a structurally invalid backlight entry.
    match fixture.detector().discover() {
        Err(CapabilityDiscoveryError::Read { capability, .. }) => {
            assert_eq!(capability, "keyboard_backlight");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn realtime_capability_is_presence_only() {
    let fixture = Fixture::new();
    fixture.ensure_ec_root();
    // Contents are intentionally non-numeric: presence must not read values.
    fixture.ec_file("cpu/realtime_temperature", b"not-a-number\n");
    fixture.ec_file("gpu/realtime_fan_speed", b"also-not-a-number\n");
    let discovered = fixture.detector().discover().unwrap();
    assert!(discovered.cpu_temperature);
    assert!(discovered.gpu_fan);
}

#[test]
fn discovery_does_not_depend_on_product_name_or_dmi() {
    let fixture = Fixture::new();
    fixture.enable_full_simple_interface();
    fixture.enable_mode_pairs();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"40\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    fixture.write_in(&backlight, "max_brightness", b"3\n");
    let without_dmi = fixture.detector().discover().unwrap();

    // Adding a DMI tree with an unrelated product name must change nothing:
    // discovery is driven purely by exposed interfaces.
    let dmi = fixture.root.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(
        dmi.join("sys_vendor"),
        b"Micro-Star International Co., Ltd.\n",
    )
    .unwrap();
    fs::write(dmi.join("product_name"), b"Totally Not A Model Name\n").unwrap();
    let with_dmi = fixture.detector().discover().unwrap();
    assert_eq!(with_dmi, without_dmi);
    assert_eq!(with_dmi, expected_full_gf_style());
}

#[test]
fn full_gf_style_expectation_matches_helper() {
    // Guards the expected-value helper itself against silent drift.
    let expected = expected_full_gf_style();
    assert_eq!(
        expected.fan_modes,
        modes(["auto", "silent", "basic", "advanced"])
    );
    assert_eq!(
        expected.shift_modes,
        shift_modes(["eco", "comfort", "sport", "turbo"])
    );
    assert_eq!(
        expected.keyboard_backlight,
        Some(BacklightCapability { max_brightness: 3 })
    );
}

// Re-exported helper assertions keep the fixture API exercised on both newtypes.
#[test]
fn fan_mode_newtype_accepts_each_upstream_mode() {
    for name in ["auto", "silent", "basic", "advanced", "future_mode"] {
        assert_eq!(
            mec::hardware::FanMode::try_from(name).unwrap().as_str(),
            name
        );
    }
}
