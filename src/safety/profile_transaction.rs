//! Reusable profile transaction execution with verified rollback.
//!
//! [`ProfileTransactionExecutor::apply`] accepts a [`Profile`] — never a
//! stored preview or plan — and prepares everything fresh inside the call:
//! fresh support evaluation, fresh capability discovery, a fresh
//! [`ProfilePlanner::preview`], a fresh [`HardwareSnapshot`], and a fresh
//! [`ProfileTransactionPlanner::plan`]. Only then does it execute forward
//! steps, each through [`HardwareCommandExecutor::execute`], rolling back
//! on failure. No stale preview or plan can authorize execution.

use std::fmt;

use thiserror::Error;

use crate::hardware::{
    BackendError, Capabilities, CapabilityDetector, CapabilityDiscoveryError,
    CommandValidationError, EcBackend, HardwareCommand, HardwareWriteBoundary, MsiEcBackend,
    SupportEvaluator, SupportMode, SysfsReader, SystemPaths,
};
use crate::profiles::{
    Profile, ProfileName, ProfilePlanner, ProfileTransactionPlan, ProfileTransactionPlanner,
    TransactionPlanError,
};

use super::{CommandExecutionError, HardwareCommandExecutor};

/// Reusable transaction engine: composition dependencies only, no cached
/// support state, capabilities, snapshots, previews, or plans. Every
/// [`apply`](Self::apply) call derives all of those again.
pub struct ProfileTransactionExecutor<R, W> {
    paths: SystemPaths,
    reader: R,
    executor: HardwareCommandExecutor<R, W>,
}

impl<R, W> ProfileTransactionExecutor<R, W>
where
    R: SysfsReader + Clone,
    W: HardwareWriteBoundary,
{
    /// Creates an engine over `paths` with the supplied reader and
    /// boundary. Stores only composition dependencies; all safety state is
    /// derived fresh per [`apply`](Self::apply) call.
    pub fn new(paths: SystemPaths, reader: R, boundary: W) -> Self {
        Self {
            executor: HardwareCommandExecutor::new(paths.clone(), reader.clone(), boundary),
            paths,
            reader,
        }
    }

    /// Applies `profile` through fresh preparation plus verified forward
    /// execution with best-effort reverse rollback. See the module
    /// documentation for the exact sequence.
    pub fn apply(&self, profile: &Profile) -> Result<ProfileApplyReport, ProfileApplyError> {
        // Fresh support evaluation on every call; never a stored verdict.
        let mode = SupportEvaluator::new(self.paths.clone(), self.reader.clone()).evaluate();
        // Fresh capability discovery for READY; read-only short-circuits
        // through pure validation with conservative empty capabilities.
        let capabilities = match &mode {
            SupportMode::Ready => CapabilityDetector::new(self.paths.clone(), self.reader.clone())
                .discover()
                .map_err(ProfileApplyError::CapabilityDiscovery)?,
            SupportMode::ReadOnly(_) => Capabilities::default(),
        };
        // Fresh preview; rejected entries fail before any snapshot or write.
        let preview = ProfilePlanner::preview(profile, &mode, &capabilities);
        if !preview.is_applicable() {
            let errors: Vec<CommandValidationError> = preview
                .entries()
                .iter()
                .filter_map(|entry| entry.status().error().cloned())
                .collect();
            return Err(ProfileApplyError::PreviewRejected(errors));
        }
        // Fresh rollback-baseline snapshot from the same root and reader.
        let snapshot = MsiEcBackend::new(self.paths.clone(), self.reader.clone())
            .snapshot()
            .map_err(ProfileApplyError::Snapshot)?;
        // Fresh reversible plan; missing rollback state fails closed here.
        let plan = ProfileTransactionPlanner::plan(&preview, &snapshot)
            .map_err(ProfileApplyError::Planning)?;
        if plan.is_noop() {
            return Ok(ProfileApplyReport {
                name: plan.name().clone(),
                applied: Vec::new(),
                unchanged: plan.unchanged().to_vec(),
            });
        }
        // Forward execution in plan order; every step goes through the
        // command executor, never directly to the boundary.
        let mut applied = Vec::new();
        for (index, step) in plan.steps().iter().enumerate() {
            if let Err(source) = self.executor.execute(step.forward()) {
                let rollback_attempts = self.rollback(&plan, index, &source);
                return Err(ProfileApplyError::Execution(Box::new(
                    ProfileApplyFailure {
                        failed_command: step.forward().clone(),
                        source,
                        applied_before_failure: applied,
                        rollback_attempts,
                    },
                )));
            }
            applied.push(step.forward().clone());
        }
        Ok(ProfileApplyReport {
            name: plan.name().clone(),
            applied,
            unchanged: plan.unchanged().to_vec(),
        })
    }

    /// Best-effort rollback for a failure at forward step `failed_index`:
    /// the failed step's own rollback first when it may have mutated
    /// hardware, then earlier applied steps in reverse order. Every
    /// rollback command goes through the command executor; failures are
    /// recorded and never stop the remaining attempts.
    fn rollback(
        &self,
        plan: &ProfileTransactionPlan,
        failed_index: usize,
        source: &CommandExecutionError,
    ) -> Vec<RollbackAttempt> {
        let mut candidates = Vec::new();
        if failed_step_may_have_mutated(source) {
            candidates.push(plan.steps()[failed_index].rollback().clone());
        }
        candidates.extend(
            plan.steps()[..failed_index]
                .iter()
                .rev()
                .map(|step| step.rollback().clone()),
        );
        candidates
            .into_iter()
            .map(|command| {
                let result = self.executor.execute(&command);
                RollbackAttempt { command, result }
            })
            .collect()
    }
}

/// Successful application: which commands were applied, in order, and which
/// requested commands were already satisfied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileApplyReport {
    name: ProfileName,
    applied: Vec<HardwareCommand>,
    unchanged: Vec<HardwareCommand>,
}

/// Why one `apply` call failed. The primary forward failure stays
/// authoritative; rollback outcomes are recorded separately and never
/// replace it.
#[derive(Debug, Error)]
pub enum ProfileApplyError {
    /// Fresh capability discovery failed before any preview.
    #[error("profile apply failed during capability discovery: {0}")]
    CapabilityDiscovery(#[from] CapabilityDiscoveryError),
    /// The fresh rollback-baseline snapshot could not be read.
    #[error("profile apply failed reading current state: {0}")]
    Snapshot(#[from] BackendError),
    /// Fresh preview rejected commands; nothing was written.
    #[error("profile apply failed: preview rejected")]
    PreviewRejected(Vec<CommandValidationError>),
    /// Fresh transaction planning failed; nothing was written.
    #[error("profile apply planning failed: {0}")]
    Planning(#[from] TransactionPlanError),
    /// A forward command failed; see the failure for rollback detail.
    #[error("profile apply failed: {0}")]
    Execution(Box<ProfileApplyFailure>),
}

/// A failed forward step: the exact command, its exact error, what had
/// already applied, and every rollback attempt with its own result.
#[derive(Debug)]
pub struct ProfileApplyFailure {
    failed_command: HardwareCommand,
    source: CommandExecutionError,
    applied_before_failure: Vec<HardwareCommand>,
    rollback_attempts: Vec<RollbackAttempt>,
}

/// One rollback attempt and its individual outcome.
#[derive(Debug)]
pub struct RollbackAttempt {
    command: HardwareCommand,
    result: Result<(), CommandExecutionError>,
}

impl ProfileApplyReport {
    /// Profile display name, preserved from the plan.
    pub fn name(&self) -> &ProfileName {
        &self.name
    }

    /// Successfully applied forward commands, in application order.
    pub fn applied(&self) -> &[HardwareCommand] {
        &self.applied
    }

    /// Requested commands already satisfied, in preview relative order.
    pub fn unchanged(&self) -> &[HardwareCommand] {
        &self.unchanged
    }

    /// True when there was nothing to mutate.
    pub fn is_noop(&self) -> bool {
        self.applied.is_empty()
    }
}

impl ProfileApplyFailure {
    /// The forward command that failed.
    pub fn failed_command(&self) -> &HardwareCommand {
        &self.failed_command
    }

    /// The exact error from the failed forward command.
    pub fn source(&self) -> &CommandExecutionError {
        &self.source
    }

    /// Forward commands successfully applied before the failure, in order.
    pub fn applied_before_failure(&self) -> &[HardwareCommand] {
        &self.applied_before_failure
    }

    /// Every rollback attempt with its individual result, in attempt order.
    pub fn rollback_attempts(&self) -> &[RollbackAttempt] {
        &self.rollback_attempts
    }
}

impl RollbackAttempt {
    /// The rollback command that was attempted.
    pub fn command(&self) -> &HardwareCommand {
        &self.command
    }

    /// The outcome of this rollback attempt.
    pub fn result(&self) -> &Result<(), CommandExecutionError> {
        &self.result
    }
}

impl fmt::Display for ProfileApplyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to apply {:?}: {}; {} applied before failure; {} rollback attempts",
            self.failed_command,
            self.source,
            self.applied_before_failure.len(),
            self.rollback_attempts.len()
        )
    }
}

/// Whether the failed step may have mutated hardware. Only a crossed
/// write boundary leaves doubt: validation and discovery failures happen
/// strictly before any crossing. Never inferred from display strings.
fn failed_step_may_have_mutated(error: &CommandExecutionError) -> bool {
    matches!(error, CommandExecutionError::Boundary(_))
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::hardware::{
        BatteryThreshold, CapabilityDiscoveryError, CommandValidationError, FanMode,
        HardwareWriteBoundary, LinuxSysfsReader, ShiftMode, SysfsError, SysfsReader,
        WriteBoundaryError,
    };
    use crate::profiles::Profile;
    use crate::safety::CommandExecutionError;

    const CANONICAL_TOML: &str = concat!(
        "name = \"Gaming\"\n",
        "\n",
        "[performance]\n",
        "shift_mode = \"turbo\"\n",
        "fan_mode = \"advanced\"\n",
        "cooler_boost = true\n",
        "super_battery = false\n",
        "\n",
        "[battery]\n",
        "charge_end_threshold = 80\n",
        "\n",
        "[device]\n",
        "keyboard_backlight = 2\n",
    );

    const TRIO_TOML: &str = concat!(
        "name = \"Trio\"\n",
        "\n",
        "[performance]\n",
        "shift_mode = \"turbo\"\n",
        "fan_mode = \"advanced\"\n",
        "cooler_boost = true\n",
    );

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    fn threshold(start: u8, end: u8) -> BatteryThreshold {
        BatteryThreshold::new(start, end).unwrap()
    }

    /// Test-only recording transport: records crossings in order and serves
    /// scripted results, defaulting to success. Never touches hardware.
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

        fn recorded(&self) -> Vec<HardwareCommand> {
            self.recorded.borrow().clone()
        }
    }

    impl HardwareWriteBoundary for RecordingBoundary {
        fn execute(&self, command: &HardwareCommand) -> Result<(), WriteBoundaryError> {
            self.recorded.borrow_mut().push(command.clone());
            self.scripted.borrow_mut().pop_front().unwrap_or(Ok(()))
        }
    }

    /// Test-only reader: path-suffixed `exists` probes vanish from the Nth
    /// call on, simulating capability loss mid-apply without chmod games.
    #[derive(Clone)]
    struct VanishAfter<R> {
        inner: R,
        suffix: &'static str,
        vanish_from: usize,
        calls: Rc<Cell<usize>>,
    }

    impl<R: SysfsReader> SysfsReader for VanishAfter<R> {
        fn exists(&self, path: &Path) -> Result<bool, SysfsError> {
            if path.ends_with(self.suffix) {
                let seen = self.calls.get() + 1;
                self.calls.set(seen);
                if seen >= self.vanish_from {
                    return Ok(false);
                }
            }
            self.inner.exists(path)
        }

        fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
            self.inner.read_string(path)
        }

        fn read_u8(&self, path: &Path) -> Result<u8, SysfsError> {
            self.inner.read_u8(path)
        }

        fn read_u16(&self, path: &Path) -> Result<u16, SysfsError> {
            self.inner.read_u16(path)
        }

        fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_dirs(path)
        }

        fn list_entries(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_entries(path)
        }
    }

    /// Test-only reader: path-suffixed `read_string` calls fail from the Nth
    /// call on, simulating discovery/read decay mid-apply.
    #[derive(Clone)]
    struct FailReadsAfter<R> {
        inner: R,
        suffix: &'static str,
        fail_from: usize,
        calls: Rc<Cell<usize>>,
    }

    impl<R: SysfsReader> SysfsReader for FailReadsAfter<R> {
        fn exists(&self, path: &Path) -> Result<bool, SysfsError> {
            self.inner.exists(path)
        }

        fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
            if path.ends_with(self.suffix) {
                let seen = self.calls.get() + 1;
                self.calls.set(seen);
                if seen >= self.fail_from {
                    return Err(SysfsError::PermissionDenied(path.to_path_buf()));
                }
            }
            self.inner.read_string(path)
        }

        fn read_u8(&self, path: &Path) -> Result<u8, SysfsError> {
            self.inner.read_u8(path)
        }

        fn read_u16(&self, path: &Path) -> Result<u16, SysfsError> {
            self.inner.read_u16(path)
        }

        fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_dirs(path)
        }

        fn list_entries(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_entries(path)
        }
    }

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

        fn transaction(
            &self,
            boundary: RecordingBoundary,
        ) -> ProfileTransactionExecutor<LinuxSysfsReader, RecordingBoundary> {
            ProfileTransactionExecutor::new(self.paths.clone(), LinuxSysfsReader, boundary)
        }

        fn write_dmi(&self, name: &str, contents: &[u8]) {
            let dir = self.root.path().join("sys/class/dmi/id");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(name), contents).unwrap();
        }

        fn write_msi_identity(&self) {
            self.write_dmi("sys_vendor", b"MSI\n");
            self.write_dmi("product_name", b"Test Laptop 15\n");
        }

        fn ec_dir(&self) -> PathBuf {
            self.root.path().join("sys/devices/platform/msi-ec")
        }

        fn ensure_ec_root(&self) {
            fs::create_dir_all(self.ec_dir()).unwrap();
        }

        fn ec_file(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self.ec_dir().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        fn ready_base(&self) {
            self.write_msi_identity();
            self.ensure_ec_root();
        }

        fn enable_fan_modes(&self, current: &str, available: &[&str]) {
            self.ec_file("fan_mode", format!("{current}\n").as_bytes());
            self.ec_file(
                "available_fan_modes",
                (available.join("\n") + "\n").as_bytes(),
            );
        }

        fn enable_shift_modes(&self, current: &str, available: &[&str]) {
            self.ec_file("shift_mode", format!("{current}\n").as_bytes());
            self.ec_file(
                "available_shift_modes",
                (available.join("\n") + "\n").as_bytes(),
            );
        }

        /// Full canonical fixture where every desired value differs from
        /// the readable current value.
        fn full_changed(&self) {
            self.ready_base();
            self.enable_shift_modes("comfort", &["comfort", "turbo"]);
            self.enable_fan_modes("auto", &["auto", "advanced"]);
            self.ec_file("cooler_boost", b"off\n");
            self.ec_file("super_battery", b"on\n");
            let entry = self.root.path().join("sys/class/power_supply/BAT0");
            fs::create_dir_all(&entry).unwrap();
            fs::write(entry.join("charge_control_start_threshold"), b"60\n").unwrap();
            fs::write(entry.join("charge_control_end_threshold"), b"70\n").unwrap();
            let backlight = self
                .root
                .path()
                .join("sys/class/leds/msiacpi::kbd_backlight");
            fs::create_dir_all(&backlight).unwrap();
            fs::write(backlight.join("brightness"), b"1\n").unwrap();
            fs::write(backlight.join("max_brightness"), b"3\n").unwrap();
        }

        fn canonical_profile() -> Profile {
            Profile::parse_toml(CANONICAL_TOML).unwrap()
        }

        fn expected_forwards() -> Vec<HardwareCommand> {
            vec![
                HardwareCommand::SetShiftMode(shift("turbo")),
                HardwareCommand::SetFanMode(fan("advanced")),
                HardwareCommand::SetCoolerBoost(true),
                HardwareCommand::SetSuperBattery(false),
                HardwareCommand::SetBatteryThreshold(threshold(70, 80)),
                HardwareCommand::SetKeyboardBacklight(2),
            ]
        }
    }

    #[test]
    fn readonly_apply_performs_zero_boundary_calls() {
        let fixture = Fixture::new();
        fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
        fixture.write_dmi("product_name", b"XPS 15\n");
        fixture.ensure_ec_root();
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("read-only apply must fail");
        assert!(
            matches!(error, ProfileApplyError::PreviewRejected(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn unsupported_capability_apply_performs_zero_calls() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        assert!(matches!(
            transaction.apply(&profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn unadvertised_fan_apply_performs_zero_calls() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto"]);
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"turbo\"\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("unadvertised mode must fail");
        assert!(
            matches!(
                &error,
                ProfileApplyError::PreviewRejected(errors)
                if errors == &vec![CommandValidationError::FanModeNotAdvertised(fan("turbo"))]
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"auto\n"
        );
    }

    #[test]
    fn snapshot_failure_performs_zero_calls() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("cooler_boost", b"bogus\n");
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("unreadable current value must fail closed");
        assert!(
            matches!(error, ProfileApplyError::Snapshot(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn missing_rollback_value_performs_zero_calls() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::succeeding();
        let reader = VanishAfter {
            inner: LinuxSysfsReader,
            suffix: "cooler_boost",
            vanish_from: 3,
            calls: Rc::new(Cell::new(0)),
        };
        let transaction =
            ProfileTransactionExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("vanished current value must fail closed");
        assert!(
            matches!(
                error,
                ProfileApplyError::Planning(TransactionPlanError::MissingCurrentValue(
                    "cooler boost"
                ))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn invalid_current_battery_pair_performs_zero_calls() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
        fs::write(entry.join("charge_control_end_threshold"), b"90\n").unwrap();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("incoherent current pair must fail closed");
        assert!(
            matches!(error, ProfileApplyError::Snapshot(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn noop_apply_performs_zero_calls() {
        let fixture = Fixture::new();
        fixture.full_changed();
        fixture.ec_file("shift_mode", b"turbo\n");
        fixture.ec_file("fan_mode", b"advanced\n");
        fixture.ec_file("cooler_boost", b"on\n");
        fixture.ec_file("super_battery", b"off\n");
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
        fs::write(entry.join("charge_control_end_threshold"), b"80\n").unwrap();
        let backlight = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight");
        fs::write(backlight.join("brightness"), b"2\n").unwrap();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let report = transaction
            .apply(&Fixture::canonical_profile())
            .expect("matching state must apply cleanly");
        assert!(report.is_noop());
        assert!(report.applied().is_empty());
        assert_eq!(report.unchanged().len(), 6);
        assert_eq!(report.name().as_str(), "Gaming");
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn one_step_transaction_executes_exact_forward() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap();
        let report = transaction.apply(&profile).expect("apply must succeed");
        assert_eq!(
            report.applied(),
            &[HardwareCommand::SetFanMode(fan("silent"))]
        );
        assert_eq!(boundary.recorded(), report.applied());
    }

    #[test]
    fn six_step_transaction_executes_exact_forward_order() {
        let fixture = Fixture::new();
        fixture.full_changed();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let report = transaction
            .apply(&Fixture::canonical_profile())
            .expect("apply must succeed");
        assert_eq!(report.applied(), &Fixture::expected_forwards());
        assert_eq!(boundary.recorded(), Fixture::expected_forwards());
        assert!(report.unchanged().is_empty());
        assert!(!report.is_noop());
    }

    #[test]
    fn report_unchanged_preserves_satisfied_commands() {
        let fixture = Fixture::new();
        fixture.full_changed();
        fixture.ec_file("fan_mode", b"advanced\n");
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let report = transaction
            .apply(&Fixture::canonical_profile())
            .expect("apply must succeed");
        assert_eq!(
            report.unchanged(),
            &[HardwareCommand::SetFanMode(fan("advanced"))]
        );
        assert_eq!(report.applied().len(), 5);
        assert_eq!(boundary.calls(), 5);
    }

    #[test]
    fn repeated_apply_reprepares_after_capability_loss() {
        let fixture = Fixture::new();
        fixture.full_changed();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        assert!(transaction.apply(&Fixture::canonical_profile()).is_ok());
        assert_eq!(boundary.calls(), 6);
        fs::remove_file(fixture.ec_dir().join("cooler_boost")).unwrap();
        let error = transaction
            .apply(&Fixture::canonical_profile())
            .expect_err("second apply must see the lost capability");
        assert!(matches!(error, ProfileApplyError::PreviewRejected(_)));
        assert_eq!(boundary.calls(), 6);
    }

    #[test]
    fn instance_reusable_without_cached_state() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let fan_only =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"silent\"\n").unwrap();
        let cooler_only =
            Profile::parse_toml("name = \"B\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        assert!(transaction.apply(&fan_only).is_ok());
        let report = transaction.apply(&cooler_only).expect("second profile");
        assert_eq!(report.name().as_str(), "B");
        assert_eq!(
            boundary.recorded(),
            vec![
                HardwareCommand::SetFanMode(fan("silent")),
                HardwareCommand::SetCoolerBoost(true),
            ]
        );
    }

    #[test]
    fn later_validation_failure_rolls_back_only_earlier() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["auto", "silent"]);
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::succeeding();
        let reader = FailReadsAfter {
            inner: LinuxSysfsReader,
            suffix: "available_fan_modes",
            fail_from: 6,
            calls: Rc::new(Cell::new(0)),
        };
        let transaction =
            ProfileTransactionExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        let profile = Profile::parse_toml(concat!(
            "name = \"Pair\"\n",
            "\n",
            "[performance]\n",
            "fan_mode = \"auto\"\n",
            "cooler_boost = true\n",
        ))
        .unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("later validation failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert_eq!(
            failure.failed_command(),
            &HardwareCommand::SetCoolerBoost(true)
        );
        assert!(
            matches!(
                failure.source(),
                CommandExecutionError::Validation(CommandValidationError::ReadOnly)
            ),
            "unexpected source: {:?}",
            failure.source()
        );
        assert_eq!(
            failure.applied_before_failure(),
            &[HardwareCommand::SetFanMode(fan("auto"))]
        );
        assert_eq!(
            boundary.recorded(),
            vec![HardwareCommand::SetFanMode(fan("auto"))]
        );
        assert_eq!(failure.rollback_attempts().len(), 1);
        assert_eq!(
            failure.rollback_attempts()[0].command(),
            &HardwareCommand::SetFanMode(fan("silent"))
        );
        assert!(
            failure.rollback_attempts()[0].result().is_err(),
            "rollback under decayed discovery must be recorded as failed"
        );
    }

    #[test]
    fn later_discovery_failure_step_is_not_rolled_back() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["auto", "silent"]);
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::succeeding();
        let reader = FailReadsAfter {
            inner: LinuxSysfsReader,
            suffix: "available_fan_modes",
            fail_from: 7,
            calls: Rc::new(Cell::new(0)),
        };
        let transaction =
            ProfileTransactionExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        let profile = Profile::parse_toml(concat!(
            "name = \"Pair\"\n",
            "\n",
            "[performance]\n",
            "fan_mode = \"auto\"\n",
            "cooler_boost = true\n",
        ))
        .unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("later discovery failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert!(
            matches!(
                failure.source(),
                CommandExecutionError::CapabilityDiscovery(_)
            ),
            "unexpected source: {:?}",
            failure.source()
        );
        assert_eq!(boundary.calls(), 1);
        assert_eq!(failure.rollback_attempts().len(), 1);
        assert_eq!(
            failure.rollback_attempts()[0].command(),
            &HardwareCommand::SetFanMode(fan("silent"))
        );
    }

    #[test]
    fn boundary_failure_rolls_back_failed_first_then_reverse() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_shift_modes("comfort", &["comfort", "turbo"]);
        fixture.enable_fan_modes("auto", &["auto", "advanced"]);
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::scripted(vec![
            Ok(()),
            Ok(()),
            Err(WriteBoundaryError::VerificationFailed("cooler boost")),
            Ok(()),
            Ok(()),
            Ok(()),
        ]);
        let transaction = fixture.transaction(boundary.clone());
        let profile = Profile::parse_toml(TRIO_TOML).unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("boundary failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert_eq!(
            failure.failed_command(),
            &HardwareCommand::SetCoolerBoost(true)
        );
        assert!(
            matches!(
                failure.source(),
                CommandExecutionError::Boundary(WriteBoundaryError::VerificationFailed(_))
            ),
            "unexpected source: {:?}",
            failure.source()
        );
        assert_eq!(
            failure.applied_before_failure(),
            &[
                HardwareCommand::SetShiftMode(shift("turbo")),
                HardwareCommand::SetFanMode(fan("advanced")),
            ]
        );
        let attempts: Vec<(&HardwareCommand, bool)> = failure
            .rollback_attempts()
            .iter()
            .map(|attempt| (attempt.command(), attempt.result().is_ok()))
            .collect();
        assert_eq!(
            attempts,
            vec![
                (&HardwareCommand::SetCoolerBoost(false), true),
                (&HardwareCommand::SetFanMode(fan("auto")), true),
                (&HardwareCommand::SetShiftMode(shift("comfort")), true),
            ]
        );
        assert_eq!(
            boundary.recorded(),
            vec![
                HardwareCommand::SetShiftMode(shift("turbo")),
                HardwareCommand::SetFanMode(fan("advanced")),
                HardwareCommand::SetCoolerBoost(true),
                HardwareCommand::SetCoolerBoost(false),
                HardwareCommand::SetFanMode(fan("auto")),
                HardwareCommand::SetShiftMode(shift("comfort")),
            ]
        );
    }

    #[test]
    fn access_denied_triggers_failed_step_rollback_first() {
        check_boundary_error_rolls_back_failed_first(WriteBoundaryError::AccessDenied);
    }

    #[test]
    fn unavailable_triggers_failed_step_rollback_first() {
        check_boundary_error_rolls_back_failed_first(WriteBoundaryError::Unavailable);
    }

    #[test]
    fn execution_failed_triggers_failed_step_rollback_first() {
        check_boundary_error_rolls_back_failed_first(WriteBoundaryError::ExecutionFailed(
            "write failed".to_owned(),
        ));
    }

    #[test]
    fn verification_failed_triggers_failed_step_rollback_first() {
        check_boundary_error_rolls_back_failed_first(WriteBoundaryError::VerificationFailed(
            "cooler boost",
        ));
    }

    fn check_boundary_error_rolls_back_failed_first(boundary_error: WriteBoundaryError) {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::scripted(vec![Err(boundary_error.clone()), Ok(())]);
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("boundary failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert_eq!(
            failure.failed_command(),
            &HardwareCommand::SetCoolerBoost(true)
        );
        assert!(matches!(
            failure.source(),
            CommandExecutionError::Boundary(_)
        ));
        assert!(failure.applied_before_failure().is_empty());
        assert_eq!(failure.rollback_attempts().len(), 1);
        assert_eq!(
            failure.rollback_attempts()[0].command(),
            &HardwareCommand::SetCoolerBoost(false)
        );
        assert!(failure.rollback_attempts()[0].result().is_ok());
        assert_eq!(
            boundary.recorded(),
            vec![
                HardwareCommand::SetCoolerBoost(true),
                HardwareCommand::SetCoolerBoost(false),
            ]
        );
    }

    #[test]
    fn rollback_validation_failure_is_recorded() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::scripted(vec![Err(WriteBoundaryError::ExecutionFailed(
            "boom".to_owned(),
        ))]);
        let reader = VanishAfter {
            inner: LinuxSysfsReader,
            suffix: "cooler_boost",
            vanish_from: 6,
            calls: Rc::new(Cell::new(0)),
        };
        let transaction =
            ProfileTransactionExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("forward boundary failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert!(matches!(
            failure.source(),
            CommandExecutionError::Boundary(_)
        ));
        assert_eq!(failure.rollback_attempts().len(), 1);
        assert!(matches!(
            failure.rollback_attempts()[0].result(),
            Err(CommandExecutionError::Validation(_))
        ));
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn rollback_discovery_failure_is_recorded() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["auto", "silent"]);
        let boundary = RecordingBoundary::scripted(vec![Err(WriteBoundaryError::ExecutionFailed(
            "boom".to_owned(),
        ))]);
        let reader = FailReadsAfter {
            inner: LinuxSysfsReader,
            suffix: "available_fan_modes",
            fail_from: 7,
            calls: Rc::new(Cell::new(0)),
        };
        let transaction =
            ProfileTransactionExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"auto\"\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("forward boundary failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert_eq!(failure.rollback_attempts().len(), 1);
        assert!(matches!(
            failure.rollback_attempts()[0].result(),
            Err(CommandExecutionError::CapabilityDiscovery(_))
        ));
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn rollback_boundary_failure_is_recorded() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::scripted(vec![
            Err(WriteBoundaryError::ExecutionFailed("fwd".to_owned())),
            Err(WriteBoundaryError::Unavailable),
        ]);
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("rollback boundary failure must surface");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert!(matches!(
            failure.rollback_attempts()[0].result(),
            Err(CommandExecutionError::Boundary(
                WriteBoundaryError::Unavailable
            ))
        ));
    }

    #[test]
    fn rollback_verification_failure_is_recorded_as_failure() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::scripted(vec![
            Err(WriteBoundaryError::ExecutionFailed("fwd".to_owned())),
            Err(WriteBoundaryError::VerificationFailed("cooler boost")),
        ]);
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("rollback verification failure must surface");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert!(matches!(
            failure.rollback_attempts()[0].result(),
            Err(CommandExecutionError::Boundary(
                WriteBoundaryError::VerificationFailed(_)
            ))
        ));
    }

    #[test]
    fn rollback_failure_does_not_stop_remaining_rollbacks() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_shift_modes("comfort", &["comfort", "turbo"]);
        fixture.enable_fan_modes("auto", &["auto", "advanced"]);
        fixture.ec_file("cooler_boost", b"off\n");
        let boundary = RecordingBoundary::scripted(vec![
            Ok(()),
            Ok(()),
            Err(WriteBoundaryError::ExecutionFailed("fwd".to_owned())),
            Err(WriteBoundaryError::ExecutionFailed("rb2".to_owned())),
            Ok(()),
            Ok(()),
        ]);
        let transaction = fixture.transaction(boundary.clone());
        let error = transaction
            .apply(&Profile::parse_toml(TRIO_TOML).unwrap())
            .expect_err("forward failure must fail the apply");
        let ProfileApplyError::Execution(failure) = error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert!(matches!(
            failure.source(),
            CommandExecutionError::Boundary(WriteBoundaryError::ExecutionFailed(message))
            if message == "fwd"
        ));
        assert_eq!(failure.rollback_attempts().len(), 3);
        assert!(failure.rollback_attempts()[0].result().is_err());
        assert!(failure.rollback_attempts()[1].result().is_ok());
        assert!(failure.rollback_attempts()[2].result().is_ok());
        assert_eq!(boundary.calls(), 6);
    }

    #[test]
    fn partial_profile_applies_only_requested_setting() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap();
        let report = transaction.apply(&profile).expect("apply must succeed");
        assert_eq!(
            boundary.recorded(),
            vec![HardwareCommand::SetFanMode(fan("silent"))]
        );
        assert!(!report.is_noop());
    }

    #[test]
    fn unchanged_setting_is_never_written() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap();
        let report = transaction.apply(&profile).expect("apply must succeed");
        assert!(report.is_noop());
        assert_eq!(
            report.unchanged(),
            &[HardwareCommand::SetFanMode(fan("silent"))]
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn mixed_profile_writes_only_changed_settings() {
        let fixture = Fixture::new();
        fixture.full_changed();
        fixture.ec_file("fan_mode", b"advanced\n");
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let report = transaction
            .apply(&Fixture::canonical_profile())
            .expect("apply must succeed");
        assert_eq!(boundary.calls(), 5);
        assert_eq!(
            report.unchanged(),
            &[HardwareCommand::SetFanMode(fan("advanced"))]
        );
        assert!(
            !boundary
                .recorded()
                .contains(&HardwareCommand::SetFanMode(fan("advanced")))
        );
    }

    #[test]
    fn battery_only_profile_writes_only_threshold() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), b"50\n").unwrap();
        fs::write(entry.join("charge_control_end_threshold"), b"60\n").unwrap();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 80\n")
                .unwrap();
        let report = transaction.apply(&profile).expect("apply must succeed");
        assert_eq!(
            boundary.recorded(),
            vec![HardwareCommand::SetBatteryThreshold(threshold(70, 80))]
        );
        assert_eq!(report.applied().len(), 1);
    }

    #[test]
    fn failure_then_success_grants_no_authorization() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto"]);
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"turbo\"\n").unwrap();
        assert!(matches!(
            transaction.apply(&profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert_eq!(boundary.calls(), 0);
        fixture.enable_fan_modes("auto", &["auto", "turbo"]);
        let report = transaction
            .apply(&profile)
            .expect("fresh preparation must see the fixed fixture");
        assert_eq!(
            report.applied(),
            &[HardwareCommand::SetFanMode(fan("turbo"))]
        );
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn profile_is_not_mutated_by_apply() {
        let fixture = Fixture::new();
        fixture.full_changed();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile = Fixture::canonical_profile();
        let before = profile.clone();
        let _ = transaction.apply(&profile);
        assert_eq!(profile, before);
    }

    #[test]
    fn accessors_expose_shared_references_only() {
        // Compile-level proof: report and failure accessors yield shared
        // references, so callers can inspect but never mutate results.
        let fixture = Fixture::new();
        fixture.full_changed();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let report = transaction
            .apply(&Fixture::canonical_profile())
            .expect("apply must succeed");
        let _: &ProfileName = report.name();
        let _: &[HardwareCommand] = report.applied();
        let _: &[HardwareCommand] = report.unchanged();
    }

    #[test]
    fn failure_accessors_expose_shared_references_only() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let boundary = RecordingBoundary::succeeding();
        let transaction = fixture.transaction(boundary.clone());
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error = transaction
            .apply(&profile)
            .expect_err("unsupported control must fail");
        let ProfileApplyError::PreviewRejected(errors) = &error else {
            panic!("expected preview rejection, got {error:?}");
        };
        let _: &Vec<CommandValidationError> = errors;
    }

    #[test]
    fn failed_step_classification() {
        assert!(!failed_step_may_have_mutated(
            &CommandExecutionError::Validation(CommandValidationError::ReadOnly)
        ));
        assert!(!failed_step_may_have_mutated(
            &CommandExecutionError::CapabilityDiscovery(
                CapabilityDiscoveryError::InconsistentInterface {
                    capability: "fan mode",
                    detail: "test",
                }
            )
        ));
        assert!(failed_step_may_have_mutated(
            &CommandExecutionError::Boundary(WriteBoundaryError::AccessDenied)
        ));
        assert!(failed_step_may_have_mutated(
            &CommandExecutionError::Boundary(WriteBoundaryError::Unavailable)
        ));
        assert!(failed_step_may_have_mutated(
            &CommandExecutionError::Boundary(WriteBoundaryError::ExecutionFailed(
                "test".to_owned()
            ))
        ));
        assert!(failed_step_may_have_mutated(
            &CommandExecutionError::Boundary(WriteBoundaryError::VerificationFailed("test"))
        ));
    }

    #[test]
    fn error_display_is_stable_and_human_readable() {
        assert_eq!(
            ProfileApplyError::PreviewRejected(vec![CommandValidationError::ReadOnly]).to_string(),
            "profile apply failed: preview rejected"
        );
        assert_eq!(
            ProfileApplyError::Snapshot(crate::hardware::BackendError::Unavailable).to_string(),
            "profile apply failed reading current state: hardware backend is unavailable"
        );
    }
}
