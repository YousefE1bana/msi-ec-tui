//! Narrow execution-transport contract for validated hardware commands.
//!
//! [`HardwareWriteBoundary`] carries typed [`HardwareCommand`] values across
//! the privilege/write transport boundary. It performs no validation
//! itself: [`HardwareCommand::validate`] is a point-in-time safety policy,
//! and support/capability verdicts can go stale, so validation must happen
//! immediately before every crossing in the final executor. There is no
//! storable authorization token, no path/value API, and no production
//! implementation yet.

use thiserror::Error;

use super::HardwareCommand;

/// The privilege/write transport boundary for validated hardware commands.
///
/// Transport only: implementations execute the typed command they receive
/// and report transport failures. They must not reinterpret the command as
/// a path, shell invocation, or register access, and must not treat a
/// received command as proof of a still-current safety verdict.
pub trait HardwareWriteBoundary {
    /// Executes one typed command across the boundary.
    fn execute(&self, command: &HardwareCommand) -> Result<(), WriteBoundaryError>;
}

/// Why a boundary crossing failed. Carries human-readable context only;
/// never paths, command lines, or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WriteBoundaryError {
    /// The transport refused the write for privilege reasons.
    #[error("hardware write access denied")]
    AccessDenied,
    /// No write transport is available.
    #[error("hardware write transport unavailable")]
    Unavailable,
    /// The write was attempted but failed.
    #[error("hardware write failed: {0}")]
    ExecutionFailed(String),
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;
    use crate::hardware::{BatteryThreshold, FanMode, HardwareCommand, ShiftMode, SupportMode};

    /// In-memory recording transport used only to prove the contract shape.
    /// Deliberately never calls `HardwareCommand::validate`: policy and
    /// transport stay separate by construction.
    struct RecordingBoundary {
        recorded: RefCell<Vec<HardwareCommand>>,
        scripted: RefCell<VecDeque<Result<(), WriteBoundaryError>>>,
    }

    impl RecordingBoundary {
        fn succeeding() -> Self {
            Self {
                recorded: RefCell::new(Vec::new()),
                scripted: RefCell::new(VecDeque::new()),
            }
        }

        fn failing(results: Vec<Result<(), WriteBoundaryError>>) -> Self {
            Self {
                recorded: RefCell::new(Vec::new()),
                scripted: RefCell::new(results.into()),
            }
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

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    #[test]
    fn boundary_accepts_fan_mode_as_typed_value() {
        let boundary = RecordingBoundary::succeeding();
        let command = HardwareCommand::SetFanMode(fan("auto"));
        assert!(boundary.execute(&command).is_ok());
        assert_eq!(boundary.recorded(), vec![command]);
    }

    #[test]
    fn boundary_receives_shift_mode_exactly() {
        let boundary = RecordingBoundary::succeeding();
        let command = HardwareCommand::SetShiftMode(shift("sport"));
        assert!(boundary.execute(&command).is_ok());
        assert_eq!(boundary.recorded(), vec![command]);
    }

    #[test]
    fn boundary_preserves_boolean_desired_states() {
        let boundary = RecordingBoundary::succeeding();
        for command in [
            HardwareCommand::SetCoolerBoost(false),
            HardwareCommand::SetSuperBattery(true),
            HardwareCommand::SetWebcam(false),
            HardwareCommand::SetWebcamBlock(true),
        ] {
            assert!(boundary.execute(&command).is_ok());
        }
        assert_eq!(
            boundary.recorded(),
            vec![
                HardwareCommand::SetCoolerBoost(false),
                HardwareCommand::SetSuperBattery(true),
                HardwareCommand::SetWebcam(false),
                HardwareCommand::SetWebcamBlock(true),
            ]
        );
    }

    #[test]
    fn boundary_preserves_backlight_level() {
        let boundary = RecordingBoundary::succeeding();
        assert!(
            boundary
                .execute(&HardwareCommand::SetKeyboardBacklight(2))
                .is_ok()
        );
        assert_eq!(
            boundary.recorded(),
            vec![HardwareCommand::SetKeyboardBacklight(2)]
        );
    }

    #[test]
    fn boundary_preserves_threshold_pair() {
        let boundary = RecordingBoundary::succeeding();
        let command = HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap());
        assert!(boundary.execute(&command).is_ok());
        let recorded = boundary.recorded();
        let [HardwareCommand::SetBatteryThreshold(received)] = recorded.as_slice() else {
            panic!("expected exactly one recorded threshold command");
        };
        assert_eq!(received.start_percent(), 70);
        assert_eq!(received.end_percent(), 80);
    }

    #[test]
    fn fake_records_command_order() {
        let boundary = RecordingBoundary::succeeding();
        let first = HardwareCommand::SetWebcam(true);
        let second = HardwareCommand::SetFanMode(fan("silent"));
        let third = HardwareCommand::SetCoolerBoost(false);
        for command in [&first, &second, &third] {
            assert!(boundary.execute(command).is_ok());
        }
        assert_eq!(boundary.recorded(), vec![first, second, third]);
    }

    #[test]
    fn fake_returns_access_denied() {
        let boundary = RecordingBoundary::failing(vec![Err(WriteBoundaryError::AccessDenied)]);
        assert_eq!(
            boundary.execute(&HardwareCommand::SetWebcam(true)),
            Err(WriteBoundaryError::AccessDenied)
        );
    }

    #[test]
    fn fake_returns_unavailable() {
        let boundary = RecordingBoundary::failing(vec![Err(WriteBoundaryError::Unavailable)]);
        assert_eq!(
            boundary.execute(&HardwareCommand::SetFanMode(fan("auto"))),
            Err(WriteBoundaryError::Unavailable)
        );
    }

    #[test]
    fn fake_returns_execution_failed_with_context() {
        let boundary = RecordingBoundary::failing(vec![Err(WriteBoundaryError::ExecutionFailed(
            "fan write rejected".to_owned(),
        ))]);
        assert_eq!(
            boundary.execute(&HardwareCommand::SetFanMode(fan("auto"))),
            Err(WriteBoundaryError::ExecutionFailed(
                "fan write rejected".to_owned()
            ))
        );
    }

    #[test]
    fn error_display_is_stable_and_human_readable() {
        assert_eq!(
            WriteBoundaryError::AccessDenied.to_string(),
            "hardware write access denied"
        );
        assert_eq!(
            WriteBoundaryError::Unavailable.to_string(),
            "hardware write transport unavailable"
        );
        assert_eq!(
            WriteBoundaryError::ExecutionFailed("fan write rejected".to_owned()).to_string(),
            "hardware write failed: fan write rejected"
        );
    }

    #[test]
    fn boundary_needs_no_filesystem_or_backend_types() {
        // Compile-level proof: RecordingBoundary is built from nothing but
        // scripted results, and execute() takes only &HardwareCommand — no
        // SysfsReader, SystemPaths, or MsiEcBackend is involved.
        let boundary = RecordingBoundary::succeeding();
        assert!(
            boundary
                .execute(&HardwareCommand::SetSuperBattery(false))
                .is_ok()
        );
    }

    #[test]
    fn boundary_performs_no_implicit_validation() {
        // A command that pure validation would reject still crosses the
        // transport untouched: policy lives in validate(), not here.
        let boundary = RecordingBoundary::succeeding();
        let unadvertised = HardwareCommand::SetFanMode(fan("never-advertised"));
        assert!(boundary.execute(&unadvertised).is_ok());
        assert_eq!(boundary.recorded(), vec![unadvertised]);
    }

    #[test]
    fn validation_behavior_remains_unchanged() {
        use crate::hardware::{Capabilities, CommandValidationError, ReadOnlyReason};

        let command = HardwareCommand::SetCoolerBoost(true);
        assert_eq!(
            command.validate(
                &SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware),
                &Capabilities::default(),
            ),
            Err(CommandValidationError::ReadOnly)
        );
        assert!(
            command
                .validate(
                    &SupportMode::Ready,
                    &Capabilities {
                        cooler_boost: true,
                        ..Default::default()
                    }
                )
                .is_ok()
        );
    }
}
