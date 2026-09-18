//! Device identity detection against fake DMI/sysfs trees.

use std::fs;
use std::path::Path;

use tempfile::{TempDir, tempdir};

use mec::hardware::{DeviceDetector, DeviceInfo, SysfsError};

const VENDOR: &str = "sys_vendor";
const PRODUCT: &str = "product_name";
const BOARD: &str = "board_name";
const BIOS: &str = "bios_version";

struct Fixture {
    _root: TempDir,
    paths: mec::hardware::SystemPaths,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("sys/class/dmi/id")).unwrap();
        Self {
            paths: mec::hardware::SystemPaths::new(root.path()),
            _root: root,
        }
    }

    fn write_dmi(&self, name: &str, contents: &[u8]) {
        fs::write(
            self._root.path().join("sys/class/dmi/id").join(name),
            contents,
        )
        .unwrap();
    }

    fn write_vendor_product(&self, vendor: &str, product: &str) {
        self.write_dmi(VENDOR, vendor.as_bytes());
        self.write_dmi(PRODUCT, product.as_bytes());
    }

    /// Creates the msi-ec directory; fw_version itself stays absent unless written.
    fn enable_msi_ec(&self) {
        fs::create_dir_all(self._root.path().join("sys/devices/platform/msi-ec")).unwrap();
    }

    fn write_fw_version(&self, contents: &[u8]) {
        fs::write(
            self._root
                .path()
                .join("sys/devices/platform/msi-ec/fw_version"),
            contents,
        )
        .unwrap();
    }

    fn detector(&self) -> DeviceDetector<mec::hardware::LinuxSysfsReader> {
        DeviceDetector::new(self.paths.clone(), mec::hardware::LinuxSysfsReader)
    }

    fn dmi_file(&self, name: &str) -> impl AsRef<Path> + '_ {
        self._root.path().join("sys/class/dmi/id").join(name)
    }
}

fn msi_device(manufacturer: &str, product_name: &str) -> DeviceInfo {
    DeviceInfo {
        manufacturer: manufacturer.to_owned(),
        product_name: product_name.to_owned(),
        board_name: None,
        bios_version: None,
        ec_firmware_version: None,
    }
}

#[test]
fn detects_msi_gf63_identity() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("Micro-Star International Co., Ltd.\n", "GF63 Thin 11UC\n");
    let detected = fixture.detector().detect().unwrap();
    assert_eq!(
        detected,
        msi_device("Micro-Star International Co., Ltd.", "GF63 Thin 11UC")
    );
}

#[test]
fn detects_msi_non_gf_identity() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "Stealth 16\n");
    assert_eq!(
        fixture.detector().detect().unwrap().product_name,
        "Stealth 16"
    );
}

#[test]
fn accepts_exact_msi_vendor() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "Modern 15\n");
    assert!(fixture.detector().detect().is_ok());
}

#[test]
fn vendor_matching_is_case_insensitive() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MICRO-STAR INTERNATIONAL CO., LTD.\n", "Katana 15\n");
    assert!(fixture.detector().detect().is_ok());
}

#[test]
fn manufacturer_whitespace_is_normalized() {
    let fixture = Fixture::new();
    fixture.write_vendor_product(
        "  Micro-Star   International Co., Ltd.  \n",
        "GF63 Thin 11UC\n",
    );
    assert_eq!(
        fixture.detector().detect().unwrap().manufacturer,
        "Micro-Star International Co., Ltd."
    );
}

#[test]
fn product_name_whitespace_is_normalized() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63   Thin 11UC\n");
    assert_eq!(
        fixture.detector().detect().unwrap().product_name,
        "GF63 Thin 11UC"
    );
}

#[test]
fn non_msi_vendor_is_rejected_despite_msi_product() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("Dell Inc.\n", "MSI Test\n");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::UnsupportedVendor { detected }) => {
            assert_eq!(detected, "Dell Inc.")
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn missing_sys_vendor_preserves_field_identity() {
    let fixture = Fixture::new();
    fixture.write_dmi(PRODUCT, b"GF63 Thin 11UC\n");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::MissingRequiredField { field }) => {
            assert_eq!(field, VENDOR)
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn missing_product_name_fails() {
    let fixture = Fixture::new();
    fixture.write_dmi(VENDOR, b"MSI\n");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::MissingRequiredField { field }) => {
            assert_eq!(field, PRODUCT)
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn whitespace_only_manufacturer_fails() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("   \n", "GF63 Thin 11UC\n");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::InvalidField { field }) => {
            assert_eq!(field, VENDOR)
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn whitespace_only_product_name_fails() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", " \t\n");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::InvalidField { field }) => {
            assert_eq!(field, PRODUCT)
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn missing_board_name_is_none() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    assert_eq!(fixture.detector().detect().unwrap().board_name, None);
}

#[test]
fn missing_bios_version_is_none() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    assert_eq!(fixture.detector().detect().unwrap().bios_version, None);
}

#[test]
fn missing_fw_version_is_none_without_msi_ec() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    let detected = fixture.detector().detect().unwrap();
    assert_eq!(detected.ec_firmware_version, None);
}

#[test]
fn present_fw_version_is_returned() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    fixture.enable_msi_ec();
    fixture.write_fw_version(b"17.1.0\n");
    assert_eq!(
        fixture.detector().detect().unwrap().ec_firmware_version,
        Some("17.1.0".to_owned())
    );
}

#[test]
fn optional_invalid_utf8_is_error_not_none() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    fs::write(fixture.dmi_file(BOARD), [0xff, 0xfe]).unwrap();
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::ReadFailure { field, source }) => {
            assert_eq!(field, BOARD);
            assert!(matches!(source, SysfsError::InvalidUtf8(_)));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn optional_embedded_nul_is_error() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    fixture.enable_msi_ec();
    fixture.write_fw_version(b"17.\0");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::ReadFailure { field, source }) => {
            assert_eq!(field, "fw_version");
            assert!(matches!(source, SysfsError::EmbeddedNul(_)));
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn unknown_msi_product_family_succeeds() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "Raider GE78 HX Ultra 9\n");
    assert!(fixture.detector().detect().is_ok());
}

#[test]
fn vendor_containing_msi_substring_is_rejected() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("NotMSI Technologies\n", "Laptop X\n");
    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::UnsupportedVendor { detected }) => {
            assert_eq!(detected, "NotMSI Technologies")
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn detection_is_deterministic() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("Micro-Star International Co., Ltd.\n", "GF63 Thin 11UC\n");
    fixture.write_dmi(BIOS, b"E17GKIMS.110\n");
    fixture.enable_msi_ec();
    fixture.write_fw_version(b"17.1.0\n");
    let detector = fixture.detector();
    let first = detector.detect().unwrap();
    let second = detector.detect().unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first,
        DeviceInfo {
            manufacturer: "Micro-Star International Co., Ltd.".to_owned(),
            product_name: "GF63 Thin 11UC".to_owned(),
            board_name: None,
            bios_version: Some("E17GKIMS.110".to_owned()),
            ec_firmware_version: Some("17.1.0".to_owned()),
        }
    );
}

#[test]
fn identity_succeeds_without_ec_enrichment_when_ec_path_is_malformed() {
    let fixture = Fixture::new();
    fixture.write_vendor_product("MSI\n", "GF63 Thin 11UC\n");
    // Replace the driver directory with a regular file: reading
    // `msi-ec/fw_version` through it fails deterministically on any user.
    let ec_path = fixture._root.path().join("sys/devices/platform/msi-ec");
    fs::create_dir_all(ec_path.parent().unwrap()).unwrap();
    fs::write(&ec_path, b"not a directory\n").unwrap();

    let identity = fixture.detector().detect_identity().unwrap();
    assert_eq!(identity, msi_device("MSI", "GF63 Thin 11UC"));

    match fixture.detector().detect() {
        Err(mec::hardware::DetectionError::ReadFailure { field, .. }) => {
            assert_eq!(field, "fw_version");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}
