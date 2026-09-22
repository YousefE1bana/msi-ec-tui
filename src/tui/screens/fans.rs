//! Fans screen in the approved v1.1 card system.
//!
//! Fan readings are percentage-style values, never RPM. Control rows are
//! selectable drafts; confirming creates pending data only and never
//! executes.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware};
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::controls::control_row_styled;
use crate::tui::editing::{ControlState, control_rows};
use crate::tui::shell;
use crate::tui::theme::Theme;
use crate::tui::ui::{fan_mode_text, fan_text, joined_modes, on_off_text};

/// Renders current fan telemetry plus fan capability metadata with
/// selectable control rows and labeled history graphs. Fan readings stay
/// percentage-style; no execution.
pub fn render_fans<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &ControlState,
) {
    render_fans_with_theme(
        frame,
        area,
        &AppState::default(),
        live,
        capabilities,
        controls,
        &Theme::default(),
    );
}

/// Theme-aware fans renderer: approved shell plus real state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_fans_with_theme<B: EcBackend>(
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
    render_control_card(
        frame,
        regions.cards[0],
        live,
        capabilities,
        controls,
        focus == 0,
        theme,
    );
    render_cooling_card(frame, regions.cards[1], live, focus == 1, theme);
    render_modes_card(
        frame,
        regions.cards[2],
        live,
        capabilities,
        focus == 2,
        theme,
    );
    render_status_card(
        frame,
        regions.cards[3],
        live,
        capabilities,
        focus == 3,
        theme,
    );
}

/// Card layout in focus order: control, cooling, modes, status. Control
/// rows start at the first inner line in selection order.
pub(crate) fn hit_regions(workspace: Rect) -> shell::ScreenRegions {
    let (top, bottom) = shell::vsplit2(workspace, 52);
    let (tl, tr) = shell::hpair(top, 50);
    let (bl, br) = shell::hpair(bottom, 50);
    let cards = vec![tl, tr, bl, br];
    let inner = shell::inset(tl);
    let rows = (0..control_rows(crate::app::Screen::Fans).len())
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
    let inner = shell::card(frame, area, "FAN CONTROL", focused, theme);
    let snapshot = live.current_snapshot();
    let mode = live.mode();
    let mut lines: Vec<Line<'static>> = control_rows(crate::app::Screen::Fans)
        .iter()
        .map(|control| {
            let selected = Some(*control) == controls.selected(crate::app::Screen::Fans);
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
    lines.extend(crate::tui::controls::staged_footer_lines(controls, theme));
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Enter edits · ←/→ adjusts · review confirms",
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_cooling_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "LIVE COOLING", focused, theme);
    let snapshot = live.current_snapshot();
    let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 48);
    let cpu_fan = snapshot.and_then(|s| s.cpu_fan);
    let gpu_fan = snapshot.and_then(|s| s.gpu_fan);
    let cpu = snapshot.and_then(|s| s.cpu_temperature);
    let gpu = snapshot.and_then(|s| s.gpu_temperature);
    let mut lines = vec![
        Line::from(format!("CPU Fan: {}", fan_text(cpu_fan))),
        shell::bar_line(fan_frac(cpu_fan), bar_w, theme),
        Line::from(""),
        Line::from(format!("GPU Fan: {}", fan_text(gpu_fan))),
        shell::bar_line(fan_frac(gpu_fan), bar_w, theme),
        Line::from(""),
        Line::styled(
            format!(
                "CPU {} · GPU {}",
                crate::tui::ui::temperature_text(cpu),
                crate::tui::ui::temperature_text(gpu)
            ),
            Style::default().fg(theme.muted),
        ),
    ];
    let budget = usize::from(inner.width.saturating_sub(2));
    for row in crate::tui::history::fan_history_lines(live.history(), budget) {
        lines.push(Line::styled(row, Style::default().fg(theme.muted)));
    }
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_modes_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "AVAILABLE MODES", focused, theme);
    let snapshot = live.current_snapshot();
    let mut lines = capability_lines(capabilities, theme);
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        ratatui::text::Span::styled("Current: ", Style::default().fg(theme.muted)),
        ratatui::text::Span::styled(
            format!(
                "{} · boost {}",
                fan_mode_text(snapshot.and_then(|s| s.fan_mode.as_ref())),
                on_off_text(snapshot.and_then(|s| s.cooler_boost)),
            ),
            Style::default().fg(theme.foreground),
        ),
    ]));
    lines.push(Line::styled(
        "Values stay percent/raw",
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_status_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "COOLING STATUS", focused, theme);
    let snapshot = live.current_snapshot();
    let lines = vec![
        Line::from(format!(
            "Fan Mode: {}",
            fan_mode_text(snapshot.and_then(|s| s.fan_mode.as_ref()))
        )),
        Line::from(format!(
            "Cooler Boost: {}",
            on_off_text(snapshot.and_then(|s| s.cooler_boost))
        )),
        Line::from(vec![
            ratatui::text::Span::styled("Telemetry: ", Style::default().fg(theme.muted)),
            ratatui::text::Span::styled(
                if live.current_snapshot().is_some() {
                    "live · EC direct"
                } else if live.is_degraded() {
                    "degraded · no data"
                } else {
                    "waiting · no data"
                }
                .to_owned(),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            ratatui::text::Span::styled("Access: ", Style::default().fg(theme.muted)),
            ratatui::text::Span::styled(
                crate::tui::ui::support_mode_text(live.mode()).to_owned(),
                crate::tui::ui::support_mode_style(live.mode(), theme),
            ),
        ]),
        Line::from(""),
        Line::styled(
            format!(
                "Fan telemetry: {}",
                crate::tui::ui::support_text(capabilities.cpu_fan || capabilities.gpu_fan)
            ),
            Style::default().fg(theme.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

pub(crate) fn current_lines(
    snapshot: Option<&crate::hardware::HardwareSnapshot>,
) -> Vec<Line<'static>> {
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

pub(crate) fn capability_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        Line::styled(
            format!(
                "CPU Fan Telemetry: {}",
                crate::tui::ui::support_text(capabilities.cpu_fan)
            ),
            crate::tui::ui::capability_style(capabilities.cpu_fan, theme),
        ),
        Line::styled(
            format!(
                "GPU Fan Telemetry: {}",
                crate::tui::ui::support_text(capabilities.gpu_fan)
            ),
            crate::tui::ui::capability_style(capabilities.gpu_fan, theme),
        ),
        Line::from(format!(
            "Available Fan Modes: {}",
            joined_modes(capabilities.fan_modes.iter().map(|mode| mode.as_str()))
        )),
        Line::styled(
            format!(
                "Cooler Boost: {}",
                crate::tui::ui::support_text(capabilities.cooler_boost)
            ),
            crate::tui::ui::capability_style(capabilities.cooler_boost, theme),
        ),
    ]
}

fn fan_frac(value: Option<crate::hardware::FanPercent>) -> f64 {
    value.map(|v| f64::from(v.get()) / 100.0).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use crate::hardware::SupportMode;

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::{hit_regions, render_fans};

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(160, 50, |frame| {
            render_fans(
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
            "FAN CONTROL",
            "LIVE COOLING",
            "AVAILABLE MODES",
            "COOLING STATUS",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
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
        let text = text();
        assert!(!text.contains("RPM"));
        assert!(!text.contains("rpm"));
    }

    #[test]
    fn renders_current_fan_mode() {
        assert!(text().contains("Fan Mode: auto"));
    }

    #[test]
    fn renders_available_fan_modes() {
        assert!(text().contains("auto, silent, future-mode"));
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
        let text = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
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
        let text = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("DEGRADED"));
        assert!(text.contains("CPU Fan: N/A"));
        for line in text.lines() {
            if line.contains("42%") {
                assert!(line.contains("History"), "{line:?}");
            }
        }
        assert!(text.lines().any(|line| line.contains("42%")));
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
        let text = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("CPU Fan Telemetry: Supported"));
        assert!(text.contains("auto, silent, future-mode"));
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
            render_fans(frame, frame.area(), &live, &capabilities, controls);
        })
    }

    #[test]
    fn controls_panel_renders_with_selection() {
        let text = text_with_controls(
            &full_capabilities(),
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("FAN CONTROL"));
        assert!(text.contains("▸ "));
        assert!(text.contains("Fan Mode"));
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
    fn unsupported_fan_shows_unsupported() {
        let mut caps = full_capabilities();
        caps.fan_modes.clear();
        let text = text_with_controls(
            &caps,
            &crate::tui::editing::ControlState::default(),
            SupportMode::Ready,
        );
        assert!(text.contains("Unsupported"));
    }

    #[test]
    fn editing_and_pending_render_without_rpm() {
        use crate::tui::editing::ControlState;
        use crate::tui::screens::support::healthy_snapshot;
        let mut controls = ControlState::default();
        assert!(controls.begin_edit(
            crate::app::Screen::Fans,
            Some(&healthy_snapshot()),
            &full_capabilities(),
            &SupportMode::Ready
        ));
        assert!(controls.confirm(&SupportMode::Ready, &full_capabilities()));
        let text = text_with_controls(&full_capabilities(), &controls, SupportMode::Ready);
        assert!(text.contains("Pending confirmation"));
        assert!(text.contains("NOT applied yet"));
        assert!(!text.contains("RPM"));
    }

    #[test]
    fn hit_regions_match_drawn_control_rows() {
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48));
        assert_eq!(regions.cards.len(), 4);
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
                render_fans(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &live,
                    &caps,
                    &crate::tui::editing::ControlState::default(),
                );
            })
            .expect("zero-area fans draws");
    }

    #[test]
    fn full_shows_fan_history_with_percent_semantics() {
        let text = text();
        assert!(text.contains("CPU Fan History: 42%"));
        assert!(text.contains("GPU Fan History: 31%"));
        assert!(text.contains("min 42 / max 42"));
        assert!(!text.contains("RPM"));
    }

    #[test]
    fn cpu_only_fan_does_not_invent_gpu_graph() {
        use crate::hardware::FanPercent;
        let snapshot = crate::hardware::HardwareSnapshot {
            cpu_fan: FanPercent::try_from(55).ok(),
            ..Default::default()
        };
        let (live, _) = live_for(vec![Ok(snapshot)], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("CPU Fan History: 55%"));
        assert!(text.contains("GPU Fan History: No history"));
    }

    #[test]
    fn gpu_only_fan_does_not_invent_cpu_graph() {
        use crate::hardware::FanPercent;
        let snapshot = crate::hardware::HardwareSnapshot {
            gpu_fan: FanPercent::try_from(70).ok(),
            ..Default::default()
        };
        let (live, _) = live_for(vec![Ok(snapshot)], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("GPU Fan History: 70%"));
        assert!(text.contains("CPU Fan History: No history"));
    }

    #[test]
    fn empty_history_shows_no_history_state() {
        let (live, _) = live_for(
            vec![Ok(crate::hardware::HardwareSnapshot::default())],
            SupportMode::Ready,
            1,
        );
        let capabilities = full_capabilities();
        let text = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert!(text.contains("CPU Fan History: No history"));
        assert!(text.contains("GPU Fan History: No history"));
    }

    #[test]
    fn history_rendering_performs_zero_backend_calls() {
        let (live, calls) = live_for(
            vec![
                Ok(healthy_snapshot()),
                Ok(healthy_snapshot()),
                Ok(healthy_snapshot()),
            ],
            SupportMode::Ready,
            3,
        );
        assert_eq!(calls.get(), 3);
        assert_eq!(live.history().len(), 3);
        let capabilities = full_capabilities();
        let _ = screen_text(160, 50, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        assert_eq!(calls.get(), 3);
    }
}
