//! Shared TUI presentation helpers for typed hardware commands.
//!
//! Pure text mapping only: [`command_text`] names the requested setting,
//! [`current_text`] names the already-sampled snapshot value for the same
//! setting. No validation, no policy, no I/O, no execution. Values are
//! never RPM; fan data stays percentage/raw.
//!
//! [`control_row_lines`] renders selectable control rows for the four
//! interactive screens from already-sampled data plus [`ControlState`].
//! Fan Mode / Cooler Boost share this helper across Performance and Fans
//! so duplicates produce identical text and commands.

use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::app::Screen;
use crate::hardware::{Capabilities, HardwareCommand, HardwareSnapshot, SupportMode};

use super::editing::{ControlId, ControlState, control_rows, editability};
use super::theme::Theme;
use super::ui::{capability_style, support_text};

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

/// Display name for one selectable control row.
pub(crate) fn control_display_name(control: ControlId) -> &'static str {
    match control {
        ControlId::ShiftMode => "Shift Mode",
        ControlId::FanMode => "Fan Mode",
        ControlId::CoolerBoost => "Cooler Boost",
        ControlId::SuperBattery => "Super Battery",
        ControlId::BatteryThreshold => "Battery Limit",
        ControlId::Webcam => "Webcam",
        ControlId::WebcamBlock => "Webcam Block",
        ControlId::KeyboardBacklight => "Keyboard Backlight",
        ControlId::FnKeyInfo => "Fn Key",
        ControlId::WinKeyInfo => "Win Key",
    }
}

/// Maps a typed command back to its control row.
pub(crate) fn control_for_command(command: &HardwareCommand) -> ControlId {
    match command {
        HardwareCommand::SetShiftMode(_) => ControlId::ShiftMode,
        HardwareCommand::SetFanMode(_) => ControlId::FanMode,
        HardwareCommand::SetCoolerBoost(_) => ControlId::CoolerBoost,
        HardwareCommand::SetSuperBattery(_) => ControlId::SuperBattery,
        HardwareCommand::SetBatteryThreshold(_) => ControlId::BatteryThreshold,
        HardwareCommand::SetWebcam(_) => ControlId::Webcam,
        HardwareCommand::SetWebcamBlock(_) => ControlId::WebcamBlock,
        HardwareCommand::SetKeyboardBacklight(_) => ControlId::KeyboardBacklight,
    }
}

/// Representative command for reading the current snapshot value of one
/// control. The value inside is ignored by [`current_text`]; the variant
/// selects which snapshot field to display.
fn representative_command(control: ControlId) -> Option<HardwareCommand> {
    match control {
        ControlId::ShiftMode => crate::hardware::ShiftMode::try_from("current")
            .ok()
            .map(HardwareCommand::SetShiftMode),
        ControlId::FanMode => crate::hardware::FanMode::try_from("current")
            .ok()
            .map(HardwareCommand::SetFanMode),
        ControlId::CoolerBoost => Some(HardwareCommand::SetCoolerBoost(false)),
        ControlId::SuperBattery => Some(HardwareCommand::SetSuperBattery(false)),
        ControlId::BatteryThreshold => crate::hardware::BatteryThreshold::from_end_percent(10)
            .ok()
            .map(HardwareCommand::SetBatteryThreshold),
        ControlId::Webcam => Some(HardwareCommand::SetWebcam(false)),
        ControlId::WebcamBlock => Some(HardwareCommand::SetWebcamBlock(false)),
        ControlId::KeyboardBacklight => Some(HardwareCommand::SetKeyboardBacklight(0)),
        ControlId::FnKeyInfo | ControlId::WinKeyInfo => None,
    }
}

/// Selectable rows for one interactive screen with selection, editing,
/// pending, and disabled states. Shared by Performance and Fans so
/// duplicated Fan Mode / Cooler Boost rows stay identical.
#[allow(clippy::too_many_arguments)]
pub(crate) fn control_row_lines(
    screen: Screen,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
    mode: &SupportMode,
    controls: &ControlState,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let selected = controls.selected(screen);
    control_rows(screen)
        .iter()
        .map(|control| {
            control_row_line(
                *control,
                Some(*control) == selected,
                snapshot,
                capabilities,
                mode,
                controls,
                theme,
            )
        })
        .collect()
}

fn control_row_line(
    control: ControlId,
    is_selected: bool,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
    mode: &SupportMode,
    controls: &ControlState,
    theme: &Theme,
) -> Line<'static> {
    // Informational Fn/Win rows: capability existence only, never a command.
    if matches!(control, ControlId::FnKeyInfo | ControlId::WinKeyInfo) {
        let supported = match control {
            ControlId::FnKeyInfo => capabilities.fn_key,
            _ => capabilities.win_key,
        };
        let text = format!(
            "{}: {} (informational only)",
            control_display_name(control),
            support_text(supported)
        );
        return styled_control_row(is_selected, text, Style::default(), theme);
    }
    let current = representative_command(control)
        .map(|command| current_text(&command, snapshot))
        .unwrap_or_else(|| "unknown".to_owned());
    let mut text = format!("{}: current {}", control_display_name(control), current);
    let state = editability(control, snapshot, capabilities, mode);
    if state != crate::tui::editing::Editability::Editable {
        text.push_str(&format!(" — {}", state.reason()));
    }
    if let Some(editor) = controls.editor()
        && editor.control() == control
    {
        text.push_str(&format!(" — Editing: {}", command_text(editor.draft())));
        if let Some(reason) = controls.error() {
            text.push_str(&format!(" — {reason}"));
        }
    }
    if let Some(pending) = controls.pending()
        && control_for_command(pending) == control
    {
        text.push_str(&format!(
            " — Pending confirmation: {} (NOT applied yet)",
            command_text(pending)
        ));
    }
    let plain = match state {
        crate::tui::editing::Editability::Editable => Style::default(),
        _ => Style::default().fg(theme.muted),
    };
    // Preserve capability meaning for supported rows while keeping the
    // selection marker semantic.
    let _ = capability_style(true, theme);
    styled_control_row(is_selected, text, plain, theme)
}

fn styled_control_row(selected: bool, text: String, plain: Style, theme: &Theme) -> Line<'static> {
    if selected {
        Line::styled(
            format!("> {text}"),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Line::styled(format!("  {text}"), plain)
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
    // ---- Task 4: row rendering ----

    fn caps() -> Capabilities {
        crate::tui::screens::support::full_capabilities()
    }

    fn row_text(
        screen: Screen,
        controls: &crate::tui::editing::ControlState,
        capabilities: &Capabilities,
        mode: &SupportMode,
    ) -> String {
        let snapshot = Some(snapshot());
        control_row_lines(
            screen,
            snapshot.as_ref(),
            capabilities,
            mode,
            controls,
            &crate::tui::theme::Theme::default(),
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn selected_row_uses_marker() {
        let controls = crate::tui::editing::ControlState::default();
        let text = row_text(Screen::Performance, &controls, &caps(), &SupportMode::Ready);
        assert!(text.contains("> Shift Mode"));
        assert!(!text.contains("> Fan Mode"));
    }

    #[test]
    fn unsupported_row_shows_reason() {
        let mut unsupported = caps();
        unsupported.cooler_boost = false;
        let controls = crate::tui::editing::ControlState::default();
        let text = row_text(
            Screen::Performance,
            &controls,
            &unsupported,
            &SupportMode::Ready,
        );
        assert!(text.contains("Cooler Boost"));
        assert!(text.contains("Unsupported"));
    }

    #[test]
    fn read_only_rows_show_disabled() {
        let mode = SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable);
        let controls = crate::tui::editing::ControlState::default();
        let text = row_text(Screen::Performance, &controls, &caps(), &mode);
        assert!(text.contains("Disabled (read-only)"));
    }

    #[test]
    fn missing_telemetry_shows_not_editable() {
        use crate::tui::editing::{ControlId, Editability, editability};
        let empty = HardwareSnapshot::default();
        assert_eq!(
            editability(
                ControlId::FanMode,
                Some(&empty),
                &caps(),
                &SupportMode::Ready
            ),
            Editability::UnknownCurrent
        );
        let controls = crate::tui::editing::ControlState::default();
        let lines = control_row_lines(
            Screen::Fans,
            Some(&empty),
            &caps(),
            &SupportMode::Ready,
            &controls,
            &crate::tui::theme::Theme::default(),
        );
        let text = lines
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Not currently editable"));
    }

    #[test]
    fn fn_win_rows_are_informational_only() {
        let controls = crate::tui::editing::ControlState::default();
        let text = row_text(Screen::Devices, &controls, &caps(), &SupportMode::Ready);
        assert!(text.contains("Fn Key"));
        assert!(text.contains("informational only"));
        assert!(text.contains("Win Key"));
    }

    #[test]
    fn editing_row_shows_draft() {
        use crate::tui::editing::ControlState;
        let mut controls = ControlState::default();
        assert!(controls.begin_edit(
            Screen::Fans,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready
        ));
        let text = row_text(Screen::Fans, &controls, &caps(), &SupportMode::Ready);
        assert!(text.contains("Editing:"));
        assert!(text.contains("Fan Mode:"));
    }

    #[test]
    fn pending_row_states_not_applied_yet() {
        use crate::tui::editing::ControlState;
        let mut controls = ControlState::default();
        assert!(controls.begin_edit(
            Screen::Fans,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready
        ));
        assert!(controls.confirm(&SupportMode::Ready, &caps()));
        let text = row_text(Screen::Fans, &controls, &caps(), &SupportMode::Ready);
        assert!(text.contains("Pending confirmation"));
        assert!(text.contains("NOT applied yet"));
        assert!(!text.contains("Applied "));
    }

    #[test]
    fn duplicate_fan_rows_share_text_shape() {
        let controls = crate::tui::editing::ControlState::default();
        let perf = row_text(Screen::Performance, &controls, &caps(), &SupportMode::Ready);
        let fans = row_text(Screen::Fans, &controls, &caps(), &SupportMode::Ready);
        assert!(perf.contains("Fan Mode: current auto"));
        assert!(fans.contains("Fan Mode: current auto"));
    }

    #[test]
    fn rows_never_mention_rpm() {
        let controls = crate::tui::editing::ControlState::default();
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
        ] {
            let text = row_text(screen, &controls, &caps(), &SupportMode::Ready);
            assert!(!text.contains("RPM"), "{screen:?}");
        }
    }
}
