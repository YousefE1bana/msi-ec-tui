//! Performance screen in the approved v1.1 card system.
//!
//! Current values come from [`LiveHardware::current_snapshot`] only.
//! Available modes come from injected startup [`Capabilities`]. Control
//! rows are selectable drafts; confirming creates pending data only and
//! never executes hardware writes.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware};
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::controls::control_row_styled;
use crate::tui::editing::{ControlState, control_rows};
use crate::tui::shell;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, fan_mode_text, fan_text, joined_modes, on_off_text, performance_lines,
    shift_mode_text, support_text, temperature_text,
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
    render_performance_with_theme(
        frame,
        area,
        &AppState::default(),
        live,
        capabilities,
        controls,
        &Theme::default(),
    );
}

/// Theme-aware performance renderer: approved shell plus real state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_performance_with_theme<B: EcBackend>(
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
    render_control_card(
        frame,
        regions.cards[0],
        live,
        capabilities,
        controls,
        focus == 0,
        theme,
    );
    render_telemetry_card(frame, regions.cards[1], live, focus == 1, theme);
    render_current_card(frame, regions.cards[2], live, focus == 2, theme);
    render_capabilities_card(
        frame,
        regions.cards[3],
        capabilities,
        live,
        focus == 3,
        theme,
    );
    render_activity_card(frame, regions.cards[4], live, focus == 4, theme);
}

/// Card layout in focus order: control, telemetry, current, capabilities,
/// activity. Control rows start at the first inner line in selection
/// order so mouse clicks land on the drawn rows.
pub(crate) fn hit_regions(workspace: Rect) -> shell::ScreenRegions {
    let (top, mid, bottom) = shell::vsplit3(workspace, 44, 33);
    let (tl, tr) = shell::hpair(top, 60);
    let (ml, mr) = shell::hpair(mid, 50);
    let cards = vec![tl, tr, ml, mr, bottom];
    let inner = shell::inset(tl);
    let rows = (0..control_rows(crate::app::Screen::Performance).len())
        .map(|i| shell::row_rect(inner, i))
        .collect();
    shell::ScreenRegions { cards, rows }
}

fn render_control_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "PERFORMANCE CONTROL", focused, theme);
    let snapshot = live.current_snapshot();
    let mode = live.mode();
    let rows: Vec<Line<'static>> = control_rows(crate::app::Screen::Performance)
        .iter()
        .map(|control| {
            let selected = Some(*control) == controls.selected(crate::app::Screen::Performance);
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
    let mut lines = rows;
    lines.extend(crate::tui::controls::editor_footer_lines(controls, theme));
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Enter edits · ←/→ adjusts · review confirms",
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_telemetry_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "LIVE TELEMETRY", focused, theme);
    let snapshot = live.current_snapshot();
    // Preserve all readings before decorative meters on short terminals.
    if inner.height < 14 {
        frame.render_widget(
            Paragraph::new(Text::from(crate::tui::ui::thermals_lines(snapshot)))
                .style(shell::card_style(theme)),
            inner,
        );
        return;
    }
    let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 48);
    let cpu = snapshot.and_then(|s| s.cpu_temperature);
    let gpu = snapshot.and_then(|s| s.gpu_temperature);
    let cpu_fan = snapshot.and_then(|s| s.cpu_fan);
    let gpu_fan = snapshot.and_then(|s| s.gpu_fan);
    let lines = vec![
        Line::styled("CPU Temperature", Style::default().fg(theme.accent)),
        Line::styled(
            temperature_text(cpu),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        shell::bar_line(temp_frac(cpu), bar_w, theme),
        Line::styled("GPU Temperature", Style::default().fg(theme.accent)),
        Line::styled(
            temperature_text(gpu),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        shell::bar_line(temp_frac(gpu), bar_w, theme),
        Line::from(""),
        Line::styled("CPU Fan", Style::default().fg(theme.accent)),
        Line::styled(
            fan_text(cpu_fan),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        shell::bar_line(fan_frac(cpu_fan), bar_w, theme),
        Line::styled("GPU Fan", Style::default().fg(theme.accent)),
        Line::styled(
            fan_text(gpu_fan),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        shell::bar_line(fan_frac(gpu_fan), bar_w, theme),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_current_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "CURRENT STATE", focused, theme);
    let mut lines = performance_lines(live.current_snapshot());
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Access: ", Style::default().fg(theme.muted)),
        Span::styled(
            crate::tui::ui::support_mode_text(live.mode()).to_owned(),
            crate::tui::ui::support_mode_style(live.mode(), theme),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_capabilities_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    capabilities: &Capabilities,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "CAPABILITIES", focused, theme);
    let mut lines = capability_lines(capabilities, theme);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Telemetry: ", Style::default().fg(theme.muted)),
        Span::styled(
            crate::tui::ui::telemetry_state_text(
                live.is_degraded(),
                live.current_snapshot().is_some(),
            )
            .to_owned(),
            crate::tui::ui::telemetry_style(
                live.is_degraded(),
                live.current_snapshot().is_some(),
                theme,
            ),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_activity_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "ACTIVITY", focused, theme);
    let snapshot = live.current_snapshot();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Shift current: ", Style::default().fg(theme.muted)),
            Span::styled(
                shift_mode_text(snapshot.and_then(|s| s.shift_mode.as_ref())),
                Style::default().fg(theme.foreground),
            ),
            Span::styled("   Fan current: ", Style::default().fg(theme.muted)),
            Span::styled(
                fan_mode_text(snapshot.and_then(|s| s.fan_mode.as_ref())),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Boost: ", Style::default().fg(theme.muted)),
            Span::styled(
                on_off_text(snapshot.and_then(|s| s.cooler_boost)),
                Style::default().fg(theme.foreground),
            ),
            Span::styled("   Super: ", Style::default().fg(theme.muted)),
            Span::styled(
                on_off_text(snapshot.and_then(|s| s.super_battery)),
                Style::default().fg(theme.foreground),
            ),
        ]),
    ];
    for row in crate::tui::history::temperature_summary_lines(live.history()) {
        lines.push(Line::styled(row, Style::default().fg(theme.muted)));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
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

fn temp_frac(value: Option<crate::hardware::TemperatureCelsius>) -> f64 {
    value.map(|v| f64::from(v.get()) / 100.0).unwrap_or(0.0)
}

fn fan_frac(value: Option<crate::hardware::FanPercent>) -> f64 {
    value.map(|v| f64::from(v.get()) / 100.0).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use crate::hardware::{FanMode, ShiftMode, SupportMode};

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::{hit_regions, render_performance};

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(160, 50, |frame| {
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
    fn renders_shell_and_card_headings() {
        let text = text();
        for heading in [
            "PERFORMANCE CONTROL",
            "LIVE TELEMETRY",
            "CURRENT STATE",
            "CAPABILITIES",
            "ACTIVITY",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
        assert!(text.contains("Quit"));
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
        let text = screen_text(160, 50, |frame| {
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
        let text = screen_text(160, 50, |frame| {
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
        screen_text(160, 50, |frame| {
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
        assert!(text.contains("PERFORMANCE CONTROL"));
        assert!(text.contains("▸ "));
        assert!(text.contains("Shift Mode"));
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
        let text = screen_text(160, 50, |frame| {
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
    fn hit_regions_match_drawn_control_rows() {
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48));
        assert_eq!(regions.cards.len(), 5);
        assert_eq!(regions.rows.len(), 4);
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
    #[test]
    fn sampled_state_never_claims_execution_verification() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let text = screen_text(160, 50, |frame| {
            render_performance(
                frame,
                frame.area(),
                &live,
                &full_capabilities(),
                &crate::tui::editing::ControlState::default(),
            )
        });
        assert!(!text.contains("verified"));
    }
}
