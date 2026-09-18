//! Production profile application through the public API only: TempDir
//! fake sysfs roots, no writer types named, no committed fixtures touched.

use std::fs;
use std::path::PathBuf;

use tempfile::{TempDir, tempdir};

use mec::hardware::SystemPaths;
use mec::safety::{ProfileApplyError, apply_profile};

struct FakeRoot {
    _dir: TempDir,
    root: PathBuf,
}

impl FakeRoot {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        Self {
            root: dir.path().to_path_buf(),
            _dir: dir,
        }
    }

    fn paths(&self) -> SystemPaths {
        SystemPaths::new(&self.root)
    }

    fn write_msi_identity(&self) {
        let dmi = self.root.join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).unwrap();
        fs::write(
            dmi.join("sys_vendor"),
            b"Micro-Star International Co., Ltd.\n",
        )
        .unwrap();
        fs::write(dmi.join("product_name"), b"Test MSI Laptop\n").unwrap();
    }

    fn ec_file(&self, relative: &str, contents: &[u8]) -> PathBuf {
        let path = self.root.join("sys/devices/platform/msi-ec").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        path
    }

    fn ready_base(&self) {
        self.write_msi_identity();
        fs::create_dir_all(self.root.join("sys/devices/platform/msi-ec")).unwrap();
    }
}

#[test]
fn public_apply_writes_fan_mode() {
    let root = FakeRoot::new();
    root.ready_base();
    let target = root.ec_file("fan_mode", b"silent\n");
    root.ec_file("available_fan_modes", b"silent\nauto\n");
    let profile: mec::profiles::Profile = "name = \"Air\"\n\n[performance]\nfan_mode = \"auto\"\n"
        .parse()
        .unwrap();
    let report = apply_profile(root.paths(), &profile).expect("apply must succeed");
    assert_eq!(report.applied().len(), 1);
    assert_eq!(fs::read(&target).unwrap(), b"auto\n");
}

#[test]
fn public_apply_noop_crosses_nothing() {
    let root = FakeRoot::new();
    root.ready_base();
    let target = root.ec_file("cooler_boost", b"on\n");
    let profile: mec::profiles::Profile = "name = \"Cool\"\n\n[performance]\ncooler_boost = true\n"
        .parse()
        .unwrap();
    let report = apply_profile(root.paths(), &profile).expect("noop must succeed");
    assert!(report.is_noop());
    assert_eq!(fs::read(&target).unwrap(), b"on\n");
}

#[test]
fn public_apply_battery_mismatch_restores_both_nodes() {
    let root = FakeRoot::new();
    root.ready_base();
    let entry = root.root.join("sys/class/power_supply/BAT0");
    fs::create_dir_all(&entry).unwrap();
    fs::write(entry.join("charge_control_start_threshold"), b"50\n").unwrap();
    fs::write(entry.join("charge_control_end_threshold"), b"60\n").unwrap();
    let profile: mec::profiles::Profile =
        "name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 80\n"
            .parse()
            .unwrap();
    let error =
        apply_profile(root.paths(), &profile).expect_err("mismatched readback must fail closed");
    assert!(matches!(error, ProfileApplyError::Execution(_)));
    assert_eq!(
        fs::read(entry.join("charge_control_start_threshold")).unwrap(),
        b"50\n"
    );
    assert_eq!(
        fs::read(entry.join("charge_control_end_threshold")).unwrap(),
        b"60\n"
    );
}

#[test]
fn public_apply_readonly_fails_without_mutation() {
    let dir = tempdir().unwrap();
    let dmi = dir.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("sys_vendor"), b"Dell Inc.\n").unwrap();
    fs::write(dmi.join("product_name"), b"XPS 15\n").unwrap();
    let profile: mec::profiles::Profile = "name = \"Cool\"\n\n[performance]\ncooler_boost = true\n"
        .parse()
        .unwrap();
    assert!(matches!(
        apply_profile(SystemPaths::new(dir.path()), &profile),
        Err(ProfileApplyError::PreviewRejected(_))
    ));
}

#[test]
fn public_apply_mixed_writes_only_changed() {
    let root = FakeRoot::new();
    root.ready_base();
    let cooler = root.ec_file("cooler_boost", b"off\n");
    let fan = root.ec_file("fan_mode", b"silent\n");
    root.ec_file("available_fan_modes", b"silent\n");
    let profile: mec::profiles::Profile = concat!(
        "name = \"Mixed\"\n",
        "\n",
        "[performance]\n",
        "fan_mode = \"silent\"\n",
        "cooler_boost = true\n",
    )
    .parse()
    .unwrap();
    let report = apply_profile(root.paths(), &profile).expect("apply must succeed");
    assert_eq!(report.applied().len(), 1);
    assert_eq!(report.unchanged().len(), 1);
    assert_eq!(fs::read(&cooler).unwrap(), b"on\n");
    assert_eq!(fs::read(&fan).unwrap(), b"silent\n");
}

#[test]
fn public_apply_repeated_call_becomes_noop() {
    let root = FakeRoot::new();
    root.ready_base();
    root.ec_file("cooler_boost", b"off\n");
    let profile: mec::profiles::Profile = "name = \"Cool\"\n\n[performance]\ncooler_boost = true\n"
        .parse()
        .unwrap();
    assert!(
        !apply_profile(root.paths(), &profile)
            .expect("first")
            .is_noop()
    );
    assert!(
        apply_profile(root.paths(), &profile)
            .expect("second")
            .is_noop()
    );
}
