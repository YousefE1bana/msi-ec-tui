//! Transaction execution through the public API only: isolated TempDir
//! fake sysfs roots plus an injected test boundary. Never touches real
//! hardware or the committed fixtures.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use tempfile::{TempDir, tempdir};

use mec::hardware::{HardwareCommand, HardwareWriteBoundary, LinuxSysfsReader, WriteBoundaryError};
use mec::profiles::Profile;
use mec::safety::{ProfileApplyError, ProfileTransactionExecutor};

/// Test-only recording transport with scripted results.
#[derive(Clone)]
struct RecordingBoundary {
    recorded: Rc<RefCell<Vec<HardwareCommand>>>,
    scripted: Rc<RefCell<VecDeque<Result<(), WriteBoundaryError>>>>,
}

impl RecordingBoundary {
    fn succeeding() -> Self {
        Self {
            recorded: Rc::new(RefCell::new(Vec::new())),
            scripted: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    fn scripted(results: Vec<Result<(), WriteBoundaryError>>) -> Self {
        Self {
            recorded: Rc::new(RefCell::new(Vec::new())),
            scripted: Rc::new(RefCell::new(results.into())),
        }
    }

    fn calls(&self) -> usize {
        self.recorded.borrow().len()
    }
}

impl HardwareWriteBoundary for RecordingBoundary {
    fn execute(&self, command: &HardwareCommand) -> Result<(), WriteBoundaryError> {
        self.recorded.borrow_mut().push(command.clone());
        self.scripted.borrow_mut().pop_front().unwrap_or(Ok(()))
    }
}

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

    fn ec_file(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join("sys/devices/platform/msi-ec").join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// MSI identity plus a two-setting interface whose current values
    /// differ from the test profile below.
    fn ready_trio(&self) {
        let dmi = self.root.join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).unwrap();
        fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
        fs::write(dmi.join("product_name"), b"Test Laptop 15\n").unwrap();
        self.ec_file("shift_mode", b"comfort\n");
        self.ec_file("available_shift_modes", b"comfort\nturbo\n");
        self.ec_file("fan_mode", b"auto\n");
        self.ec_file("available_fan_modes", b"auto\nsilent\n");
        self.ec_file("cooler_boost", b"off\n");
    }

    fn applier(
        &self,
        boundary: RecordingBoundary,
    ) -> ProfileTransactionExecutor<LinuxSysfsReader, RecordingBoundary> {
        ProfileTransactionExecutor::new(
            mec::hardware::SystemPaths::new(&self.root),
            LinuxSysfsReader,
            boundary,
        )
    }
}

const TRIO_TOML: &str = concat!(
    "name = \"Trio\"\n",
    "\n",
    "[performance]\n",
    "shift_mode = \"turbo\"\n",
    "fan_mode = \"silent\"\n",
    "cooler_boost = true\n",
);

#[test]
fn successful_apply_runs_in_forward_order() {
    let root = FakeRoot::new();
    root.ready_trio();
    let boundary = RecordingBoundary::succeeding();
    let transaction = root.applier(boundary.clone());
    let profile: Profile = TRIO_TOML.parse().unwrap();
    let report = transaction.apply(&profile).expect("apply must succeed");
    assert_eq!(report.name().as_str(), "Trio");
    assert_eq!(report.applied().len(), 3);
    assert!(report.unchanged().is_empty());
    assert!(!report.is_noop());
    assert_eq!(boundary.calls(), 3);
}

#[test]
fn noop_apply_crosses_no_boundary() {
    let root = FakeRoot::new();
    root.ready_trio();
    let boundary = RecordingBoundary::succeeding();
    let transaction = root.applier(boundary.clone());
    let profile: Profile = concat!(
        "name = \"Steady\"\n",
        "\n",
        "[performance]\n",
        "shift_mode = \"comfort\"\n",
        "fan_mode = \"auto\"\n",
        "cooler_boost = false\n",
    )
    .parse()
    .unwrap();
    let report = transaction.apply(&profile).expect("noop must succeed");
    assert!(report.is_noop());
    assert!(report.applied().is_empty());
    assert_eq!(report.unchanged().len(), 3);
    assert_eq!(boundary.calls(), 0);
}

#[test]
fn boundary_failure_rolls_back_in_reverse() {
    let root = FakeRoot::new();
    root.ready_trio();
    let boundary = RecordingBoundary::scripted(vec![
        Ok(()),
        Err(WriteBoundaryError::VerificationFailed("fan mode")),
        Ok(()),
        Ok(()),
    ]);
    let transaction = root.applier(boundary.clone());
    let profile: Profile = TRIO_TOML.parse().unwrap();
    let error = transaction
        .apply(&profile)
        .expect_err("boundary failure must fail the apply");
    let ProfileApplyError::Execution(failure) = error else {
        panic!("expected execution failure, got {error:?}");
    };
    assert_eq!(failure.applied_before_failure().len(), 1);
    assert_eq!(failure.rollback_attempts().len(), 2);
    assert!(
        failure
            .rollback_attempts()
            .iter()
            .all(|attempt| attempt.result().is_ok())
    );
    assert_eq!(boundary.calls(), 4);
}

#[test]
fn rollback_continues_after_rollback_failure() {
    let root = FakeRoot::new();
    root.ready_trio();
    let boundary = RecordingBoundary::scripted(vec![
        Ok(()),
        Ok(()),
        Err(WriteBoundaryError::ExecutionFailed("fwd".to_owned())),
        Err(WriteBoundaryError::AccessDenied),
        Ok(()),
        Ok(()),
    ]);
    let transaction = root.applier(boundary.clone());
    let profile: Profile = TRIO_TOML.parse().unwrap();
    let error = transaction
        .apply(&profile)
        .expect_err("forward failure must fail the apply");
    let ProfileApplyError::Execution(failure) = error else {
        panic!("expected execution failure, got {error:?}");
    };
    assert_eq!(failure.rollback_attempts().len(), 3);
    assert!(failure.rollback_attempts()[0].result().is_err());
    assert!(failure.rollback_attempts()[1].result().is_ok());
    assert!(failure.rollback_attempts()[2].result().is_ok());
    assert_eq!(boundary.calls(), 6);
}

#[test]
fn readonly_apply_crosses_no_boundary() {
    let dir = tempdir().unwrap();
    let dmi = dir.path().join("sys/class/dmi/id");
    fs::create_dir_all(&dmi).unwrap();
    fs::write(dmi.join("sys_vendor"), b"Dell Inc.\n").unwrap();
    fs::write(dmi.join("product_name"), b"XPS 15\n").unwrap();
    let boundary = RecordingBoundary::succeeding();
    let transaction = ProfileTransactionExecutor::new(
        mec::hardware::SystemPaths::new(dir.path()),
        LinuxSysfsReader,
        boundary.clone(),
    );
    let profile: Profile = TRIO_TOML.parse().unwrap();
    let error = transaction
        .apply(&profile)
        .expect_err("read-only apply must fail");
    assert!(matches!(error, ProfileApplyError::PreviewRejected(_)));
    assert_eq!(boundary.calls(), 0);
}
