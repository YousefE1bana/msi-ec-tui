//! Read-only devices screen: current device state plus capability metadata.
//!
//! Fn/Win keys expose capability existence only: the snapshot carries no
//! runtime Fn/Win values, so no current state is ever invented. Control
//! rows are selectable drafts for webcam/backlight only; Fn/Win stay
//! informational and Task 4 never executes.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{BacklightCapability, Capabilities, EcBackend};

use crate::tui::controls::control_row_lines;
use crate::tui::editing::ControlState;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, device_lines, render_panel, render_screen_shell, support_text,
};

/// Renders current device state plus control-interface capabilities with
/// selectable rows. Fn/Win keys report capability existence only: the
/// snapshot carries no runtime Fn/Win values, so no current state is
/// invented.
pub fn render_devices<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
) {
    render_devices_with_theme(frame, area, live, capabilities, controls, &Theme::default());
}

/// Theme-aware devices renderer behind the Task-5 API.
pub(crate) fn render_devices_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Devices", live, theme);
    let control_rows = control_row_lines(
        crate::app::Screen::Devices,
        live.current_snapshot(),
        capabilities,
        live.mode(),
        controls,
        theme,
    );
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(control_rows.len() as u16 + 2),
            Constraint::Min(0),
        ])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " CURRENT ",
        device_lines(live.current_snapshot()),
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

fn backlight_capability_text(capability: Option<&BacklightCapability>) -> String {
    match capability {
        Some(spec) => format!("Supported (Max Level: {})", spec.max_brightness),
        None => "Unavailable".to_owned(),
    }
}

pub(crate) fn capability_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
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
            render_devices(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
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
                render_devices(
                    frame,
                    frame.area(),
                    &live,
                    &capabilities,
                    &crate::tui::editing::ControlState::default(),
                );
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
            render_devices(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
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

    fn text_with_controls(
        capabilities: &crate::hardware::Capabilities,
        controls: &crate::tui::editing::ControlState,
        mode: SupportMode,
    ) -> String {
        use crate::tui::screens::support::{healthy_snapshot, live_for};
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], mode, 1);
        let capabilities = capabilities.clone();
        screen_text(100, 30, |frame| {
            render_devices(frame, frame.area(), &live, &capabilities, controls);
        })
    }

    #[test]
    fn controls_panel_renders_with_fn_win_informational() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("CONTROLS"));
        assert!(text.contains("> Webcam"));
        assert!(text.contains("Fn Key"));
        assert!(text.contains("informational only"));
        assert!(text.contains("Win Key"));
    }

    #[test]
    fn fn_win_never_create_commands() {
        use crate::tui::editing::{ControlId, ControlState};
        use crate::tui::screens::support::healthy_snapshot;
        // Fn/Win initial drafts are always None.
        assert!(ControlState::default().pending().is_none());
        let snapshot = healthy_snapshot();
        for control in [ControlId::FnKeyInfo, ControlId::WinKeyInfo] {
            assert!(
                crate::tui::editing::initial_draft(
                    control,
                    Some(&snapshot),
                    &full_capabilities(),
                    &SupportMode::Ready
                )
                .is_none()
            );
        }
    }

    #[test]
    fn backlight_pending_never_exceeds_max() {
        use crate::tui::editing::ControlState;
        use crate::tui::screens::support::healthy_snapshot;
        let mut controls = ControlState::default();
        // Select backlight row (index 2).
        controls.move_down(crate::app::Screen::Devices);
        controls.move_down(crate::app::Screen::Devices);
        assert!(controls.begin_edit(
            crate::app::Screen::Devices,
            Some(&healthy_snapshot()),
            &full_capabilities(),
            &SupportMode::Ready,
        ));
        for _ in 0..10 {
            controls.adjust(&full_capabilities(), 1);
            if let Some(editor) = controls.editor() {
                match editor.draft() {
                    crate::hardware::HardwareCommand::SetKeyboardBacklight(level) => {
                        assert!(*level <= 3, "level {level} exceeds max 3");
                    }
                    other => panic!("unexpected draft {other:?}"),
                }
            }
        }
    }

    #[test]
    fn read_only_devices_show_disabled_but_fn_stays_informational() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
        );
        assert!(text.contains("Disabled (read-only)"));
        assert!(text.contains("informational only"));
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
                render_devices(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &live,
                    &caps,
                    &crate::tui::editing::ControlState::default(),
                );
            })
            .expect("zero-area devices draws");
    }
}
