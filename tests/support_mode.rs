//! Compatibility / read-only policy against fake DMI/sysfs trees.

use std::fs;

use tempfile::{TempDir, tempdir};

use mec::hardware::{LinuxSysfsReader, ReadOnlyReason, SupportEvaluator, SupportMode, SystemPaths};

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

    fn evaluate(&self) -> SupportMode {
        SupportEvaluator::new(self.paths.clone(), LinuxSysfsReader).evaluate()
    }

    fn write_dmi(&self, name: &str, contents: &[u8]) {
        let dir = self.root.path().join("sys/class/dmi/id");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), contents).unwrap();
    }

    fn write_msi_identity(&self, product: &str) {
        self.write_dmi("sys_vendor", b"Micro-Star International Co., Ltd.\n");
        self.write_dmi("product_name", product.as_bytes());
    }

    fn ec_dir(&self) -> std::path::PathBuf {
        self.root.path().join("sys/devices/platform/msi-ec")
    }

    fn ensure_ec_root(&self) {
        fs::create_dir_all(self.ec_dir()).unwrap();
    }

    fn ec_file(&self, relative: &str, contents: &[u8]) {
        let path = self.ec_dir().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// A coherent GF-style interface: full mode pairs plus one representative
    /// presence capability, without depending on any product-family name.
    fn enable_coherent_interface(&self) {
        self.ensure_ec_root();
        self.ec_file("fan_mode", b"auto\n");
        self.ec_file("available_fan_modes", b"auto\nsilent\n");
        self.ec_file("shift_mode", b"comfort\n");
        self.ec_file("available_shift_modes", b"eco\ncomfort\n");
        self.ec_file("cooler_boost", b"0\n");
    }

    fn class_dir(&self, class: &str, name: &str) -> std::path::PathBuf {
        let path = self.root.path().join("sys/class").join(class).join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_in(&self, entry: &std::path::Path, name: &str, contents: &[u8]) {
        fs::write(entry.join(name), contents).unwrap();
    }
}

#[test]
fn non_msi_vendor_is_read_only() {
    let fixture = Fixture::new();
    fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
    fixture.write_dmi("product_name", b"XPS 15\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware)
    );
}

#[test]
fn msi_product_name_under_non_msi_vendor_is_still_non_msi() {
    let fixture = Fixture::new();
    fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
    fixture.write_dmi("product_name", b"MSI GF63 Thin\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware)
    );
}

#[test]
fn missing_sys_vendor_is_unverified() {
    let fixture = Fixture::new();
    fixture.write_dmi("product_name", b"GF63 Thin 11UC\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::UnverifiedHardwareIdentity)
    );
}

#[test]
fn missing_product_name_is_unverified() {
    let fixture = Fixture::new();
    fixture.write_dmi("sys_vendor", b"MSI\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::UnverifiedHardwareIdentity)
    );
}

#[test]
fn empty_required_dmi_is_unverified() {
    let fixture = Fixture::new();
    fixture.write_dmi("sys_vendor", b"MSI\n");
    fixture.write_dmi("product_name", b"   \n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::UnverifiedHardwareIdentity)
    );
}

#[test]
fn missing_msi_ec_root_is_unavailable() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable)
    );
}

#[test]
fn non_enumerable_msi_ec_root_is_unreadable() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    // A regular file where the driver directory belongs: list_entries()
    // fails deterministically with a typed I/O error on any platform/user.
    let path = fixture.ec_dir();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"not a directory\n").unwrap();
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnreadable)
    );
}

#[test]
fn gf_style_model_with_coherent_interface_is_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.enable_coherent_interface();
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn unknown_msi_product_with_coherent_interface_is_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("Never Released 9999 ZZZ\n");
    fixture.enable_coherent_interface();
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn readable_empty_msi_ec_root_is_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn missing_webcam_is_still_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.enable_coherent_interface();
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn missing_fan_mode_pair_is_still_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    fixture.ec_file("shift_mode", b"comfort\n");
    fixture.ec_file("available_shift_modes", b"eco\ncomfort\n");
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn fan_mode_only_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn available_fan_modes_only_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    fixture.ec_file("available_fan_modes", b"auto\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn malformed_fan_mode_list_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    fixture.ec_file("fan_mode", b"auto\n");
    fixture.ec_file("available_fan_modes", b"auto\nauto\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn half_present_shift_pair_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    fixture.ec_file("available_shift_modes", b"eco\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn half_present_battery_pair_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    let battery = fixture.class_dir("power_supply", "BAT0");
    fixture.write_in(&battery, "charge_control_start_threshold", b"40\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn invalid_backlight_entry_is_inconsistent() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    fixture.write_in(&backlight, "max_brightness", b"0\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
    );
}

#[test]
fn capability_read_failure_is_unreadable() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.ensure_ec_root();
    // Both backlight files exist, but max_brightness cannot be parsed:
    // discovery fails with a sysfs read error, not a structural one.
    let backlight = fixture.class_dir("leds", "msiacpi::kbd_backlight");
    fixture.write_in(&backlight, "brightness", b"1\n");
    fixture.write_in(&backlight, "max_brightness", b"many\n");
    assert_eq!(
        fixture.evaluate(),
        SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnreadable)
    );
}

#[test]
fn missing_power_supply_root_still_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.enable_coherent_interface();
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn missing_leds_root_still_ready() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("Stealth 16\n");
    fixture.enable_coherent_interface();
    assert_eq!(fixture.evaluate(), SupportMode::Ready);
}

#[test]
fn evaluation_ignores_product_family_names() {
    for product in [
        "GF63 Thin 11UC\n",
        "Katana 15\n",
        "Stealth 16\n",
        "Modern 15\n",
        "Raider GE78\n",
        "Unheard Of Model 1234\n",
    ] {
        let fixture = Fixture::new();
        fixture.write_msi_identity(product);
        fixture.enable_coherent_interface();
        assert_eq!(fixture.evaluate(), SupportMode::Ready, "{product:?}");
    }
}

#[test]
fn repeated_evaluation_is_stable() {
    let fixture = Fixture::new();
    fixture.write_msi_identity("GF63 Thin 11UC\n");
    fixture.enable_coherent_interface();
    let evaluator = SupportEvaluator::new(fixture.paths.clone(), LinuxSysfsReader);
    assert_eq!(evaluator.evaluate(), SupportMode::Ready);
    assert_eq!(evaluator.evaluate(), SupportMode::Ready);
}
