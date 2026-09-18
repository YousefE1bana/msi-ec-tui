//! Read-only devices screen: current device state plus capability metadata.
//!
//! Fn/Win keys expose capability existence only: the snapshot carries no
//! runtime Fn/Win values, so no current state is ever invented. No editing,
//! no hardware transport here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{BacklightCapability, Capabilities, EcBackend};

use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, device_lines, render_panel, render_screen_shell, support_text,
};

/// Renders current device state plus control-interface capabilities. Fn/Win
/// keys report capability existence only: the snapshot carries no runtime
/// Fn/Win values, so no current state is invented.
pub fn render_devices<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    render_devices_with_theme(frame, area, live, capabilities, &Theme::default());
}

/// Theme-aware devices renderer behind the Task-5 API.
pub(crate) fn render_devices_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Devices", live, theme);
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(0)])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        device_lines(live.current_snapshot()),
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

fn backlight_capability_text(capability: Option<&BacklightCapability>) -> String {
    match capability {
        Some(spec) => format!("Supported (Max Level: {})", spec.max_brightness),
        None => "Unavailable".to_owned(),
    }
}

fn capability_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
    let backlight_supported = capabilities.keyboard_backlight.is_some();
    vec![
        Line::styled(
            format!("Webcam: {}", support_text(capabilities.webcam)),
            capability_style(capabilities.webcam, theme),
        ),
        Line::styled(
            format!("Webcam Block: {}", support_text(capabilities.webcam_block)),
            capability_style(capabilities.webcam_block, theme),
        ),
        Line::styled(
            format!(
                "Keyboard Backlight: {}",
                backlight_capability_text(capabilities.keyboard_backlight.as_ref())
            ),
            capability_style(backlight_supported, theme),
        ),
        Line::styled(
            format!("Fn Key: {}", support_text(capabilities.fn_key)),
            capability_style(capabilities.fn_key, theme),
        ),
        Line::styled(
            format!("Win Key: {}", support_text(capabilities.win_key)),
            capability_style(capabilities.win_key, theme),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use crate::hardware::SupportMode;

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::render_devices;

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(100, 30, |frame| {
            render_devices(frame, frame.area(), &live, &capabilities);
        })
    }

    #[test]
    fn renders_webcam_current_state() {
        assert!(text().contains("Webcam: On"));
    }

    #[test]
    fn renders_webcam_block_current_state() {
        assert!(text().contains("Webcam Block: Off"));
    }

    #[test]
    fn renders_backlight_current_level() {
        assert!(text().contains("Keyboard Backlight: 2"));
    }

    #[test]
    fn webcam_capabilities_report_both_states() {
        let cases = [
            (true, true, "Webcam: Supported", "Webcam Block: Supported"),
            (
                true,
                false,
                "Webcam: Supported",
                "Webcam Block: Unavailable",
            ),
            (
                false,
                true,
                "Webcam: Unavailable",
                "Webcam Block: Supported",
            ),
            (
                false,
                false,
                "Webcam: Unavailable",
                "Webcam Block: Unavailable",
            ),
        ];
        for (webcam, block, expected_webcam, expected_block) in cases {
            let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
            let mut capabilities = full_capabilities();
            capabilities.webcam = webcam;
            capabilities.webcam_block = block;
            let text = screen_text(100, 30, |frame| {
                render_devices(frame, frame.area(), &live, &capabilities);
            });
            assert!(text.contains(expected_webcam), "webcam={webcam}");
            assert!(text.contains(expected_block), "block={block}");
        }
    }

    #[test]
    fn backlight_capability_shows_supported_and_max() {
        let text = text();
        assert!(text.contains("Keyboard Backlight: Supported"));
        assert!(text.contains("Max Level: 3"));
    }

    #[test]
    fn absent_backlight_capability_shows_unavailable() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut capabilities = full_capabilities();
        capabilities.keyboard_backlight = None;
        let text = screen_text(100, 30, |frame| {
            render_devices(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Keyboard Backlight: Unavailable"));
    }

    #[test]
    fn fn_key_capability_supported_only() {
        assert!(text().contains("Fn Key: Supported"));
    }

    #[test]
    fn win_key_capability_unavailable_only() {
        assert!(text().contains("Win Key: Unavailable"));
    }

    #[test]
    fn invents_no_fn_win_current_state() {
        let text = text();
        for forbidden in [
            "Fn Key: On",
            "Fn Key: Off",
            "Win Key: On",
            "Win Key: Off",
            "Standard",
        ] {
            assert!(!text.contains(forbidden), "{forbidden:?} must not appear");
        }
    }
}
