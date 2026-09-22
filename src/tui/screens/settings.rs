//! Settings screen in the approved v1.1 card system.
//!
//! Display-only presentation of real persistent preferences: theme,
//! refresh interval, and vim keys come from the loaded [`AppConfig`].
//! Nothing here writes: theme switching lives in the command palette,
//! and no other preference is mutable from the TUI, so no config schema
//! change was needed for this route.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware};
use crate::config::AppConfig;
use crate::hardware::{EcBackend, SupportMode};

use crate::tui::shell;
use crate::tui::theme::Theme;

/// Renders display preferences with defaults. Production passes the real
/// config through [`render_settings_with_theme`].
pub fn render_settings<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    config: &AppConfig,
) {
    render_settings_with_theme(
        frame,
        area,
        &AppState::default(),
        live,
        config,
        &Theme::default(),
    );
}

/// Theme-aware settings renderer: approved shell plus real config state.
/// `mode` supplies the access row; no hardware writes exist on this
/// screen under any input.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_settings_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    config: &AppConfig,
    theme: &Theme,
) {
    let mode = live.mode().clone();
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
    render_interface_card(frame, regions.cards[0], config, focus == 0, theme);
    render_input_card(frame, regions.cards[1], config, focus == 1, theme);
    render_display_card(frame, regions.cards[2], config, &mode, focus == 2, theme);
    render_note_card(frame, regions.cards[3], focus == 3, theme);
}

/// Card layout in focus order: interface, input, display, note. Settings
/// carries no interactive rows: every value is display-only.
pub(crate) fn hit_regions(workspace: Rect) -> shell::ScreenRegions {
    let (main, bottom) = shell::vsplit2(workspace, 86);
    let (left, side) = shell::hpair(main, 52);
    let (side_top, side_bot) = shell::vsplit2(side, 50);
    shell::ScreenRegions {
        cards: vec![left, side_top, side_bot, bottom],
        rows: Vec::new(),
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

#[allow(clippy::too_many_arguments)]
fn render_interface_card(
    frame: &mut Frame,
    area: Rect,
    config: &AppConfig,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "INTERFACE", focused, theme);
    let lines = vec![
        Line::from(vec![
            Span::styled("Theme:            ", Style::default().fg(theme.muted)),
            Span::styled(
                config.theme().display_name().to_owned(),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Refresh interval: ", Style::default().fg(theme.muted)),
            Span::styled(
                format!("{} ms", config.refresh_interval_ms()),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Vim keys:         ", Style::default().fg(theme.muted)),
            Span::styled(
                on_off(config.vim_keys()),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(""),
        Line::styled(
            "Display-only · switch themes with P",
            Style::default().fg(theme.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_input_card(
    frame: &mut Frame,
    area: Rect,
    config: &AppConfig,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "INPUT", focused, theme);
    let lines = vec![
        Line::from(vec![
            Span::styled("Keyboard:  ", Style::default().fg(theme.muted)),
            Span::styled(
                "1–9 · arrows · Tab · Enter · Esc",
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Vim keys:  ", Style::default().fg(theme.muted)),
            Span::styled(
                on_off(config.vim_keys()),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Mouse:     ", Style::default().fg(theme.muted)),
            Span::styled(
                "capture active · click selects · wheel moves",
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::styled(
            "Clicks never confirm or apply",
            Style::default().fg(theme.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_display_card(
    frame: &mut Frame,
    area: Rect,
    config: &AppConfig,
    mode: &SupportMode,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "DISPLAY", focused, theme);
    let lines = vec![
        Line::from(vec![
            Span::styled("Theme:    ", Style::default().fg(theme.muted)),
            Span::styled(
                config.theme().display_name().to_owned(),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Refresh:  ", Style::default().fg(theme.muted)),
            Span::styled(
                format!("{} ms", config.refresh_interval_ms()),
                Style::default().fg(theme.foreground),
            ),
        ]),
        Line::from(vec![
            Span::styled("Access:   ", Style::default().fg(theme.muted)),
            Span::styled(
                crate::tui::ui::support_mode_text(mode).to_owned(),
                crate::tui::ui::support_mode_style(mode, theme),
            ),
        ]),
        Line::from(""),
        Line::styled(
            "Compact rows · full canvas · no scroll",
            Style::default().fg(theme.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_note_card(frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
    let inner = shell::card(frame, area, "NOTE", focused, theme);
    let lines = vec![Line::styled(
        "Changes shown here affect display preferences.",
        Style::default().fg(theme.muted),
    )];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use crate::config::AppConfig;

    use super::{hit_regions, render_settings};

    fn text() -> String {
        use super::super::support::{healthy_snapshot, live_for};
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot())],
            crate::hardware::SupportMode::Ready,
            1,
        );
        let backend = ratatui::backend::TestBackend::new(160, 50);
        let mut terminal = ratatui::Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_settings(frame, frame.area(), &live, &AppConfig::default()))
            .expect("settings draws");
        let mut out = String::new();
        for y in 0..50 {
            let mut line = String::new();
            for x in 0..160 {
                line.push_str(terminal.backend().buffer()[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    #[test]
    fn renders_shell_and_card_headings() {
        let text = text();
        for heading in ["INTERFACE", "INPUT", "DISPLAY", "NOTE"] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
        assert!(text.contains("Quit"));
    }

    #[test]
    fn exposes_real_theme_name() {
        assert!(text().contains(AppConfig::default().theme().display_name()));
    }

    #[test]
    fn exposes_real_refresh_interval() {
        let ms = AppConfig::default().refresh_interval_ms();
        assert!(text().contains(&ms.to_string()));
    }

    #[test]
    fn exposes_real_vim_keys_state() {
        let expected = if AppConfig::default().vim_keys() {
            "on"
        } else {
            "off"
        };
        assert!(text().contains(expected));
    }

    #[test]
    fn carries_no_interactive_rows() {
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48));
        assert_eq!(regions.cards.len(), 4);
        assert!(regions.rows.is_empty());
    }

    #[test]
    fn states_display_only_safety() {
        let text = text();
        assert!(text.contains("Display-only"));
        assert!(text.contains("never confirm"));
    }

    #[test]
    fn zero_area_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        use super::super::support::{healthy_snapshot, live_for};
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot())],
            crate::hardware::SupportMode::Ready,
            1,
        );
        terminal
            .draw(|frame| {
                render_settings(frame, Rect::new(0, 0, 0, 0), &live, &AppConfig::default())
            })
            .expect("zero-area settings draws");
    }
}
