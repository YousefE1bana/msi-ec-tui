//! Shared TUI presentation helpers for typed hardware commands.
//!
//! Pure text mapping only: [`command_text`] names the requested setting,
//! [`current_text`] names the already-sampled snapshot value for the same
//! setting. No validation, no policy, no I/O, no execution. Values are
//! never RPM; fan data stays percentage/raw.

use crate::hardware::{HardwareCommand, HardwareSnapshot};

/// Human-readable name of the requested setting, without values that
/// need snapshot context.
pub(crate) fn command_text(command: &HardwareCommand) -> String {
    match command {
        HardwareCommand::SetFanMode(mode) => format!("Fan Mode: {}", mode.as_str()),
        HardwareCommand::SetShiftMode(mode) => format!("Shift Mode: {}", mode.as_str()),
        HardwareCommand::SetCoolerBoost(value) => format!("Cooler Boost: {}", on_off(*value)),
        HardwareCommand::SetSuperBattery(value) => format!("Super Battery: {}", on_off(*value)),
        HardwareCommand::SetWebcam(value) => format!("Webcam: {}", on_off(*value)),
        HardwareCommand::SetWebcamBlock(value) => format!("Webcam Block: {}", on_off(*value)),
        HardwareCommand::SetKeyboardBacklight(level) => {
            format!("Keyboard Backlight: {level}")
        }
        HardwareCommand::SetBatteryThreshold(threshold) => {
            format!("Battery Limit: {}%", threshold.end_percent())
        }
    }
}

/// Human-readable current snapshot value for the same setting, or
/// `"unknown"` when telemetry is absent. Never invents state.
pub(crate) fn current_text(
    command: &HardwareCommand,
    snapshot: Option<&HardwareSnapshot>,
) -> String {
    let Some(snapshot) = snapshot else {
        return "unknown".to_owned();
    };
    match command {
        HardwareCommand::SetFanMode(_) => snapshot
            .fan_mode
            .as_ref()
            .map(|mode| mode.as_str().to_owned())
            .unwrap_or_else(|| "unknown".to_owned()),
        HardwareCommand::SetShiftMode(_) => snapshot
            .shift_mode
            .as_ref()
            .map(|mode| mode.as_str().to_owned())
            .unwrap_or_else(|| "unknown".to_owned()),
        HardwareCommand::SetCoolerBoost(_) => on_off_opt(snapshot.cooler_boost).to_owned(),
        HardwareCommand::SetSuperBattery(_) => on_off_opt(snapshot.super_battery).to_owned(),
        HardwareCommand::SetWebcam(_) => on_off_opt(snapshot.webcam).to_owned(),
        HardwareCommand::SetWebcamBlock(_) => on_off_opt(snapshot.webcam_block).to_owned(),
        HardwareCommand::SetKeyboardBacklight(_) => snapshot
            .keyboard_backlight
            .map(|level| level.to_string())
            .unwrap_or_else(|| "unknown".to_owned()),
        HardwareCommand::SetBatteryThreshold(_) => {
            match (
                snapshot.battery_start_threshold,
                snapshot.battery_end_threshold,
            ) {
                (Some(start), Some(end)) => format!("{start}%->{end}%"),
                _ => "unknown".to_owned(),
            }
        }
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "On" } else { "Off" }
}

fn on_off_opt(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "On",
        Some(false) => "Off",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use crate::hardware::{BatteryThreshold, FanMode, ShiftMode};

    use super::*;

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    fn threshold() -> BatteryThreshold {
        BatteryThreshold::new(70, 80).unwrap()
    }

    fn snapshot() -> HardwareSnapshot {
        crate::tui::screens::support::healthy_snapshot()
    }

    #[test]
    fn command_text_names_modes_verbatim() {
        assert_eq!(
            command_text(&HardwareCommand::SetFanMode(fan("future-mode"))),
            "Fan Mode: future-mode"
        );
        assert_eq!(
            command_text(&HardwareCommand::SetShiftMode(shift("sport"))),
            "Shift Mode: sport"
        );
    }

    #[test]
    fn command_text_names_booleans() {
        assert_eq!(
            command_text(&HardwareCommand::SetCoolerBoost(true)),
            "Cooler Boost: On"
        );
        assert_eq!(
            command_text(&HardwareCommand::SetSuperBattery(false)),
            "Super Battery: Off"
        );
    }

    #[test]
    fn command_text_names_backlight_and_battery() {
        assert_eq!(
            command_text(&HardwareCommand::SetKeyboardBacklight(2)),
            "Keyboard Backlight: 2"
        );
        assert_eq!(
            command_text(&HardwareCommand::SetBatteryThreshold(threshold())),
            "Battery Limit: 80%"
        );
    }

    #[test]
    fn command_text_never_mentions_rpm() {
        for command in [
            HardwareCommand::SetFanMode(fan("auto")),
            HardwareCommand::SetCoolerBoost(true),
        ] {
            assert!(!command_text(&command).contains("RPM"));
        }
    }

    #[test]
    fn current_text_reports_snapshot_values() {
        let snapshot = snapshot();
        assert_eq!(
            current_text(&HardwareCommand::SetFanMode(fan("silent")), Some(&snapshot)),
            "auto"
        );
        assert_eq!(
            current_text(&HardwareCommand::SetCoolerBoost(true), Some(&snapshot)),
            "Off"
        );
        assert_eq!(
            current_text(&HardwareCommand::SetKeyboardBacklight(2), Some(&snapshot)),
            "2"
        );
        assert_eq!(
            current_text(
                &HardwareCommand::SetBatteryThreshold(threshold()),
                Some(&snapshot)
            ),
            "50%->80%"
        );
    }

    #[test]
    fn current_text_reports_unknown_without_invention() {
        for command in [
            HardwareCommand::SetFanMode(fan("auto")),
            HardwareCommand::SetShiftMode(shift("comfort")),
            HardwareCommand::SetCoolerBoost(true),
            HardwareCommand::SetSuperBattery(false),
            HardwareCommand::SetWebcam(true),
            HardwareCommand::SetWebcamBlock(false),
            HardwareCommand::SetKeyboardBacklight(2),
            HardwareCommand::SetBatteryThreshold(threshold()),
        ] {
            assert_eq!(current_text(&command, None), "unknown");
        }
        let empty = HardwareSnapshot::default();
        assert_eq!(
            current_text(&HardwareCommand::SetFanMode(fan("auto")), Some(&empty)),
            "unknown"
        );
    }
}
