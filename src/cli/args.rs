//! Command-line argument model for the `mec` executable.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use thiserror::Error;

use crate::hardware::{
    BatteryThreshold, BatteryThresholdError, FanMode, ModeValidationError, ShiftMode,
};
use crate::monitoring::PollInterval;

/// MEC command-line interface.
#[derive(Debug, Parser)]
#[command(name = "mec", about = "MSI EC Control Center for Linux")]
pub struct Cli {
    /// Alternate system root for hardware inspection (tests and fixtures).
    #[arg(long, global = true, value_name = "PATH", default_value = "/")]
    pub sys_root: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available `mec` subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect hardware compatibility and report diagnostics.
    Doctor,
    /// Show hardware state.
    Status {
        /// Emit machine-readable JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Continuously monitor hardware state until Ctrl+C.
    Monitor {
        /// Poll interval between samples.
        #[arg(long, default_value_t = PollInterval::default())]
        interval: PollInterval,
    },
    /// Set the fan mode to an advertised value.
    Fan {
        #[command(subcommand)]
        command: FanCommand,
    },
    /// Set the shift mode to an advertised value.
    Shift {
        /// Desired shift mode.
        #[arg(value_parser = parse_shift_mode, value_name = "MODE")]
        mode: ShiftMode,
    },
    /// Turn cooler boost on or off.
    CoolerBoost {
        /// Desired state.
        state: OnOff,
    },
    /// Turn super battery mode on or off.
    SuperBattery {
        /// Desired state.
        state: OnOff,
    },
    /// Turn the webcam on or off.
    Webcam {
        /// Desired state.
        state: OnOff,
    },
    /// Turn webcam blocking on or off.
    WebcamBlock {
        /// Desired state.
        state: OnOff,
    },
    /// Set the keyboard backlight level.
    KeyboardBacklight {
        /// Desired brightness level.
        level: u8,
    },
    /// Set the battery charge limit.
    Battery {
        #[command(subcommand)]
        command: BatteryCommand,
    },
}

/// `mec fan` subcommands.
#[derive(Debug, Subcommand)]
pub enum FanCommand {
    /// Set the fan mode.
    Mode {
        /// Desired fan mode.
        #[arg(value_parser = parse_fan_mode, value_name = "MODE")]
        mode: FanMode,
    },
}

/// `mec battery` subcommands.
#[derive(Debug, Subcommand)]
pub enum BatteryCommand {
    /// Set the battery charge limit (end percent).
    Limit {
        /// Charge limit end percent (10-100); start derives as end - 10.
        #[arg(value_parser = parse_battery_limit, value_name = "END_PERCENT")]
        threshold: BatteryThreshold,
    },
}

/// Exact on/off CLI vocabulary for boolean controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OnOff {
    /// Enable the control.
    On,
    /// Disable the control.
    Off,
}

impl OnOff {
    /// Desired boolean state.
    pub fn as_bool(self) -> bool {
        matches!(self, Self::On)
    }

    /// CLI spelling, reused in success output.
    pub fn as_str(self) -> &'static str {
        if self.as_bool() { "on" } else { "off" }
    }
}

/// Why a battery limit argument was rejected before any execution attempt.
#[derive(Debug, Error)]
pub enum BatteryLimitParseError {
    /// The argument is not an integer.
    #[error("battery limit must be an integer: {0}")]
    NotAnInteger(#[from] std::num::ParseIntError),
    /// The integer is outside the representable `msi-ec` range.
    #[error("{0}")]
    InvalidThreshold(#[from] BatteryThresholdError),
}

/// Parses a fan mode with the domain syntax rules. Syntactically valid
/// future modes pass through; the capability validator decides support.
fn parse_fan_mode(value: &str) -> Result<FanMode, ModeValidationError> {
    FanMode::try_from(value)
}

/// Parses a shift mode with the domain syntax rules.
fn parse_shift_mode(value: &str) -> Result<ShiftMode, ModeValidationError> {
    ShiftMode::try_from(value)
}

/// Parses a battery charge limit into its validated threshold pair.
fn parse_battery_limit(value: &str) -> Result<BatteryThreshold, BatteryLimitParseError> {
    let end: u8 = value.parse()?;
    BatteryThreshold::from_end_percent(end).map_err(BatteryLimitParseError::from)
}
