//! Battery screen in the approved v1.1 card system.
//!
//! Only battery threshold control carries an explicit capability label:
//! absent charge/state/AC values render `N/A`, never "Unavailable". The
//! threshold and Super Battery rows use the existing editors; accepting
//! a draft creates pending data only and never executes.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware};
use crate::hardware::{Capabilities, EcBackend, SupportMode};

use crate::tui::controls::control_row_styled;
use crate::tui::editing::{ControlState, control_rows};
use crate::tui::shell;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    ac_text, battery_lines, battery_status_text, capability_style, percent_text, support_text,
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
    render_battery_with_theme(
        frame,
        area,
        &AppState::default(),
        live,
        capabilities,
        controls,
        &Theme::default(),
    );
}

/// Theme-aware battery renderer: approved shell plus real state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_battery_with_theme<B: EcBackend>(
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
    if regions.cards.len() != 5 {
        return;
    }
    let focus = app.focused_card();
    render_status_card(frame, regions.cards[0], live, focus == 0, theme);
    render_limit_card(
        frame,
        regions.cards[1],
        live,
        capabilities,
        controls,
        focus == 1,
        theme,
    );
    render_policy_card(
        frame,
        regions.cards[2],
        live,
        capabilities,
        focus == 2,
        theme,
    );
    render_details_card(frame, regions.cards[3], live, focus == 3, theme);
    render_safety_strip(frame, regions.cards[4], focus == 4, theme);
}

/// Card layout in focus order: status, limit, policy, details, safety.
/// Threshold and Super Battery rows start at the first inner line of the
/// limit card so mouse clicks land on the drawn row.
pub(crate) fn hit_regions(workspace: Rect) -> shell::ScreenRegions {
    let (top, mid, bottom) = shell::vsplit3(workspace, 40, 40);
    let (tl, tr) = shell::hpair(top, 50);
    let (bl, br) = shell::hpair(mid, 50);
    let cards = vec![tl, tr, bl, br, bottom];
    let inner = shell::inset(tr);
    let rows = (0..control_rows(crate::app::Screen::Battery).len())
        .map(|i| shell::row_rect(inner, i))
        .collect();
    shell::ScreenRegions { cards, rows }
}

fn render_status_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "BATTERY STATUS", focused, theme);
    let snapshot = live.current_snapshot();
    let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 48);
    let charge = snapshot.and_then(|s| s.battery_percentage);
    let lines = vec![
        Line::styled("Battery", Style::default().fg(theme.accent)),
        Line::styled(
            percent_text(charge),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        shell::bar_line(charge_frac(charge), bar_w, theme),
        Line::from(vec![
            Span::styled("State: ", Style::default().fg(theme.muted)),
            Span::styled(
                battery_status_text(snapshot.and_then(|s| s.battery_status.as_ref())).to_owned(),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("AC: ", Style::default().fg(theme.muted)),
            Span::styled(
                ac_text(snapshot.and_then(|s| s.ac_connected)).to_owned(),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Window: ", Style::default().fg(theme.muted)),
            Span::styled(
                format!(
                    "{} → {}",
                    percent_text(snapshot.and_then(|s| s.battery_start_threshold)),
                    percent_text(snapshot.and_then(|s| s.battery_end_threshold)),
                ),
                Style::default().fg(theme.foreground),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_limit_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "CHARGE LIMIT", focused, theme);
    let snapshot = live.current_snapshot();
    let mode = live.mode();
    let mut lines: Vec<Line<'static>> = control_rows(crate::app::Screen::Battery)
        .iter()
        .map(|control| {
            let selected = Some(*control) == controls.selected(crate::app::Screen::Battery);
            control_row_styled(
                *control,
                selected,
                snapshot,
                capabilities,
                mode,
                controls,
                theme,
            )
        })
        .collect();
    let start = snapshot.and_then(|s| s.battery_start_threshold);
    let end = snapshot.and_then(|s| s.battery_end_threshold);
    lines.extend(crate::tui::controls::editor_footer_lines(controls, theme));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Start: ", Style::default().fg(theme.muted)),
        Span::styled(percent_text(start), Style::default().fg(theme.foreground)),
        Span::styled("   End: ", Style::default().fg(theme.muted)),
        Span::styled(percent_text(end), Style::default().fg(theme.foreground)),
    ]));
    if let (Some(a), Some(b)) = (start, end) {
        let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 44);
        lines.push(shell::window_bar(a, b, bar_w, theme));
    }
    lines.push(Line::styled(
        "Start trails end by 10 points",
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_policy_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "POWER POLICY", focused, theme);
    let snapshot = live.current_snapshot();
    let lines = vec![
        Line::from(vec![
            Span::styled("Super Battery: ", Style::default().fg(theme.muted)),
            Span::styled(
                crate::tui::ui::on_off_text(snapshot.and_then(|s| s.super_battery)).to_owned(),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::styled(
            "Start trails end by 10 points",
            Style::default().fg(theme.muted),
        ),
        Line::from(""),
        threshold_capability_line(capabilities, theme),
        Line::from(vec![
            Span::styled("Access: ", Style::default().fg(theme.muted)),
            Span::styled(
                crate::tui::ui::support_mode_text(live.mode()).to_owned(),
                crate::tui::ui::support_mode_style(live.mode(), theme),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_details_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "BATTERY DETAILS", focused, theme);
    let snapshot = live.current_snapshot();
    let mut lines = battery_lines(snapshot);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Controls: ", Style::default().fg(theme.muted)),
        Span::styled(
            if matches!(live.mode(), SupportMode::ReadOnly(_)) {
                "unavailable"
            } else {
                "available"
            }
            .to_owned(),
            Style::default().fg(theme.foreground),
        ),
    ]));
    lines.push(Line::styled(
        format!(
            "Updated: {}",
            crate::tui::ui::telemetry_state_text(live.is_degraded(), snapshot.is_some())
        ),
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_safety_strip(frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
    let inner = shell::card(frame, area, "SAFETY", focused, theme);
    let lines = vec![Line::styled(
        "Changes are staged first and require review before apply.",
        Style::default().fg(theme.muted),
    )];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
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

fn charge_frac(value: Option<u8>) -> f64 {
    value.map(|v| f64::from(v) / 100.0).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use crate::hardware::{HardwareSnapshot, SupportMode};

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::{hit_regions, render_battery};

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(160, 50, |frame| {
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
    fn renders_shell_and_card_headings() {
        let text = text();
        for heading in [
            "BATTERY STATUS",
            "CHARGE LIMIT",
            "POWER POLICY",
            "BATTERY DETAILS",
            "SAFETY",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
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
        assert!(text().contains("50%"));
    }

    #[test]
    fn renders_end_threshold() {
        assert!(text().contains("80%"));
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
        let text = screen_text(160, 50, |frame| {
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
        let text = screen_text(160, 50, |frame| {
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
        screen_text(160, 50, |frame| {
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
        assert!(text.contains("CHARGE LIMIT"));
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
        let pending = controls.pending_command().expect("pending stored");
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
    fn gf63_fixture_maps_real_threshold_state() {
        use std::path::PathBuf;

        use crate::hardware::{LinuxSysfsReader, SystemPaths};
        let paths = SystemPaths::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("gf63"),
        );
        let mut app = crate::tui::prepare_tui(paths, LinuxSysfsReader)
            .expect("gf63 fixture prepares without hardware");
        app.refresh();
        let backend = ratatui::backend::TestBackend::new(160, 50);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_battery(
                    frame,
                    frame.area(),
                    app.live(),
                    app.capabilities(),
                    app.controls(),
                );
            })
            .expect("fixture battery draws");
        let mut text = String::new();
        for y in 0..50 {
            let mut line = String::new();
            for x in 0..160 {
                line.push_str(terminal.backend().buffer()[(x, y)].symbol());
            }
            text.push_str(line.trim_end());
            text.push('\n');
        }
        assert!(text.contains("BATTERY STATUS"));
        assert!(text.contains("CHARGE LIMIT"));
        assert!(text.contains("Threshold Control"));
        assert!(!text.contains("RPM"));
        assert!(!text.contains("cycle"));
    }

    #[test]
    fn hit_regions_match_drawn_threshold_row() {
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48));
        assert_eq!(regions.cards.len(), 5);
        assert_eq!(regions.rows.len(), 2);
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
