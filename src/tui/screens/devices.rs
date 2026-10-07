//! Devices screen in the approved v1.1 card system.
//!
//! Fn/Win keys expose capability existence only: the snapshot carries no
//! runtime Fn/Win values, so no current state is ever invented. Control
//! rows are selectable drafts for webcam/backlight only; Fn/Win stay
//! informational and confirmation still gates every change.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware};
use crate::hardware::{BacklightCapability, Capabilities, EcBackend, HardwareSnapshot};

use crate::tui::editing::{ControlId, ControlState, control_rows};
use crate::tui::shell;
use crate::tui::theme::Theme;
use crate::tui::ui::{capability_style, device_lines, support_text};

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
    render_devices_with_theme(
        frame,
        area,
        &AppState::default(),
        live,
        capabilities,
        controls,
        &Theme::default(),
    );
}

/// Theme-aware devices renderer: approved shell plus real state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_devices_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme.base_style()),
        area,
    );
    let (top, workspace, footer) = shell::shell_split(area);
    shell::render_top_strip(frame, top, live, theme);
    shell::render_bottom_strip(frame, footer, live, theme);
    let regions = hit_regions(workspace);
    if regions.cards.len() != 4 {
        return;
    }
    let focus = app.focused_card();
    render_table_card(
        frame,
        regions.cards[0],
        live,
        capabilities,
        controls,
        focus == 0,
        theme,
    );
    render_details_card(
        frame,
        regions.cards[1],
        live,
        capabilities,
        controls,
        focus == 1,
        theme,
    );
    render_preview_card(frame, regions.cards[2], controls, focus == 2, theme);
    render_status_card(
        frame,
        regions.cards[3],
        capabilities,
        live,
        focus == 3,
        theme,
    );
}

/// Card layout in focus order: table, details, preview, status. Table
/// rows start below the header line in control order so mouse clicks land
/// on the drawn rows.
pub(crate) fn hit_regions(workspace: Rect) -> shell::ScreenRegions {
    let (table, side) = shell::hpair(workspace, 55);
    let (side_top, side_rest) = shell::vsplit2(side, 42);
    let (side_mid, side_bot) = shell::vsplit2(side_rest, 50);
    let cards = vec![table, side_top, side_mid, side_bot];
    let inner = shell::inset(table);
    let rows = (0..control_rows(crate::app::Screen::Devices).len())
        .map(|i| shell::row_rect(inner, i + 1))
        .collect();
    shell::ScreenRegions { cards, rows }
}

const DEVICE_NAME_WIDTH: usize = 18;

pub(crate) fn value_region(
    row: Rect,
    control: ControlId,
    snapshot: Option<&HardwareSnapshot>,
) -> Rect {
    if matches!(control, ControlId::FnKeyInfo | ControlId::WinKeyInfo) {
        return Rect::default();
    }
    shell::text_region(
        row,
        (2 + DEVICE_NAME_WIDTH) as u16,
        &current_value(control, snapshot),
    )
}

/// Real action label per row: toggles flip, backlight steps levels,
/// informational rows stage nothing.
fn action_label(control: ControlId) -> &'static str {
    match control {
        ControlId::Webcam | ControlId::WebcamBlock => "toggle",
        ControlId::KeyboardBacklight => "level",
        _ => "info",
    }
}

/// Current snapshot value for a device row, or an honest placeholder
/// when the snapshot carries nothing (Fn/Win have no runtime values).
fn current_value(control: ControlId, snapshot: Option<&HardwareSnapshot>) -> String {
    match control {
        ControlId::Webcam => match snapshot.and_then(|s| s.webcam) {
            Some(true) => "On".to_owned(),
            Some(false) => "Off".to_owned(),
            None => "N/A".to_owned(),
        },
        ControlId::WebcamBlock => match snapshot.and_then(|s| s.webcam_block) {
            Some(true) => "On".to_owned(),
            Some(false) => "Off".to_owned(),
            None => "N/A".to_owned(),
        },
        ControlId::KeyboardBacklight => match snapshot.and_then(|s| s.keyboard_backlight) {
            Some(level) => format!("Level {level}"),
            None => "N/A".to_owned(),
        },
        _ => "—".to_owned(),
    }
}

/// Capability text for a device row from the real support model.
fn capability_value(control: ControlId, capabilities: &Capabilities) -> String {
    match control {
        ControlId::Webcam => support_text(capabilities.webcam).to_owned(),
        ControlId::WebcamBlock => support_text(capabilities.webcam_block).to_owned(),
        ControlId::KeyboardBacklight => {
            backlight_capability_text(capabilities.keyboard_backlight.as_ref())
        }
        ControlId::FnKeyInfo => support_text(capabilities.fn_key).to_owned(),
        ControlId::WinKeyInfo => support_text(capabilities.win_key).to_owned(),
        _ => "—".to_owned(),
    }
}

fn control_name(control: ControlId) -> &'static str {
    match control {
        ControlId::Webcam => "Webcam",
        ControlId::WebcamBlock => "Webcam Block",
        ControlId::KeyboardBacklight => "Keyboard Backlight",
        ControlId::FnKeyInfo => "Fn Key",
        ControlId::WinKeyInfo => "Win Key",
        _ => "?",
    }
}

fn render_table_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "DEVICES", focused, theme);
    let snapshot = live.current_snapshot();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{:<20}", "DEVICE"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<11}", "CURRENT"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<16}", "CAPABILITY"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "ACTION",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    for control in control_rows(crate::app::Screen::Devices) {
        let selected = Some(*control) == controls.selected(crate::app::Screen::Devices);
        let current = current_value(*control, snapshot);
        let marker = if selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(marker.to_owned(), Style::default().fg(theme.accent)),
            Span::styled(
                format!(
                    "{:<width$}",
                    control_name(*control),
                    width = DEVICE_NAME_WIDTH
                ),
                if selected {
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground)
                },
            ),
            Span::styled(
                format!("{current:<11}"),
                Style::default().fg(theme.foreground),
            ),
            Span::styled(
                format!("{:<16}", capability_value(*control, capabilities)),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                action_label(*control).to_owned(),
                Style::default().fg(theme.muted),
            ),
        ]));
    }
    lines.push(Line::from(""));
    if matches!(live.mode(), crate::hardware::SupportMode::ReadOnly(_)) {
        lines.push(Line::styled(
            "Controls: Disabled (read-only)",
            Style::default().fg(theme.warning),
        ));
    }
    lines.push(Line::styled(
        "Click value / Enter edits · review confirms",
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_details_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "SELECTED DEVICE", focused, theme);
    let snapshot = live.current_snapshot();
    let index = controls.selected_index(crate::app::Screen::Devices);
    let control = control_rows(crate::app::Screen::Devices)
        .get(index)
        .copied()
        .unwrap_or(ControlId::Webcam);
    let lines = vec![
        Line::styled(
            control_name(control).to_uppercase(),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            supported_values(control, capabilities),
            Style::default().fg(theme.muted),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled("Current: ", Style::default().fg(theme.muted)),
            Span::styled(
                current_value(control, snapshot),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Capability: ", Style::default().fg(theme.muted)),
            Span::styled(
                capability_value(control, capabilities),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(""),
        Line::styled(
            "Enter edits where supported",
            Style::default().fg(theme.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

/// Supported values from the real capability model, never invented.
fn supported_values(control: ControlId, capabilities: &Capabilities) -> String {
    match control {
        ControlId::Webcam | ControlId::WebcamBlock => "toggle on/off".to_owned(),
        ControlId::KeyboardBacklight => match &capabilities.keyboard_backlight {
            Some(spec) => format!("levels 0–{}", spec.max_brightness),
            None => "unavailable".to_owned(),
        },
        _ => "informational only".to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn render_preview_card(
    frame: &mut Frame,
    area: Rect,
    controls: &ControlState,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "CONTROL PREVIEW", focused, theme);
    let lines = match controls.editor() {
        Some(_) => {
            let mut lines = crate::tui::controls::editor_footer_lines(controls, theme);
            lines.push(Line::styled(
                "Nothing applied yet",
                Style::default().fg(theme.warning),
            ));
            lines
        }
        None => match controls.pending_command() {
            Some(command) => vec![
                Line::from(vec![
                    Span::styled("Pending: ", Style::default().fg(theme.muted)),
                    Span::styled(
                        crate::tui::controls::command_text(command),
                        Style::default()
                            .fg(theme.warning)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::styled(
                    "NOT applied yet · Enter confirms",
                    Style::default().fg(theme.warning),
                ),
            ],
            None => vec![
                Line::styled("No change staged", Style::default().fg(theme.muted)),
                Line::from(""),
                Line::styled(
                    "Review opens on Enter · nothing changes early",
                    Style::default().fg(theme.muted),
                ),
            ],
        },
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_status_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    capabilities: &Capabilities,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "DEVICE STATUS", focused, theme);
    let mut lines = capability_lines(capabilities, theme);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Access: ", Style::default().fg(theme.muted)),
        Span::styled(
            crate::tui::ui::support_mode_text(live.mode()).to_owned(),
            crate::tui::ui::support_mode_style(live.mode(), theme),
        ),
    ]));
    // Keep device current values visible beside capabilities.
    lines.extend(device_lines(live.current_snapshot()));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
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
    use super::{hit_regions, render_devices};

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(160, 50, |frame| {
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
    fn renders_shell_and_card_headings() {
        let text = text();
        for heading in [
            "DEVICES",
            "SELECTED DEVICE",
            "CONTROL PREVIEW",
            "DEVICE STATUS",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
    }

    #[test]
    fn renders_table_columns() {
        let text = text();
        for heading in ["DEVICE", "CURRENT", "CAPABILITY", "ACTION"] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
    }

    #[test]
    fn renders_webcam_current_state() {
        assert!(text().contains("Webcam"));
        assert!(text().contains("On"));
    }

    #[test]
    fn renders_webcam_block_current_state() {
        assert!(text().contains("Webcam Block"));
        assert!(text().contains("Off"));
    }

    #[test]
    fn renders_backlight_current_level() {
        assert!(text().contains("Level 2"));
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
            let text = screen_text(160, 50, |frame| {
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
        let text = screen_text(160, 50, |frame| {
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
        screen_text(160, 50, |frame| {
            render_devices(frame, frame.area(), &live, &capabilities, controls);
        })
    }

    #[test]
    fn controls_table_marks_selected_row() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("▸ "));
        assert!(text.contains("Webcam"));
        assert!(text.contains("Fn Key"));
        assert!(text.contains("informational only") || text.contains("info"));
        assert!(text.contains("Win Key"));
    }

    #[test]
    fn staged_device_value_renders_amber() {
        use crate::tui::editing::ControlState;
        use crate::tui::screens::support::healthy_snapshot;
        let mut controls = ControlState::default();
        assert!(controls.begin_edit(
            crate::app::Screen::Devices,
            Some(&healthy_snapshot()),
            &full_capabilities(),
            &SupportMode::Ready,
        ));
        controls.adjust(&full_capabilities(), 1);
        let text = text_with_controls(&full_capabilities(), &controls, SupportMode::Ready);
        assert!(text.contains("Editing:"));
    }

    #[test]
    fn fn_win_never_create_commands() {
        use crate::tui::editing::{ControlId, ControlState};
        use crate::tui::screens::support::healthy_snapshot;
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
        assert!(text.contains("info"));
    }

    #[test]
    fn hit_regions_match_drawn_device_rows() {
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48));
        assert_eq!(regions.cards.len(), 4);
        assert_eq!(regions.rows.len(), 5);
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
    #[test]
    fn staged_device_keeps_current_value_in_current_column() {
        let snapshot = healthy_snapshot();
        let (live, _) = live_for(vec![Ok(snapshot.clone())], SupportMode::Ready, 1);
        let caps = full_capabilities();
        let mut controls = crate::tui::editing::ControlState::default();
        assert!(controls.begin_edit(
            crate::app::Screen::Devices,
            Some(&snapshot),
            &caps,
            live.mode()
        ));
        controls.adjust(&caps, 1);
        let text = screen_text(160, 50, |frame| {
            render_devices(frame, frame.area(), &live, &caps, &controls)
        });
        let row = text
            .lines()
            .find(|line| line.contains("▸ Webcam "))
            .unwrap();
        assert!(row.contains("On"), "{row}");
        assert!(text.contains("Editing: Webcam: Off"));
    }
}
