//! Read-only performance screen: current modes plus capability metadata.
//!
//! Current values come from [`LiveHardware::current_snapshot`] only.
//! Available modes come from injected startup [`Capabilities`]. No
//! selectors, no editing, no hardware transport here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, joined_modes, performance_lines, render_panel, render_screen_shell,
    support_text,
};

/// Renders current performance state plus available modes and feature
/// support. Read-only: no selectors, no editing.
pub fn render_performance<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    render_performance_with_theme(frame, area, live, capabilities, &Theme::default());
}

/// Theme-aware performance renderer behind the Task-5 API.
pub(crate) fn render_performance_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Performance", live, theme);
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        performance_lines(live.current_snapshot()),
        theme,
    );
    render_panel(
        frame,
        panels[1],
        " CAPABILITIES ",
        capability_lines(capabilities, theme),
        theme,
    );
}

fn capability_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
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
            render_performance(frame, frame.area(), &live, &capabilities);
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
            render_performance(frame, frame.area(), &live, &capabilities);
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
            render_performance(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Turbo_PLUS"));
        assert!(text.contains("Whisper 2.0"));
    }
}
