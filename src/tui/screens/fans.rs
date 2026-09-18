//! Read-only fans screen: current fan telemetry plus capability metadata.
//!
//! Fan readings are percentage-style values, never RPM. No editing, no fan
//! curves, no graphs, no hardware transport here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, EcBackend, HardwareSnapshot};

use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, fan_mode_text, fan_text, joined_modes, on_off_text, render_panel,
    render_screen_shell, support_text,
};

/// Renders current fan telemetry plus fan capability metadata. Fan readings
/// stay percentage-style; no curves, graphs, or editing.
pub fn render_fans<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    render_fans_with_theme(frame, area, live, capabilities, &Theme::default());
}

/// Theme-aware fans renderer behind the Task-5 API.
pub(crate) fn render_fans_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Fans", live, theme);
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        current_lines(live.current_snapshot()),
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

fn current_lines(snapshot: Option<&HardwareSnapshot>) -> Vec<Line<'static>> {
    let cpu = snapshot.and_then(|state| state.cpu_fan);
    let gpu = snapshot.and_then(|state| state.gpu_fan);
    let mode = snapshot.and_then(|state| state.fan_mode.as_ref());
    let cooler = snapshot.and_then(|state| state.cooler_boost);
    vec![
        Line::from(format!("CPU Fan: {}", fan_text(cpu))),
        Line::from(format!("GPU Fan: {}", fan_text(gpu))),
        Line::from(format!("Fan Mode: {}", fan_mode_text(mode))),
        Line::from(format!("Cooler Boost: {}", on_off_text(cooler))),
    ]
}

fn capability_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            format!("CPU Fan Telemetry: {}", support_text(capabilities.cpu_fan)),
            capability_style(capabilities.cpu_fan, theme),
        ),
        Line::styled(
            format!("GPU Fan Telemetry: {}", support_text(capabilities.gpu_fan)),
            capability_style(capabilities.gpu_fan, theme),
        ),
        Line::from(format!(
            "Available Fan Modes: {}",
            joined_modes(capabilities.fan_modes.iter().map(|mode| mode.as_str()))
        )),
        Line::styled(
            format!("Cooler Boost: {}", support_text(capabilities.cooler_boost)),
            capability_style(capabilities.cooler_boost, theme),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::hardware::SupportMode;

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::render_fans;

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(100, 30, |frame| {
            render_fans(frame, frame.area(), &live, &capabilities);
        })
    }

    #[test]
    fn renders_cpu_fan_as_percentage() {
        assert!(text().contains("CPU Fan: 42%"));
    }

    #[test]
    fn renders_gpu_fan_as_percentage() {
        assert!(text().contains("GPU Fan: 31%"));
    }

    #[test]
    fn output_contains_no_rpm() {
        assert!(!text().contains("RPM"));
    }

    #[test]
    fn renders_current_fan_mode() {
        assert!(text().contains("Fan Mode: auto"));
    }

    #[test]
    fn renders_available_fan_modes() {
        assert!(text().contains("Available Fan Modes: auto, silent, future-mode"));
    }

    #[test]
    fn cpu_fan_capability_supported() {
        assert!(text().contains("CPU Fan Telemetry: Supported"));
    }

    #[test]
    fn gpu_fan_capability_unavailable_reports_honestly() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut capabilities = full_capabilities();
        capabilities.gpu_fan = false;
        let text = screen_text(100, 30, |frame| {
            render_fans(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("GPU Fan Telemetry: Unavailable"));
    }

    #[test]
    fn cooler_boost_capability_supported() {
        assert!(text().contains("Cooler Boost: Supported"));
    }

    #[test]
    fn degraded_hides_stale_telemetry() {
        let (live, _) = live_for(
            vec![
                Ok(healthy_snapshot()),
                Err(crate::hardware::BackendError::InvalidData(
                    "fan bus offline".to_owned(),
                )),
            ],
            SupportMode::Ready,
            2,
        );
        assert_eq!(live.history().len(), 1);
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_fans(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("DEGRADED"));
        assert!(text.contains("CPU Fan: N/A"));
        assert!(!text.contains("42%"));
    }

    #[test]
    fn degraded_keeps_capability_metadata_visible() {
        let (live, _) = live_for(
            vec![
                Ok(healthy_snapshot()),
                Err(crate::hardware::BackendError::Unavailable),
            ],
            SupportMode::Ready,
            2,
        );
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_fans(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("CPU Fan Telemetry: Supported"));
        assert!(text.contains("Available Fan Modes: auto, silent, future-mode"));
    }
}
