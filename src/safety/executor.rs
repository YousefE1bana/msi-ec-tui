//! Safe command execution coordinator.
//!
//! [`HardwareCommandExecutor`] sequences policy immediately before transport
//! on every attempt:
//!
//! ```text
//! Current support evaluation
//!         ↓
//! READ-ONLY? → reject (no further I/O, no boundary crossing)
//!         ↓
//! Fresh capability discovery
//!         ↓
//! HardwareCommand::validate
//!         ↓
//! HardwareWriteBoundary::execute
//! ```
//!
//! The executor stores no [`SupportMode`] and no [`Capabilities`]: both are
//! derived from [`SystemPaths`] plus the [`SysfsReader`] on every call, so a
//! verdict from attempt N can never authorize attempt N+1. There is no
//! storable authorization token. The boundary performs no validation itself.

use thiserror::Error;

use crate::hardware::{
    Capabilities, CapabilityDetector, CapabilityDiscoveryError, CommandValidationError,
    HardwareCommand, HardwareWriteBoundary, SupportEvaluator, SupportMode, SysfsReader,
    SystemPaths, WriteBoundaryError,
};

/// Coordinates validated execution: fresh support evaluation, fresh
/// capability discovery, pure validation, then exactly one boundary crossing.
pub struct HardwareCommandExecutor<R, W> {
    paths: SystemPaths,
    reader: R,
    boundary: W,
}

impl<R, W> HardwareCommandExecutor<R, W>
where
    R: SysfsReader + Clone,
    W: HardwareWriteBoundary,
{
    /// Creates an executor over `paths` with the supplied reader and
    /// boundary. Stores no support verdict and no capabilities.
    pub fn new(paths: SystemPaths, reader: R, boundary: W) -> Self {
        Self {
            paths,
            reader,
            boundary,
        }
    }

    /// Executes one command through the full safety sequence. Support state
    /// is re-evaluated and capabilities are freshly discovered on every
    /// call; the boundary is crossed only after all policy checks succeed.
    pub fn execute(&self, command: &HardwareCommand) -> Result<(), CommandExecutionError> {
        let mode = SupportEvaluator::new(self.paths.clone(), self.reader.clone()).evaluate();
        if matches!(mode, SupportMode::ReadOnly(_)) {
            // Single policy, no second verdict: route through the existing
            // pure validation with conservative empty capabilities. Read-only
            // short-circuits before capabilities matter, so no further I/O
            // happens on this path.
            command.validate(&mode, &Capabilities::default())?;
            // Fail closed: validation rejects every read-only command, so
            // reaching here must never authorize a boundary crossing.
            return Err(CommandExecutionError::Validation(
                CommandValidationError::ReadOnly,
            ));
        }
        let capabilities =
            CapabilityDetector::new(self.paths.clone(), self.reader.clone()).discover()?;
        command.validate(&SupportMode::Ready, &capabilities)?;
        self.boundary.execute(command)?;
        Ok(())
    }
}

/// Why one execution attempt failed. Fail-closed typed context only; no
/// sysfs paths and no reusable authorization.
#[derive(Debug, Error)]
pub enum CommandExecutionError {
    /// The command failed pure validation against current policy.
    #[error("command failed validation: {0}")]
    Validation(#[from] CommandValidationError),
    /// Fresh capability discovery failed after a READY verdict.
    #[error("command failed capability discovery: {0}")]
    CapabilityDiscovery(#[from] CapabilityDiscoveryError),
    /// The write boundary rejected or failed the validated command.
    #[error("command failed at the write boundary: {0}")]
    Boundary(#[from] WriteBoundaryError),
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::fs;
    use std::path::Path;
    use std::rc::Rc;

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::hardware::{
        BatteryThreshold, FanMode, LinuxSysfsReader, ReadOnlyReason, ShiftMode, SysfsError,
    };

    /// Test-only recording transport: records crossings, optionally scripts
    /// failures, never validates, never touches the filesystem.
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

        fn failing(results: Vec<Result<(), WriteBoundaryError>>) -> Self {
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

    /// Test-only reader that fails `available_fan_modes` reads after the
    /// first success, so support evaluation sees READY while the executor's
    /// own fresh discovery fails.
    #[derive(Clone)]
    struct FailSecondAvailableModesRead<R> {
        inner: R,
        reads: Rc<Cell<usize>>,
    }

    impl<R: SysfsReader> SysfsReader for FailSecondAvailableModesRead<R> {
        fn exists(&self, path: &Path) -> Result<bool, SysfsError> {
            self.inner.exists(path)
        }

        fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
            if path.ends_with("available_fan_modes") {
                let seen = self.reads.get();
                self.reads.set(seen + 1);
                if seen >= 1 {
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

        fn list_dirs(&self, path: &Path) -> Result<Vec<std::path::PathBuf>, SysfsError> {
            self.inner.list_dirs(path)
        }

        fn list_entries(&self, path: &Path) -> Result<Vec<std::path::PathBuf>, SysfsError> {
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

        fn executor(
            &self,
            boundary: RecordingBoundary,
        ) -> HardwareCommandExecutor<LinuxSysfsReader, RecordingBoundary> {
            HardwareCommandExecutor::new(self.paths.clone(), LinuxSysfsReader, boundary)
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

        fn read_ec(&self, relative: &str) -> String {
            fs::read_to_string(self.ec_dir().join(relative)).unwrap()
        }

        /// READY base: MSI identity plus an existing but empty driver root,
        /// so every capability is cleanly absent.
        fn ready_base(&self) {
            self.write_msi_identity();
            self.ensure_ec_root();
        }

        fn enable_fan_modes(&self, current: &str, available: &[&str]) {
            self.ec_file("fan_mode", format!("{current}\n").as_bytes());
            let list = available.join("\n") + "\n";
            self.ec_file("available_fan_modes", list.as_bytes());
        }

        fn enable_shift_modes(&self, current: &str, available: &[&str]) {
            self.ec_file("shift_mode", format!("{current}\n").as_bytes());
            let list = available.join("\n") + "\n";
            self.ec_file("available_shift_modes", list.as_bytes());
        }

        fn enable_presence(&self, name: &str) {
            self.ec_file(name, b"off\n");
        }

        fn enable_backlight(&self, max_brightness: u8) {
            let dir = self
                .root
                .path()
                .join("sys/class/leds/msiacpi::kbd_backlight");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("brightness"), b"1\n").unwrap();
            fs::write(
                dir.join("max_brightness"),
                format!("{max_brightness}\n").as_bytes(),
            )
            .unwrap();
        }

        fn enable_battery_thresholds(&self) {
            let entry = self.root.path().join("sys/class/power_supply/BAT0");
            fs::create_dir_all(&entry).unwrap();
            fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
            fs::write(entry.join("charge_control_end_threshold"), b"80\n").unwrap();
        }
    }

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    fn threshold() -> BatteryThreshold {
        BatteryThreshold::new(70, 80).unwrap()
    }

    fn every_command() -> Vec<HardwareCommand> {
        vec![
            HardwareCommand::SetFanMode(fan("auto")),
            HardwareCommand::SetShiftMode(shift("comfort")),
            HardwareCommand::SetCoolerBoost(true),
            HardwareCommand::SetSuperBattery(false),
            HardwareCommand::SetWebcam(true),
            HardwareCommand::SetWebcamBlock(false),
            HardwareCommand::SetKeyboardBacklight(2),
            HardwareCommand::SetBatteryThreshold(threshold()),
        ]
    }

    #[test]
    fn ready_supported_command_crosses_boundary_once() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("cooler_boost");
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let command = HardwareCommand::SetCoolerBoost(true);
        assert!(executor.execute(&command).is_ok());
        assert_eq!(boundary.calls(), 1);
        assert_eq!(boundary.recorded(), vec![command]);
    }

    #[test]
    fn boundary_receives_exact_threshold_command() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_battery_thresholds();
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let command = HardwareCommand::SetBatteryThreshold(threshold());
        assert!(executor.execute(&command).is_ok());
        let recorded = boundary.recorded();
        assert_eq!(recorded.len(), 1);
        let HardwareCommand::SetBatteryThreshold(received) = &recorded[0] else {
            panic!("expected the threshold command, got {:?}", recorded[0]);
        };
        assert_eq!(received.start_percent(), 70);
        assert_eq!(received.end_percent(), 80);
    }

    #[test]
    fn readonly_never_crosses_boundary() {
        let fixture = Fixture::new();
        fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
        fixture.write_dmi("product_name", b"XPS 15\n");
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        for command in every_command() {
            assert!(
                executor.execute(&command).is_err(),
                "expected rejection for {command:?}"
            );
        }
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn every_readonly_reason_blocks_execution() {
        fn check_blocked(name: &str, setup: fn(&Fixture)) {
            let fixture = Fixture::new();
            setup(&fixture);
            let boundary = RecordingBoundary::succeeding();
            let executor = fixture.executor(boundary.clone());
            let error = executor
                .execute(&HardwareCommand::SetCoolerBoost(true))
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    CommandExecutionError::Validation(CommandValidationError::ReadOnly)
                ),
                "{name}: expected read-only validation, got {error:?}"
            );
            assert_eq!(boundary.calls(), 0, "{name} must not cross the boundary");
        }

        check_blocked("non-msi", |f| {
            f.write_dmi("sys_vendor", b"Dell Inc.\n");
            f.write_dmi("product_name", b"XPS 15\n");
        });
        check_blocked("unverified", |f| {
            f.write_dmi("product_name", b"GF63 Thin\n");
        });
        check_blocked("unavailable", |f| {
            f.write_msi_identity();
        });
        check_blocked("unreadable", |f| {
            f.write_msi_identity();
            let path = f.ec_dir();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"not a directory\n").unwrap();
        });
        check_blocked("inconsistent", |f| {
            f.write_msi_identity();
            f.ensure_ec_root();
            f.ec_file("fan_mode", b"auto\n");
        });
        // Keep the reason list honest if the domain ever grows.
        let _ = [
            ReadOnlyReason::NonMsiHardware,
            ReadOnlyReason::UnverifiedHardwareIdentity,
            ReadOnlyReason::MsiEcUnavailable,
            ReadOnlyReason::MsiEcUnreadable,
            ReadOnlyReason::InconsistentInterface,
        ];
    }

    #[test]
    fn unsupported_boolean_capabilities_never_cross() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        for command in [
            HardwareCommand::SetCoolerBoost(true),
            HardwareCommand::SetCoolerBoost(false),
            HardwareCommand::SetSuperBattery(true),
            HardwareCommand::SetSuperBattery(false),
            HardwareCommand::SetWebcam(true),
            HardwareCommand::SetWebcam(false),
            HardwareCommand::SetWebcamBlock(true),
            HardwareCommand::SetWebcamBlock(false),
        ] {
            assert!(
                executor.execute(&command).is_err(),
                "expected rejection for {command:?}"
            );
        }
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn unadvertised_fan_mode_never_crosses() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetFanMode(fan("turbo")))
            .expect_err("unadvertised fan mode must be rejected");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::FanModeNotAdvertised(_))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn unadvertised_shift_mode_never_crosses() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_shift_modes("comfort", &["eco", "comfort"]);
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetShiftMode(shift("sport")))
            .expect_err("unadvertised shift mode must be rejected");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::ShiftModeNotAdvertised(
                    _
                ))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn above_max_backlight_never_crosses() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_backlight(3);
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetKeyboardBacklight(4))
            .expect_err("level above maximum must be rejected");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::BacklightAboveMaximum {
                    level: 4,
                    max: 3
                })
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn unsupported_battery_thresholds_never_cross() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetBatteryThreshold(threshold()))
            .expect_err("unsupported thresholds must be rejected");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::UnsupportedCapability(_))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn validation_error_preserved_as_typed_variant() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetWebcam(true))
            .expect_err("absent webcam control must be rejected");
        assert!(
            matches!(error, CommandExecutionError::Validation(_)),
            "expected Validation, got {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn capability_discovery_failure_never_crosses() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let reader = FailSecondAvailableModesRead {
            inner: LinuxSysfsReader,
            reads: Rc::new(Cell::new(0)),
        };
        let executor =
            HardwareCommandExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetFanMode(fan("auto")))
            .expect_err("discovery failure must fail closed");
        assert!(
            matches!(error, CommandExecutionError::CapabilityDiscovery(_)),
            "expected discovery error, got {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn capability_discovery_error_preserved_as_typed_variant() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let reader = FailSecondAvailableModesRead {
            inner: LinuxSysfsReader,
            reads: Rc::new(Cell::new(0)),
        };
        let executor =
            HardwareCommandExecutor::new(fixture.paths.clone(), reader, boundary.clone());
        // A command needing no modes still forces fresh discovery first.
        let error = executor
            .execute(&HardwareCommand::SetWebcam(true))
            .expect_err("discovery failure must surface even for other commands");
        assert!(
            matches!(error, CommandExecutionError::CapabilityDiscovery(_)),
            "expected typed discovery error, got {error:?}"
        );
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn access_denied_boundary_error_propagates() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("cooler_boost");
        let boundary = RecordingBoundary::failing(vec![Err(WriteBoundaryError::AccessDenied)]);
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetCoolerBoost(true))
            .expect_err("access denied must propagate");
        assert!(
            matches!(
                error,
                CommandExecutionError::Boundary(WriteBoundaryError::AccessDenied)
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn unavailable_boundary_error_propagates() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("cooler_boost");
        let boundary = RecordingBoundary::failing(vec![Err(WriteBoundaryError::Unavailable)]);
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetCoolerBoost(false))
            .expect_err("unavailable must propagate");
        assert!(
            matches!(
                error,
                CommandExecutionError::Boundary(WriteBoundaryError::Unavailable)
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn execution_failed_boundary_error_propagates() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("cooler_boost");
        let boundary = RecordingBoundary::failing(vec![Err(WriteBoundaryError::ExecutionFailed(
            "fan write rejected".to_owned(),
        ))]);
        let executor = fixture.executor(boundary.clone());
        let error = executor
            .execute(&HardwareCommand::SetCoolerBoost(true))
            .expect_err("execution failure must propagate");
        assert!(
            matches!(
                error,
                CommandExecutionError::Boundary(WriteBoundaryError::ExecutionFailed(_))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn successful_execution_returns_ok() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_shift_modes("comfort", &["eco", "comfort"]);
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let command = HardwareCommand::SetShiftMode(shift("comfort"));
        assert!(executor.execute(&command).is_ok());
        assert_eq!(boundary.recorded(), vec![command]);
    }

    #[test]
    fn executor_performs_no_hardware_mutation() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("cooler_boost");
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        assert!(
            executor
                .execute(&HardwareCommand::SetCoolerBoost(true))
                .is_ok()
        );
        // The desired state crossed the boundary, but the sysfs tree the
        // executor may only read is byte-identical afterwards.
        assert_eq!(
            boundary.recorded(),
            vec![HardwareCommand::SetCoolerBoost(true)]
        );
        assert_eq!(fixture.read_ec("cooler_boost"), "off\n");
        assert_eq!(fixture.read_ec("available_fan_modes"), "auto\nsilent\n");
    }

    #[test]
    fn execution_needs_no_backend_types() {
        // Compile-level proof: this test names no EcBackend and no
        // MsiEcBackend; execution needs only paths, a reader, and the
        // boundary.
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("webcam");
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        assert!(executor.execute(&HardwareCommand::SetWebcam(false)).is_ok());
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn support_state_is_reevaluated_on_every_attempt() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("cooler_boost");
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let command = HardwareCommand::SetCoolerBoost(true);
        assert!(executor.execute(&command).is_ok());
        assert_eq!(boundary.calls(), 1);
        // Transition to non-MSI hardware between attempts.
        fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
        let error = executor
            .execute(&command)
            .expect_err("second attempt must observe the new read-only state");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::ReadOnly)
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn capabilities_are_freshly_discovered_on_every_ready_attempt() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let command = HardwareCommand::SetFanMode(fan("auto"));
        assert!(executor.execute(&command).is_ok());
        assert_eq!(boundary.calls(), 1);
        // Stay READY but stop advertising "auto".
        fixture.enable_fan_modes("silent", &["silent"]);
        let error = executor
            .execute(&command)
            .expect_err("second attempt must observe the new capability set");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::FanModeNotAdvertised(_))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(boundary.calls(), 1);
    }

    #[test]
    fn repeated_success_crosses_on_each_attempt() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_presence("super_battery");
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        let command = HardwareCommand::SetSuperBattery(true);
        assert!(executor.execute(&command).is_ok());
        assert!(executor.execute(&command).is_ok());
        assert_eq!(boundary.calls(), 2);
    }

    #[test]
    fn failed_validations_never_increment_boundary() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let boundary = RecordingBoundary::succeeding();
        let executor = fixture.executor(boundary.clone());
        for _ in 0..3 {
            assert!(
                executor
                    .execute(&HardwareCommand::SetCoolerBoost(true))
                    .is_err()
            );
        }
        assert_eq!(boundary.calls(), 0);
    }

    #[test]
    fn error_display_is_stable_and_human_readable() {
        assert_eq!(
            CommandExecutionError::Validation(CommandValidationError::ReadOnly).to_string(),
            "command failed validation: hardware command rejected while MEC is read-only"
        );
        assert_eq!(
            CommandExecutionError::Boundary(WriteBoundaryError::AccessDenied).to_string(),
            "command failed at the write boundary: hardware write access denied"
        );
        assert_eq!(
            CommandExecutionError::CapabilityDiscovery(
                CapabilityDiscoveryError::InvalidBacklight {
                    detail: "max_brightness must be at least 1",
                }
            )
            .to_string(),
            "command failed capability discovery: invalid keyboard backlight: \
             max_brightness must be at least 1"
        );
    }
}
