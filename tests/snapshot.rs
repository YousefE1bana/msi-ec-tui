//! Read-only backend snapshots against fake sysfs trees.

use std::fs;
use std::os::unix::fs::symlink;

use tempfile::{TempDir, tempdir};

use mec::hardware::{
    BackendError, Capabilities, CapabilityDetector, EcBackend, FanMode, HardwareSnapshot,
    LinuxSysfsReader, MsiEcBackend, ShiftMode, SystemPaths,
};

struct Fixture {
    _root: TempDir,
    paths: SystemPaths,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        Self {
            paths: SystemPaths::new(root.path()),
            _root: root,
        }
    }

    fn backend(&self) -> MsiEcBackend<LinuxSysfsReader> {
        MsiEcBackend::new(self.paths.clone(), LinuxSysfsReader)
    }

    fn write_dmi(&self, name: &str, contents: &[u8]) {
        let dir = self._root.path().join("sys/class/dmi/id");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), contents).unwrap();
    }

    fn write_identity(&self) {
        self.write_dmi("sys_vendor", b"Micro-Star International Co., Ltd.\n");
        self.write_dmi("product_name", b"GF63 Thin 11UC\n");
    }

    fn ec_file(&self, relative: &str, contents: &[u8]) {
        let path = self
            ._root
            .path()
            .join("sys/devices/platform/msi-ec")
            .join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn class_dir(&self, class: &str, name: &str) -> std::path::PathBuf {
        let path = self._root.path().join("sys/class").join(class).join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn class_symlink(&self, class: &str, name: &str, target: &std::path::Path) {
        let class_dir = self._root.path().join("sys/class").join(class);
        fs::create_dir_all(&class_dir).unwrap();
        symlink(target, class_dir.join(name)).unwrap();
    }

    fn write_in(&self, entry: &std::path::Path, name: &str, contents: &[u8]) {
        fs::write(entry.join(name), contents).unwrap();
    }

    /// Representative full interface matching the recommended fixture values.
    fn enable_full(&self) {
        self.write_identity();
        self.ec_file("fw_version", b"17.1.0\n");
        self.ec_file("cpu/realtime_temperature", b"63\n");
        self.ec_file("gpu/realtime_temperature", b"51\n");
        self.ec_file("cpu/realtime_fan_speed", b"42\n");
        self.ec_file("gpu/realtime_fan_speed", b"31\n");
        self.ec_file("fan_mode", b"auto\n");
        self.ec_file("available_fan_modes", b"auto\nsilent\nbasic\nadvanced\n");
        self.ec_file("shift_mode", b"comfort\n");
        self.ec_file("available_shift_modes", b"eco\ncomfort\nsport\nturbo\n");
        self.ec_file("cooler_boost", b"off\n");
        self.ec_file("super_battery", b"off\n");
        self.ec_file("webcam", b"on\n");
        self.ec_file("webcam_block", b"off\n");
        let battery = self.class_dir("power_supply", "BAT0");
        self.write_in(&battery, "charge_control_start_threshold", b"70\n");
        self.write_in(&battery, "charge_control_end_threshold", b"80\n");
        let backlight = self.class_dir("leds", "msiacpi::kbd_backlight");
        self.write_in(&backlight, "brightness", b"2\n");
        self.write_in(&backlight, "max_brightness", b"3\n");
    }

    /// Minimal EC root with only the given interface file present.
    fn enable_only(&self, relative: &str, contents: &[u8]) {
        self.ec_file(relative, contents);
    }

    fn bool_fixture(field: &str, contents: &[u8]) -> Self {
        let fixture = Self::new();
        fixture.enable_only(field, contents);
        fixture
    }

    fn bool_field(snapshot: &HardwareSnapshot, field: &str) -> Option<bool> {
        match field {
            "cooler_boost" => snapshot.cooler_boost,
            "super_battery" => snapshot.super_battery,
            "webcam" => snapshot.webcam,
            "webcam_block" => snapshot.webcam_block,
            _ => panic!("unknown boolean field: {field}"),
        }
    }
}

fn fan_mode(name: &str) -> FanMode {
    FanMode::try_from(name).unwrap()
}

fn shift_mode(name: &str) -> ShiftMode {
    ShiftMode::try_from(name).unwrap()
}

#[test]
fn full_snapshot_matches_representative_values() {
    let fixture = Fixture::new();
    fixture.enable_full();
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.cpu_temperature.unwrap().get(), 63);
    assert_eq!(snapshot.gpu_temperature.unwrap().get(), 51);
    assert_eq!(snapshot.cpu_fan.unwrap().get(), 42);
    assert_eq!(snapshot.gpu_fan.unwrap().get(), 31);
    assert_eq!(snapshot.fan_mode, Some(fan_mode("auto")));
    assert_eq!(snapshot.shift_mode, Some(shift_mode("comfort")));
    assert_eq!(snapshot.cooler_boost, Some(false));
    assert_eq!(snapshot.super_battery, Some(false));
    assert_eq!(snapshot.webcam, Some(true));
    assert_eq!(snapshot.webcam_block, Some(false));
    assert_eq!(snapshot.keyboard_backlight, Some(2));
    assert_eq!(snapshot.battery_start_threshold, Some(70));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}

#[test]
fn partial_capabilities_produce_none_fields() {
    let fixture = Fixture::new();
    fixture.enable_only("cpu/realtime_temperature", b"63\n");
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.cpu_temperature.unwrap().get(), 63);
    assert_eq!(snapshot.gpu_temperature, None);
    assert_eq!(snapshot.cpu_fan, None);
    assert_eq!(snapshot.gpu_fan, None);
    assert_eq!(snapshot.fan_mode, None);
    assert_eq!(snapshot.shift_mode, None);
    assert_eq!(snapshot.cooler_boost, None);
    assert_eq!(snapshot.super_battery, None);
    assert_eq!(snapshot.webcam, None);
    assert_eq!(snapshot.webcam_block, None);
    assert_eq!(snapshot.keyboard_backlight, None);
    assert_eq!(snapshot.battery_start_threshold, None);
    assert_eq!(snapshot.battery_end_threshold, None);
}

#[test]
fn cpu_temperature_boundaries_are_valid() {
    for value in [b"0\n".as_slice(), b"100\n".as_slice()] {
        let fixture = Fixture::new();
        fixture.enable_only("cpu/realtime_temperature", value);
        assert!(fixture.backend().snapshot().is_ok());
    }
}

#[test]
fn cpu_temperature_above_range_is_rejected() {
    let fixture = Fixture::new();
    fixture.enable_only("cpu/realtime_temperature", b"101\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn gpu_temperature_malformed_integer_is_rejected() {
    let fixture = Fixture::new();
    fixture.enable_only("gpu/realtime_temperature", b"warm\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn fan_value_boundaries_are_valid() {
    for value in [b"0\n".as_slice(), b"150\n".as_slice()] {
        let fixture = Fixture::new();
        fixture.enable_only("cpu/realtime_fan_speed", value);
        assert!(fixture.backend().snapshot().is_ok());
    }
}

#[test]
fn fan_value_above_range_is_rejected() {
    let fixture = Fixture::new();
    fixture.enable_only("cpu/realtime_fan_speed", b"151\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn fan_value_malformed_integer_is_rejected() {
    let fixture = Fixture::new();
    fixture.enable_only("gpu/realtime_fan_speed", b"fast\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn fan_value_uses_percentage_style_semantics() {
    let fixture = Fixture::new();
    fixture.enable_only("cpu/realtime_fan_speed", b"42\n");
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(
        snapshot.cpu_fan,
        Some(mec::hardware::FanPercent::try_from(42).unwrap())
    );
    assert_eq!(snapshot.cpu_fan.unwrap().get(), 42);
}

#[test]
fn current_fan_mode_is_parsed_through_newtype() {
    let fixture = Fixture::new();
    fixture.ec_file("fan_mode", b"silent\n");
    fixture.ec_file("available_fan_modes", b"auto\nsilent\n");
    assert_eq!(
        fixture.backend().snapshot().unwrap().fan_mode,
        Some(fan_mode("silent"))
    );
}

#[test]
fn unknown_valid_current_fan_mode_survives() {
    let fixture = Fixture::new();
    fixture.ec_file("fan_mode", b"hydro_v9\n");
    fixture.ec_file("available_fan_modes", b"auto\nsilent\n");
    // Syntactically valid but absent from the advertised list: still reported.
    assert_eq!(
        fixture.backend().snapshot().unwrap().fan_mode,
        Some(fan_mode("hydro_v9"))
    );
}

#[test]
fn invalid_current_fan_mode_is_rejected() {
    let fixture = Fixture::new();
    fixture.ec_file("fan_mode", b" bad mode \n");
    fixture.ec_file("available_fan_modes", b"auto\nsilent\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn current_shift_mode_is_parsed() {
    let fixture = Fixture::new();
    fixture.ec_file("shift_mode", b"sport\n");
    fixture.ec_file("available_shift_modes", b"eco\ncomfort\nsport\nturbo\n");
    assert_eq!(
        fixture.backend().snapshot().unwrap().shift_mode,
        Some(shift_mode("sport"))
    );
}

#[test]
fn unspecified_shift_mode_is_accepted_outside_available_list() {
    let fixture = Fixture::new();
    fixture.ec_file("shift_mode", b"unspecified\n");
    fixture.ec_file("available_shift_modes", b"eco\ncomfort\n");
    assert_eq!(
        fixture.backend().snapshot().unwrap().shift_mode,
        Some(shift_mode("unspecified"))
    );
}

#[test]
fn invalid_current_shift_mode_is_rejected() {
    let fixture = Fixture::new();
    fixture.ec_file("shift_mode", b"\n");
    fixture.ec_file("available_shift_modes", b"eco\ncomfort\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn boolean_on_maps_to_true() {
    for field in ["cooler_boost", "super_battery", "webcam", "webcam_block"] {
        let fixture = Fixture::bool_fixture(field, b"on\n");
        assert_eq!(
            Fixture::bool_field(&fixture.backend().snapshot().unwrap(), field),
            Some(true),
            "{field}"
        );
    }
}

#[test]
fn boolean_off_maps_to_false() {
    for field in ["cooler_boost", "super_battery", "webcam", "webcam_block"] {
        let fixture = Fixture::bool_fixture(field, b"off\n");
        assert_eq!(
            Fixture::bool_field(&fixture.backend().snapshot().unwrap(), field),
            Some(false),
            "{field}"
        );
    }
}

#[test]
fn boolean_digit_one_is_rejected() {
    for field in ["cooler_boost", "super_battery", "webcam", "webcam_block"] {
        let fixture = Fixture::bool_fixture(field, b"1\n");
        assert!(
            matches!(
                fixture.backend().snapshot(),
                Err(BackendError::InvalidData(_))
            ),
            "{field}"
        );
    }
}

#[test]
fn boolean_true_is_rejected() {
    let fixture = Fixture::bool_fixture("cooler_boost", b"true\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn boolean_uppercase_is_rejected() {
    let fixture = Fixture::bool_fixture("cooler_boost", b"ON\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn boolean_empty_is_rejected() {
    let fixture = Fixture::bool_fixture("webcam", b"");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn boolean_rejections_apply_to_every_supported_field() {
    for value in [
        b"0".as_slice(),
        b"false",
        b"OFF",
        b"yes",
        b"enabled",
        b" on",
    ] {
        for field in ["cooler_boost", "super_battery", "webcam", "webcam_block"] {
            let fixture = Fixture::bool_fixture(field, value);
            assert!(
                matches!(
                    fixture.backend().snapshot(),
                    Err(BackendError::InvalidData(_))
                ),
                "{field} with {value:?}"
            );
        }
    }
}

#[test]
fn backlight_brightness_within_max_is_accepted() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"2\n");
    fixture.write_in(&backlight, "max_brightness", b"3\n");
    assert_eq!(
        fixture.backend().snapshot().unwrap().keyboard_backlight,
        Some(2)
    );
}

#[test]
fn backlight_brightness_equal_to_max_is_accepted() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"3\n");
    fixture.write_in(&backlight, "max_brightness", b"3\n");
    assert_eq!(
        fixture.backend().snapshot().unwrap().keyboard_backlight,
        Some(3)
    );
}

#[test]
fn backlight_brightness_above_max_is_rejected() {
    let fixture = Fixture::new();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"4\n");
    fixture.write_in(&backlight, "max_brightness", b"3\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn symlinked_backlight_entry_is_followed_for_values() {
    let fixture = Fixture::new();
    let real = fixture._root.path().join("devices/leds/kbd");
    fs::create_dir_all(&real).unwrap();
    fixture.write_in(&real, "brightness", b"1\n");
    fixture.write_in(&real, "max_brightness", b"3\n");
    fixture.class_symlink("leds", "msiacpi::kbd_backlight", &real);
    assert_eq!(
        fixture.backend().snapshot().unwrap().keyboard_backlight,
        Some(1)
    );
}

#[test]
fn battery_threshold_real_directory_works() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"70\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_start_threshold, Some(70));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}

#[test]
fn battery_threshold_symlink_entry_works() {
    let fixture = Fixture::new();
    let real = fixture._root.path().join("devices/BAT0");
    fs::create_dir_all(&real).unwrap();
    fixture.write_in(&real, "charge_control_start_threshold", b"70\n");
    fixture.write_in(&real, "charge_control_end_threshold", b"80\n");
    fixture.class_symlink("power_supply", "BAT0", &real);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_start_threshold, Some(70));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}

#[test]
fn battery_threshold_equal_pair_is_rejected() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"80\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_threshold_wide_gap_is_rejected() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"50\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_threshold_above_hundred_is_rejected() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"50\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"101\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_inverted_thresholds_are_rejected() {
    let fixture = Fixture::new();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"80\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"50\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn unrelated_ac_entry_does_not_block_battery_thresholds() {
    let fixture = Fixture::new();
    fixture.class_dir("power_supply", "AC");
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"70\n");
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_start_threshold, Some(70));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}

#[test]
fn first_sorted_complete_threshold_entry_wins() {
    let fixture = Fixture::new();
    let first = fixture.class_dir("power_supply", "BAT1");
    fixture.write_in(&first, "charge_control_start_threshold", b"10\n");
    fixture.write_in(&first, "charge_control_end_threshold", b"20\n");
    let second = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&second, "charge_control_start_threshold", b"70\n");
    fixture.write_in(&second, "charge_control_end_threshold", b"80\n");
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_start_threshold, Some(70));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}

#[test]
fn vanishing_threshold_pair_is_an_error_not_none() {
    let fixture = Fixture::new();
    // A directory where the start-threshold file belongs: presence probing
    // sees it, but reading it as a value fails deterministically.
    let battery = fixture.class_dir("power_supply", "BAT0");
    fs::create_dir(battery.join("charge_control_start_threshold")).unwrap();
    fixture.write_in(&battery, "charge_control_end_threshold", b"80\n");
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn detect_device_returns_dmi_plus_firmware() {
    let fixture = Fixture::new();
    fixture.write_identity();
    fixture.write_dmi("board_name", b"Test Board\n");
    fixture.write_dmi("bios_version", b"E17GKIMS.110\n");
    fixture.ec_file("fw_version", b"17.1.0\n");
    let device = fixture.backend().detect_device().unwrap();
    assert_eq!(device.manufacturer, "Micro-Star International Co., Ltd.");
    assert_eq!(device.product_name, "GF63 Thin 11UC");
    assert_eq!(device.board_name, Some("Test Board".to_owned()));
    assert_eq!(device.bios_version, Some("E17GKIMS.110".to_owned()));
    assert_eq!(device.ec_firmware_version, Some("17.1.0".to_owned()));
}

#[test]
fn backend_capabilities_match_dynamic_discovery() {
    let fixture = Fixture::new();
    fixture.enable_full();
    let expected: Capabilities = CapabilityDetector::new(fixture.paths.clone(), LinuxSysfsReader)
        .discover()
        .unwrap();
    assert_eq!(fixture.backend().capabilities().unwrap(), expected);
}

#[test]
fn partial_snapshot_never_requires_unsupported_interfaces() {
    // No DMI, no EC root content beyond one file: snapshot must still work.
    let fixture = Fixture::new();
    fixture.enable_only("webcam", b"on\n");
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.webcam, Some(true));
    assert_eq!(snapshot.cpu_temperature, None);
}

#[test]
fn empty_root_yields_empty_snapshot_without_hardware() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.backend().capabilities().unwrap(),
        Capabilities::default()
    );
    assert_eq!(
        fixture.backend().snapshot().unwrap(),
        HardwareSnapshot::default()
    );
}

impl Fixture {
    fn power_entry(&self, name: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let entry = self.class_dir("power_supply", name);
        for (name, contents) in files {
            self.write_in(&entry, name, contents);
        }
        entry
    }

    fn battery_entry(&self, name: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let mut with_type = vec![("type", "Battery".as_bytes())];
        with_type.extend_from_slice(files);
        self.power_entry(name, &with_type)
    }
}

use mec::hardware::BatteryStatus;

#[test]
fn battery_percentage_zero_accepted() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"0\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().battery_percentage,
        Some(0)
    );
}

#[test]
fn battery_percentage_hundred_accepted() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"100\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().battery_percentage,
        Some(100)
    );
}

#[test]
fn battery_percentage_above_hundred_rejected() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"101\n")]);
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_malformed_capacity_rejected() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"high\n")]);
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_status_values_parse() {
    for (text, expected) in [
        ("Unknown\n", BatteryStatus::Unknown),
        ("Charging\n", BatteryStatus::Charging),
        ("Discharging\n", BatteryStatus::Discharging),
        ("Not charging\n", BatteryStatus::NotCharging),
        ("Full\n", BatteryStatus::Full),
    ] {
        let fixture = Fixture::new();
        fixture.battery_entry("BAT0", &[("status", text.as_bytes())]);
        assert_eq!(
            fixture.backend().snapshot().unwrap().battery_status,
            Some(expected),
            "{text:?}"
        );
    }
}

#[test]
fn battery_invalid_status_rejected() {
    for text in ["charging\n", "CHARGING\n", "Fast\n", "\n"] {
        let fixture = Fixture::new();
        fixture.battery_entry("BAT0", &[("status", text.as_bytes())]);
        assert!(
            matches!(
                fixture.backend().snapshot(),
                Err(BackendError::InvalidData(_))
            ),
            "{text:?}"
        );
    }
}

#[test]
fn battery_missing_capacity_is_none() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("status", b"Charging\n")]);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, None);
    assert_eq!(snapshot.battery_status, Some(BatteryStatus::Charging));
}

#[test]
fn battery_missing_status_is_none() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, Some(77));
    assert_eq!(snapshot.battery_status, None);
}

#[test]
fn battery_absent_without_battery_entry() {
    let fixture = Fixture::new();
    fixture.power_entry("AC", &[("type", b"Mains\n"), ("online", b"1\n")]);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, None);
    assert_eq!(snapshot.battery_status, None);
    assert_eq!(snapshot.ac_connected, Some(true));
}

#[test]
fn battery_discovered_by_type_not_name() {
    let fixture = Fixture::new();
    fixture.battery_entry("CELL0", &[("capacity", b"64\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().battery_percentage,
        Some(64)
    );
}

#[test]
fn battery_first_sorted_entry_wins() {
    let fixture = Fixture::new();
    fixture.battery_entry("ZZZ", &[("capacity", b"10\n")]);
    fixture.battery_entry("AAA", &[("capacity", b"90\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().battery_percentage,
        Some(90)
    );
}

#[test]
fn battery_non_battery_type_ignored() {
    let fixture = Fixture::new();
    fixture.power_entry("BAT0", &[("type", b"battery\n"), ("capacity", b"64\n")]);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, None);
    assert_eq!(snapshot.battery_status, None);
}

#[test]
fn battery_online_values_aggregate() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    fixture.power_entry("AC", &[("type", b"Mains\n"), ("online", b"1\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().ac_connected,
        Some(true)
    );

    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    fixture.power_entry("AC", &[("type", b"Mains\n"), ("online", b"0\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().ac_connected,
        Some(false)
    );
}

#[test]
fn battery_external_supplies_use_or_semantics() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    fixture.power_entry("AC", &[("type", b"Mains\n"), ("online", b"0\n")]);
    fixture.power_entry("ADP1", &[("type", b"Mains\n"), ("online", b"1\n")]);
    assert_eq!(
        fixture.backend().snapshot().unwrap().ac_connected,
        Some(true)
    );
}

#[test]
fn battery_no_external_online_is_none() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    assert_eq!(fixture.backend().snapshot().unwrap().ac_connected, None);
}

#[test]
fn battery_online_value_two_rejected() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    fixture.power_entry("AC", &[("type", b"Mains\n"), ("online", b"2\n")]);
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_malformed_online_rejected() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    fixture.power_entry("AC", &[("type", b"Mains\n"), ("online", b"yes\n")]);
    assert!(matches!(
        fixture.backend().snapshot(),
        Err(BackendError::InvalidData(_))
    ));
}

#[test]
fn battery_missing_power_supply_root_is_all_none() {
    let fixture = Fixture::new();
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, None);
    assert_eq!(snapshot.battery_status, None);
    assert_eq!(snapshot.ac_connected, None);
    assert_eq!(snapshot.battery_start_threshold, None);
    assert_eq!(snapshot.battery_end_threshold, None);
}

#[test]
fn battery_symlinked_battery_entry_works() {
    let fixture = Fixture::new();
    let real = fixture._root.path().join("devices/POWER");
    fs::create_dir_all(&real).unwrap();
    fixture.write_in(&real, "type", b"Battery\n");
    fixture.write_in(&real, "capacity", b"55\n");
    fixture.write_in(&real, "status", b"Discharging\n");
    fixture.class_symlink("power_supply", "BAT0", &real);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, Some(55));
    assert_eq!(snapshot.battery_status, Some(BatteryStatus::Discharging));
}

#[test]
fn battery_symlinked_external_supply_works() {
    let fixture = Fixture::new();
    fixture.battery_entry("BAT0", &[("capacity", b"77\n")]);
    let real = fixture._root.path().join("devices/ACAD");
    fs::create_dir_all(&real).unwrap();
    fixture.write_in(&real, "type", b"Mains\n");
    fixture.write_in(&real, "online", b"1\n");
    fixture.class_symlink("power_supply", "AC", &real);
    assert_eq!(
        fixture.backend().snapshot().unwrap().ac_connected,
        Some(true)
    );
}

#[test]
fn battery_runtime_independent_of_threshold_entry() {
    let fixture = Fixture::new();
    let thresholds = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&thresholds, "charge_control_start_threshold", b"70\n");
    fixture.write_in(&thresholds, "charge_control_end_threshold", b"80\n");
    fixture.battery_entry("CELL0", &[("capacity", b"64\n")]);
    let snapshot = fixture.backend().snapshot().unwrap();
    assert_eq!(snapshot.battery_percentage, Some(64));
    assert_eq!(snapshot.battery_start_threshold, Some(70));
    assert_eq!(snapshot.battery_end_threshold, Some(80));
}
