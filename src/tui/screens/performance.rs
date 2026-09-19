//! Read-only performance screen: current modes plus capability metadata.
//!
//! Current values come from [`LiveHardware::current_snapshot`] only.
//! Available modes come from injected startup [`Capabilities`]. Control
//! rows are selectable drafts; Task 4 creates pending data only and never
//! executes hardware writes.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::controls::control_row_lines;
use crate::tui::editing::ControlState;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, joined_modes, performance_lines, render_panel, render_screen_shell,
    support_text,
};

/// Renders current performance state plus available modes and feature
/// support with selectable control rows. Drafts are data only.
pub fn render_performance<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
) {
    render_performance_with_theme(frame, area, live, capabilities, controls, &Theme::default());
}

/// Theme-aware performance renderer behind the Task-5 API.
pub(crate) fn render_performance_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Performance", live, theme);
    let control_rows = control_row_lines(
        crate::app::Screen::Performance,
        live.current_snapshot(),
        capabilities,
        live.mode(),
        controls,
        theme,
    );
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(control_rows.len() as u16 + 2),
            Constraint::Min(0),
        ])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        performance_lines(live.current_snapshot()),
        theme,
    );
    render_panel(frame, panels[1], " CONTROLS ", control_rows, theme);
    render_panel(
        frame,
        panels[2],
        " CAPABILITIES ",
        capability_lines(capabilities, theme),
        theme,
    );
}

pub(crate) fn capability_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::from(format!(
            "Available Shift Modes: {}",
            joined_modes(capabilities.shift_modes.iter().map(|mode| mode.as_str()))
        )),
        Line::from(format!(
            "Available Fan Modes: {}",
            joined_modes(capabilities.fan_modes.iter().map(|mode| mode.as_str()))
        )),
        Line::styled(
            format!("Cooler Boost: {}", support_text(capabilities.cooler_boost)),
            capability_style(capabilities.cooler_boost, theme),
        ),
        Line::styled(
            format!(
                "Super Battery: {}",
                support_text(capabilities.super_battery)
            ),
            capability_style(capabilities.super_battery, theme),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::hardware::{FanMode, ShiftMode, SupportMode};

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::render_performance;

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(100, 30, |frame| {
            render_performance(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        })
    }

    #[test]
    fn renders_current_shift_mode() {
        assert!(text().contains("Shift Mode: comfort"));
    }

    #[test]
    fn renders_current_fan_mode() {
        assert!(text().contains("Fan Mode: auto"));
    }

    #[test]
    fn renders_cooler_boost_current_state() {
        assert!(text().contains("Cooler Boost: Off"));
    }

    #[test]
    fn renders_super_battery_current_state() {
        assert!(text().contains("Super Battery: Off"));
    }

    #[test]
    fn renders_available_shift_modes() {
        assert!(text().contains("Available Shift Modes: comfort, sport"));
    }

    #[test]
    fn preserves_shift_mode_upstream_order() {
        let text = text();
        let comfort = text.find("comfort").expect("shift mode present");
        let sport = text.find("sport").expect("shift mode present");
        assert!(comfort < sport);
    }

    #[test]
    fn renders_available_fan_modes() {
        assert!(text().contains("Available Fan Modes: auto, silent, future-mode"));
    }

    #[test]
    fn preserves_fan_mode_upstream_order() {
        let text = text();
        let auto = text.find("auto").expect("fan mode present");
        let silent = text.find("silent").expect("fan mode present");
        let future = text.find("future-mode").expect("fan mode present");
        assert!(auto < silent && silent < future);
    }

    #[test]
    fn empty_mode_lists_render_honestly() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut capabilities = full_capabilities();
        capabilities.fan_modes = Vec::new();
        capabilities.shift_modes = Vec::new();
        let text = screen_text(100, 30, |frame| {
            render_performance(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("Available Shift Modes: None reported"));
        assert!(text.contains("Available Fan Modes: None reported"));
    }

    #[test]
    fn cooler_boost_capability_supported() {
        assert!(text().contains("Cooler Boost: Supported"));
    }

    #[test]
    fn super_battery_capability_unavailable() {
        assert!(text().contains("Super Battery: Unavailable"));
    }

    #[test]
    fn unknown_future_modes_render_verbatim() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut capabilities = full_capabilities();
        capabilities.shift_modes = vec![ShiftMode::try_from("Turbo_PLUS").unwrap()];
        capabilities.fan_modes = vec![FanMode::try_from("Whisper 2.0").unwrap()];
        let text = screen_text(100, 30, |frame| {
            render_performance(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("Turbo_PLUS"));
        assert!(text.contains("Whisper 2.0"));
    }

    fn text_with_controls(
        capabilities: &crate::hardware::Capabilities,
        controls: &crate::tui::editing::ControlState,
        mode: SupportMode,
    ) -> String {
        use crate::tui::screens::support::{healthy_snapshot, live_for};
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], mode, 1);
        let capabilities = capabilities.clone();
        screen_text(100, 30, |frame| {
            render_performance(frame, frame.area(), &live, &capabilities, controls);
        })
    }

    #[test]
    fn controls_panel_renders_with_selection() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("CONTROLS"));
        assert!(text.contains("> Shift Mode"));
    }

    #[test]
    fn read_only_controls_show_disabled() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
        );
        assert!(text.contains("Disabled (read-only)"));
    }

    #[test]
    fn unsupported_shift_shows_unsupported() {
        let mut caps = full_capabilities();
        caps.shift_modes.clear();
        let text = text_with_controls(
            &caps,
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("Unsupported"));
    }

    #[test]
    fn missing_telemetry_shows_not_editable() {
        use crate::hardware::HardwareSnapshot;
        let (live, _) = live_for(vec![Ok(HardwareSnapshot::default())], SupportMode::Ready, 1);
        let caps = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_performance(
                frame,
                frame.area(),
                &live,
                &caps,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("Not currently editable"));
    }

    #[test]
    fn editing_and_pending_render_without_success_claims() {
        use crate::tui::editing::ControlState;
        use crate::tui::screens::support::healthy_snapshot;
        let mut controls = ControlState::default();
        assert!(controls.begin_edit(
            crate::app::Screen::Performance,
            Some(&healthy_snapshot()),
            &full_capabilities(),
            &SupportMode::Ready,
        ));
        let editing = text_with_controls(&full_capabilities(), &controls, SupportMode::Ready);
        assert!(editing.contains("Editing:"));
        assert!(!editing.contains("Applied "));
        assert!(controls.confirm(&SupportMode::Ready, &full_capabilities()));
        let pending = text_with_controls(&full_capabilities(), &controls, SupportMode::Ready);
        assert!(pending.contains("Pending confirmation"));
        assert!(pending.contains("NOT applied yet"));
        assert!(!pending.contains("Applied Fan"));
    }

    #[test]
    fn zero_area_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let caps = full_capabilities();
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_performance(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &live,
                    &caps,
                    &crate::tui::editing::ControlState::default(),
                );
            })
            .expect("zero-area performance draws");
    }
}
