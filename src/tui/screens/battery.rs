//! Read-only battery screen: current battery state plus threshold
//! capability metadata.
//!
//! Only battery threshold control carries an explicit capability label:
//! absent charge/state/AC values render `N/A`, never "Unavailable". Control
//! rows are selectable drafts; Task 4 never executes.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::controls::control_row_lines;
use crate::tui::editing::ControlState;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    battery_lines, capability_style, render_panel, render_screen_shell, support_text,
};

/// Renders current battery state plus threshold-control capability with a
/// selectable control row. Only threshold control carries a capability
/// label; absent runtime values are `N/A`, never "Unavailable".
pub fn render_battery<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
) {
    render_battery_with_theme(frame, area, live, capabilities, controls, &Theme::default());
}

/// Theme-aware battery renderer behind the Task-5 API.
pub(crate) fn render_battery_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Battery", live, theme);
    let control_rows = control_row_lines(
        crate::app::Screen::Battery,
        live.current_snapshot(),
        capabilities,
        live.mode(),
        controls,
        theme,
    );
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(control_rows.len() as u16 + 2),
            Constraint::Min(0),
        ])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        battery_lines(live.current_snapshot()),
        theme,
    );
    render_panel(frame, panels[1], " CONTROLS ", control_rows, theme);
    render_panel(
        frame,
        panels[2],
        " CAPABILITIES ",
        vec![threshold_capability_line(capabilities, theme)],
        theme,
    );
}

/// Single threshold-control capability row, shared with compact tiers.
pub(crate) fn threshold_capability_line(
    capabilities: &Capabilities,
    theme: &Theme,
) -> Line<'static> {
    Line::styled(
        format!(
            "Threshold Control: {}",
            support_text(capabilities.battery_thresholds)
        ),
        capability_style(capabilities.battery_thresholds, theme),
    )
}

#[cfg(test)]
mod tests {
    use crate::hardware::{HardwareSnapshot, SupportMode};

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::render_battery;

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(100, 30, |frame| {
            render_battery(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        })
    }

    #[test]
    fn renders_charge_percentage() {
        assert!(text().contains("Charge: 77%"));
    }

    #[test]
    fn renders_battery_state() {
        assert!(text().contains("State: Charging"));
    }

    #[test]
    fn renders_ac_status() {
        assert!(text().contains("AC: Connected"));
    }

    #[test]
    fn renders_start_threshold() {
        assert!(text().contains("Start Threshold: 50%"));
    }

    #[test]
    fn renders_end_threshold() {
        assert!(text().contains("End Threshold: 80%"));
    }

    #[test]
    fn threshold_capability_supported() {
        assert!(text().contains("Threshold Control: Supported"));
    }

    #[test]
    fn threshold_capability_unavailable() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut capabilities = full_capabilities();
        capabilities.battery_thresholds = false;
        let text = screen_text(100, 30, |frame| {
            render_battery(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("Threshold Control: Unavailable"));
    }

    #[test]
    fn absent_values_render_na_not_unavailable() {
        let (live, _) = live_for(vec![Ok(HardwareSnapshot::default())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_battery(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("Charge: N/A"));
        assert!(text.contains("State: N/A"));
        assert!(text.contains("AC: N/A"));
        assert!(!text.contains("Charge: Unavailable"));
        assert!(!text.contains("State: Unavailable"));
        assert!(!text.contains("AC: Unavailable"));
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
            render_battery(frame, frame.area(), &live, &capabilities, controls);
        })
    }

    #[test]
    fn controls_panel_renders_battery_limit() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("CONTROLS"));
        assert!(text.contains("Battery Limit"));
    }

    #[test]
    fn read_only_battery_shows_disabled() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
        );
        assert!(text.contains("Disabled (read-only)"));
    }

    #[test]
    fn unsupported_threshold_shows_unsupported() {
        let mut caps = full_capabilities();
        caps.battery_thresholds = false;
        let text = text_with_controls(
            &caps,
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("Unsupported"));
    }

    #[test]
    fn battery_pending_is_ten_point_pair() {
        use crate::tui::editing::ControlState;
        use crate::tui::screens::support::healthy_snapshot;
        let mut controls = ControlState::default();
        assert!(controls.begin_edit(
            crate::app::Screen::Battery,
            Some(&healthy_snapshot()),
            &full_capabilities(),
            &SupportMode::Ready,
        ));
        assert!(controls.confirm(&SupportMode::Ready, &full_capabilities()));
        let pending = controls.pending().expect("pending stored");
        match pending {
            crate::hardware::HardwareCommand::SetBatteryThreshold(threshold) => {
                assert!(
                    [10, 20, 30, 40, 50, 60, 70, 80, 90, 100].contains(&threshold.end_percent())
                );
                assert_eq!(threshold.end_percent(), threshold.start_percent() + 10);
            }
            other => panic!("unexpected pending {other:?}"),
        }
        let text = text_with_controls(&full_capabilities(), &controls, SupportMode::Ready);
        assert!(text.contains("Pending confirmation"));
        assert!(text.contains("NOT applied yet"));
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
                render_battery(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &live,
                    &caps,
                    &crate::tui::editing::ControlState::default(),
                );
            })
            .expect("zero-area battery draws");
    }
}
