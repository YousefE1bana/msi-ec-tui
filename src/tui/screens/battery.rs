//! Read-only battery screen: current battery state plus threshold
//! capability metadata.
//!
//! Only battery threshold control carries an explicit capability label:
//! absent charge/state/AC values render `N/A`, never "Unavailable". No
//! presets, no editing, no hardware transport here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::ui::{battery_lines, render_panel, render_screen_shell, support_text};

/// Renders current battery state plus threshold-control capability. Only
/// threshold control carries a capability label; absent runtime values are
/// `N/A`, never "Unavailable".
pub fn render_battery<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    let content = render_screen_shell(frame, area, "Battery", live);
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        battery_lines(live.current_snapshot()),
    );
    render_panel(
        frame,
        panels[1],
        " CAPABILITIES ",
        vec![Line::from(format!(
            "Threshold Control: {}",
            support_text(capabilities.battery_thresholds)
        ))],
    );
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
            render_battery(frame, frame.area(), &live, &capabilities);
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
            render_battery(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Threshold Control: Unavailable"));
    }

    #[test]
    fn absent_values_render_na_not_unavailable() {
        let (live, _) = live_for(vec![Ok(HardwareSnapshot::default())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_battery(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Charge: N/A"));
        assert!(text.contains("State: N/A"));
        assert!(text.contains("AC: N/A"));
        assert!(!text.contains("Charge: Unavailable"));
        assert!(!text.contains("State: Unavailable"));
        assert!(!text.contains("AC: Unavailable"));
    }
}
