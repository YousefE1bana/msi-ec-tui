//! Typed hardware change requests with pure validation.
//!
//! A [`HardwareCommand`] describes a requested change only. Validation is a
//! pure check against [`SupportMode`] and [`Capabilities`]; it performs no
//! mutation and no I/O. Execution lives behind a future write boundary.

use thiserror::Error;

use super::{Capabilities, FanMode, ShiftMode, SupportMode};

/// Validated start/end battery charge thresholds, in percent.
///
/// Valid by construction for the `msi-ec` backend: the two sysfs files
/// describe one EC charge-control state with a fixed 10-percentage-point
/// hysteresis, so `end_percent` is always `start_percent + 10`, start is
/// within `0..=90`, and end is within `10..=100`. Values are never clamped,
/// reordered, or normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryThreshold {
    start_percent: u8,
    end_percent: u8,
}

/// Why a [`BatteryThreshold`] pair was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum BatteryThresholdError {
    /// The start percent is outside the representable `0..=90` range.
    #[error("battery threshold start {0} is outside 0 to 90 percent")]
    StartOutOfRange(u8),
    /// The end percent is outside the representable `10..=100` range.
    #[error("battery threshold end {0} is outside 10 to 100 percent")]
    EndOutOfRange(u8),
    /// The pair does not span exactly 10 percentage points of hysteresis.
    #[error("battery thresholds must span exactly 10 percent hysteresis: start {start}, end {end}")]
    UnsupportedGap { start: u8, end: u8 },
}

impl BatteryThreshold {
    /// Builds a threshold pair without clamping or reordering. Only pairs
    /// with `end == start + 10` are representable by `msi-ec`.
    pub fn new(start_percent: u8, end_percent: u8) -> Result<Self, BatteryThresholdError> {
        if start_percent > 90 {
            return Err(BatteryThresholdError::StartOutOfRange(start_percent));
        }
        if !(10..=100).contains(&end_percent) {
            return Err(BatteryThresholdError::EndOutOfRange(end_percent));
        }
        if end_percent != start_percent + 10 {
            return Err(BatteryThresholdError::UnsupportedGap {
                start: start_percent,
                end: end_percent,
            });
        }
        Ok(Self {
            start_percent,
            end_percent,
        })
    }

    /// Derives the pair from a charge-limit end percent, for a future
    /// `mec battery limit <end>` contract. Requires `10..=100` and derives
    /// `start = end - 10` through the same validation.
    pub fn from_end_percent(end_percent: u8) -> Result<Self, BatteryThresholdError> {
        if !(10..=100).contains(&end_percent) {
            return Err(BatteryThresholdError::EndOutOfRange(end_percent));
        }
        Self::new(end_percent - 10, end_percent)
    }

    /// Charge level where charging starts, in percent.
    pub fn start_percent(&self) -> u8 {
        self.start_percent
    }

    /// Charge level where charging stops, in percent.
    pub fn end_percent(&self) -> u8 {
        self.end_percent
    }
}

/// A typed requested hardware change.
///
/// Domain values only: no paths, no raw strings, no register access. A
/// constructed command is a request, not an authorization or an execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareCommand {
    /// Switch to an advertised fan mode.
    SetFanMode(FanMode),
    /// Switch to an advertised shift mode.
    SetShiftMode(ShiftMode),
    /// Enable or disable cooler boost.
    SetCoolerBoost(bool),
    /// Enable or disable super battery mode.
    SetSuperBattery(bool),
    /// Enable or disable the webcam.
    SetWebcam(bool),
    /// Enable or disable webcam blocking.
    SetWebcamBlock(bool),
    /// Set the keyboard backlight level.
    SetKeyboardBacklight(u8),
    /// Set the battery charge thresholds.
    SetBatteryThreshold(BatteryThreshold),
}

/// Why a [`HardwareCommand`] failed pure validation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandValidationError {
    /// MEC is read-only; capabilities never override the mode verdict.
    #[error("hardware command rejected while MEC is read-only")]
    ReadOnly,
    /// A boolean or threshold control the device does not expose.
    #[error("unsupported capability: {0}")]
    UnsupportedCapability(&'static str),
    /// A syntactically valid fan mode the device did not advertise.
    #[error("fan mode not advertised: {0}")]
    FanModeNotAdvertised(FanMode),
    /// A syntactically valid shift mode the device did not advertise.
    #[error("shift mode not advertised: {0}")]
    ShiftModeNotAdvertised(ShiftMode),
    /// A backlight level above the device maximum.
    #[error("keyboard backlight level {level} exceeds device maximum {max}")]
    BacklightAboveMaximum { level: u8, max: u8 },
}

impl HardwareCommand {
    /// Pure validation against the support verdict and startup
    /// capabilities. Takes only shared references and performs no I/O.
    pub fn validate(
        &self,
        mode: &SupportMode,
        capabilities: &Capabilities,
    ) -> Result<(), CommandValidationError> {
        if matches!(mode, SupportMode::ReadOnly(_)) {
            return Err(CommandValidationError::ReadOnly);
        }
        match self {
            HardwareCommand::SetFanMode(mode) => {
                if capabilities.fan_modes.contains(mode) {
                    Ok(())
                } else {
                    Err(CommandValidationError::FanModeNotAdvertised(mode.clone()))
                }
            }
            HardwareCommand::SetShiftMode(mode) => {
                if capabilities.shift_modes.contains(mode) {
                    Ok(())
                } else {
                    Err(CommandValidationError::ShiftModeNotAdvertised(mode.clone()))
                }
            }
            HardwareCommand::SetCoolerBoost(_) => {
                require(capabilities.cooler_boost, "cooler boost")
            }
            HardwareCommand::SetSuperBattery(_) => {
                require(capabilities.super_battery, "super battery")
            }
            HardwareCommand::SetWebcam(_) => require(capabilities.webcam, "webcam"),
            HardwareCommand::SetWebcamBlock(_) => {
                require(capabilities.webcam_block, "webcam block")
            }
            HardwareCommand::SetKeyboardBacklight(level) => {
                match &capabilities.keyboard_backlight {
                    Some(capability) if *level <= capability.max_brightness => Ok(()),
                    Some(capability) => Err(CommandValidationError::BacklightAboveMaximum {
                        level: *level,
                        max: capability.max_brightness,
                    }),
                    None => Err(CommandValidationError::UnsupportedCapability(
                        "keyboard backlight",
                    )),
                }
            }
            HardwareCommand::SetBatteryThreshold(_) => {
                require(capabilities.battery_thresholds, "battery thresholds")
            }
        }
    }
}

/// Accepts when a boolean capability control exists.
fn require(supported: bool, capability: &'static str) -> Result<(), CommandValidationError> {
    if supported {
        Ok(())
    } else {
        Err(CommandValidationError::UnsupportedCapability(capability))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{
        BacklightCapability, Capabilities, FanMode, ReadOnlyReason, ShiftMode, SupportMode,
    };

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    fn full_capabilities() -> Capabilities {
        Capabilities {
            cpu_temperature: true,
            gpu_temperature: true,
            cpu_fan: true,
            gpu_fan: true,
            fan_modes: vec![fan("auto"), fan("silent")],
            shift_modes: vec![shift("comfort"), shift("sport")],
            cooler_boost: true,
            super_battery: true,
            webcam: true,
            webcam_block: true,
            fn_key: false,
            win_key: false,
            keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
            battery_thresholds: true,
        }
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
            HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap()),
        ]
    }

    #[test]
    fn threshold_minimum_pair_accepted() {
        assert!(BatteryThreshold::new(0, 10).is_ok());
    }

    #[test]
    fn threshold_typical_pair_accepted() {
        assert!(BatteryThreshold::new(50, 60).is_ok());
    }

    #[test]
    fn threshold_representative_pair_accepted() {
        assert!(BatteryThreshold::new(70, 80).is_ok());
    }

    #[test]
    fn threshold_maximum_pair_accepted() {
        assert!(BatteryThreshold::new(90, 100).is_ok());
    }

    #[test]
    fn threshold_full_range_rejected() {
        assert!(BatteryThreshold::new(0, 100).is_err());
    }

    #[test]
    fn threshold_wide_gap_rejected() {
        assert_eq!(
            BatteryThreshold::new(50, 80),
            Err(BatteryThresholdError::UnsupportedGap { start: 50, end: 80 })
        );
    }

    #[test]
    fn threshold_equal_values_rejected() {
        assert!(BatteryThreshold::new(80, 80).is_err());
        assert!(BatteryThreshold::new(50, 50).is_err());
    }

    #[test]
    fn threshold_start_above_ninety_rejected() {
        assert_eq!(
            BatteryThreshold::new(91, 100),
            Err(BatteryThresholdError::StartOutOfRange(91))
        );
    }

    #[test]
    fn threshold_short_gap_rejected() {
        assert_eq!(
            BatteryThreshold::new(90, 99),
            Err(BatteryThresholdError::UnsupportedGap { start: 90, end: 99 })
        );
    }

    #[test]
    fn threshold_start_above_100_rejected() {
        assert!(BatteryThreshold::new(101, 100).is_err());
    }

    #[test]
    fn threshold_end_above_100_rejected() {
        assert!(BatteryThreshold::new(50, 101).is_err());
    }

    #[test]
    fn threshold_end_below_10_rejected() {
        assert_eq!(
            BatteryThreshold::new(0, 9),
            Err(BatteryThresholdError::EndOutOfRange(9))
        );
    }

    #[test]
    fn threshold_start_after_end_rejected() {
        assert!(BatteryThreshold::new(90, 80).is_err());
    }

    #[test]
    fn threshold_accessors_preserve_input() {
        let threshold = BatteryThreshold::new(70, 80).unwrap();
        assert_eq!(threshold.start_percent(), 70);
        assert_eq!(threshold.end_percent(), 80);
    }

    #[test]
    fn threshold_from_end_derives_start() {
        assert_eq!(
            BatteryThreshold::from_end_percent(10).unwrap(),
            BatteryThreshold::new(0, 10).unwrap()
        );
        assert_eq!(
            BatteryThreshold::from_end_percent(60).unwrap(),
            BatteryThreshold::new(50, 60).unwrap()
        );
        assert_eq!(
            BatteryThreshold::from_end_percent(80).unwrap(),
            BatteryThreshold::new(70, 80).unwrap()
        );
        assert_eq!(
            BatteryThreshold::from_end_percent(100).unwrap(),
            BatteryThreshold::new(90, 100).unwrap()
        );
    }

    #[test]
    fn threshold_from_end_rejects_outside_representable_range() {
        assert_eq!(
            BatteryThreshold::from_end_percent(9),
            Err(BatteryThresholdError::EndOutOfRange(9))
        );
        assert_eq!(
            BatteryThreshold::from_end_percent(101),
            Err(BatteryThresholdError::EndOutOfRange(101))
        );
    }

    #[test]
    fn readonly_rejects_every_command() {
        let mode = SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware);
        for command in every_command() {
            assert_eq!(
                command.validate(&mode, &Capabilities::default()),
                Err(CommandValidationError::ReadOnly),
                "{command:?}"
            );
        }
    }

    #[test]
    fn readonly_rejection_beats_full_capabilities() {
        let mode = SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface);
        for command in every_command() {
            assert_eq!(
                command.validate(&mode, &full_capabilities()),
                Err(CommandValidationError::ReadOnly),
                "{command:?}"
            );
        }
    }

    #[test]
    fn readonly_rejection_covers_every_reason() {
        let reasons = [
            ReadOnlyReason::NonMsiHardware,
            ReadOnlyReason::UnverifiedHardwareIdentity,
            ReadOnlyReason::MsiEcUnavailable,
            ReadOnlyReason::MsiEcUnreadable,
            ReadOnlyReason::InconsistentInterface,
        ];
        for reason in reasons {
            let mode = SupportMode::ReadOnly(reason);
            let command = HardwareCommand::SetCoolerBoost(true);
            assert_eq!(
                command.validate(&mode, &full_capabilities()),
                Err(CommandValidationError::ReadOnly)
            );
        }
    }

    #[test]
    fn advertised_fan_modes_accepted() {
        for name in ["auto", "silent"] {
            let command = HardwareCommand::SetFanMode(fan(name));
            assert!(
                command
                    .validate(&SupportMode::Ready, &full_capabilities())
                    .is_ok()
            );
        }
    }

    #[test]
    fn unadvertised_valid_fan_mode_rejected() {
        let command = HardwareCommand::SetFanMode(fan("turbo-fan"));
        assert_eq!(
            command.validate(&SupportMode::Ready, &full_capabilities()),
            Err(CommandValidationError::FanModeNotAdvertised(fan(
                "turbo-fan"
            )))
        );
    }

    #[test]
    fn empty_fan_modes_rejects_command() {
        let mut capabilities = full_capabilities();
        capabilities.fan_modes.clear();
        let command = HardwareCommand::SetFanMode(fan("auto"));
        assert!(
            command
                .validate(&SupportMode::Ready, &capabilities)
                .is_err()
        );
    }

    #[test]
    fn advertised_shift_mode_accepted() {
        let command = HardwareCommand::SetShiftMode(shift("comfort"));
        assert!(
            command
                .validate(&SupportMode::Ready, &full_capabilities())
                .is_ok()
        );
    }

    #[test]
    fn unadvertised_valid_shift_mode_rejected() {
        let command = HardwareCommand::SetShiftMode(shift("turbo"));
        assert_eq!(
            command.validate(&SupportMode::Ready, &full_capabilities()),
            Err(CommandValidationError::ShiftModeNotAdvertised(shift(
                "turbo"
            )))
        );
    }

    #[test]
    fn empty_shift_modes_rejects_command() {
        let mut capabilities = full_capabilities();
        capabilities.shift_modes.clear();
        let command = HardwareCommand::SetShiftMode(shift("comfort"));
        assert!(
            command
                .validate(&SupportMode::Ready, &capabilities)
                .is_err()
        );
    }

    #[test]
    fn cooler_boost_capability_accepts_both_states() {
        for value in [true, false] {
            let command = HardwareCommand::SetCoolerBoost(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &full_capabilities())
                    .is_ok()
            );
        }
    }

    #[test]
    fn missing_cooler_boost_rejects_both_states() {
        let mut capabilities = full_capabilities();
        capabilities.cooler_boost = false;
        for value in [true, false] {
            let command = HardwareCommand::SetCoolerBoost(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &capabilities)
                    .is_err()
            );
        }
    }

    #[test]
    fn super_battery_capability_accepts_both_states() {
        for value in [true, false] {
            let command = HardwareCommand::SetSuperBattery(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &full_capabilities())
                    .is_ok()
            );
        }
    }

    #[test]
    fn missing_super_battery_rejects_both_states() {
        let mut capabilities = full_capabilities();
        capabilities.super_battery = false;
        for value in [true, false] {
            let command = HardwareCommand::SetSuperBattery(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &capabilities)
                    .is_err()
            );
        }
    }

    #[test]
    fn webcam_capability_accepts_both_states() {
        for value in [true, false] {
            let command = HardwareCommand::SetWebcam(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &full_capabilities())
                    .is_ok()
            );
        }
    }

    #[test]
    fn missing_webcam_rejects_both_states() {
        let mut capabilities = full_capabilities();
        capabilities.webcam = false;
        for value in [true, false] {
            let command = HardwareCommand::SetWebcam(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &capabilities)
                    .is_err()
            );
        }
    }

    #[test]
    fn webcam_block_capability_accepts_both_states() {
        for value in [true, false] {
            let command = HardwareCommand::SetWebcamBlock(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &full_capabilities())
                    .is_ok()
            );
        }
    }

    #[test]
    fn missing_webcam_block_rejects_both_states() {
        let mut capabilities = full_capabilities();
        capabilities.webcam_block = false;
        for value in [true, false] {
            let command = HardwareCommand::SetWebcamBlock(value);
            assert!(
                command
                    .validate(&SupportMode::Ready, &capabilities)
                    .is_err()
            );
        }
    }

    #[test]
    fn absent_backlight_capability_rejects() {
        let mut capabilities = full_capabilities();
        capabilities.keyboard_backlight = None;
        let command = HardwareCommand::SetKeyboardBacklight(0);
        assert!(
            command
                .validate(&SupportMode::Ready, &capabilities)
                .is_err()
        );
    }

    #[test]
    fn backlight_max_accepts_boundaries() {
        for level in [0, 3] {
            let command = HardwareCommand::SetKeyboardBacklight(level);
            assert!(
                command
                    .validate(&SupportMode::Ready, &full_capabilities())
                    .is_ok()
            );
        }
    }

    #[test]
    fn backlight_above_max_rejected() {
        let command = HardwareCommand::SetKeyboardBacklight(4);
        assert_eq!(
            command.validate(&SupportMode::Ready, &full_capabilities()),
            Err(CommandValidationError::BacklightAboveMaximum { level: 4, max: 3 })
        );
    }

    #[test]
    fn backlight_zero_max_accepts_only_zero() {
        let mut capabilities = full_capabilities();
        capabilities.keyboard_backlight = Some(BacklightCapability { max_brightness: 0 });
        assert!(
            HardwareCommand::SetKeyboardBacklight(0)
                .validate(&SupportMode::Ready, &capabilities)
                .is_ok()
        );
        assert!(
            HardwareCommand::SetKeyboardBacklight(1)
                .validate(&SupportMode::Ready, &capabilities)
                .is_err()
        );
    }

    #[test]
    fn supported_thresholds_accept_valid_threshold() {
        let command = HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap());
        assert!(
            command
                .validate(&SupportMode::Ready, &full_capabilities())
                .is_ok()
        );
    }

    #[test]
    fn unsupported_thresholds_reject_valid_threshold() {
        let mut capabilities = full_capabilities();
        capabilities.battery_thresholds = false;
        let command = HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap());
        assert!(
            command
                .validate(&SupportMode::Ready, &capabilities)
                .is_err()
        );
    }

    #[test]
    fn validation_mutates_neither_input() {
        let mode = SupportMode::Ready;
        let capabilities = full_capabilities();
        let command = HardwareCommand::SetFanMode(fan("auto"));
        let _ = command.validate(&mode, &capabilities);
        assert_eq!(mode, SupportMode::Ready);
        assert_eq!(capabilities, full_capabilities());
    }

    #[test]
    fn command_equality_and_clone_work() {
        let command = HardwareCommand::SetShiftMode(shift("comfort"));
        assert_eq!(command, command.clone());
        assert_ne!(command, HardwareCommand::SetShiftMode(shift("sport")));
    }

    #[test]
    fn validation_needs_no_backend() {
        // Compile-level proof: validate() takes only mode and capabilities,
        // so this test constructs no backend of any kind.
        let command = HardwareCommand::SetWebcam(true);
        assert!(
            command
                .validate(&SupportMode::Ready, &full_capabilities())
                .is_ok()
        );
    }

    #[test]
    fn error_display_is_human_readable() {
        assert!(!format!("{}", CommandValidationError::ReadOnly).is_empty());
        assert!(!format!("{}", CommandValidationError::FanModeNotAdvertised(fan("x"))).is_empty());
    }
}
