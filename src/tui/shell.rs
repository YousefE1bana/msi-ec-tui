//! Approved v1.1 shell shared by every migrated screen: thin top status
//! strip, card grid workspace, bottom shortcut strip.
//!
//! Pure presentation over already-sampled production state. Cards use the
//! theme `surface`/`accent`/`border` roles; meters use the locked
//! `meter_fill`/`meter_track` pair. No sampling, no sysfs, no execution.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::LiveHardware;
use crate::hardware::{EcBackend, SupportMode};

use super::theme::Theme;
use super::ui::{support_mode_style, support_mode_text, telemetry_state_text, telemetry_style};

/// Footer segments in draw order. Widths derive from these exact literals;
/// [`super::mouse`] measures the same strings so hit regions always match
/// the drawn labels.
pub(crate) const FOOTER_STATUS_READY: &str = " ✓ READY  ";
pub(crate) const FOOTER_STATUS_READ_ONLY: &str = " ✓ READ-ONLY  ";
pub(crate) const FOOTER_HELP: &str = "Help ";
pub(crate) const FOOTER_QUIT: &str = "Quit ";
pub(crate) const FOOTER_Q_KEY: &str = " [Q] ";

pub(crate) const FOOTER_FIXED: &[&str] = &[
    "│ ",
    " [1-9] ",
    "Select ",
    " [↑↓] ",
    "Navigate ",
    " [Enter] ",
    "Open ",
    " [?] ",
];

/// Shell rows: one-row status strip, flexible workspace, one-row footer.
pub(crate) fn shell_split(area: Rect) -> (Rect, Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    (rows[0], rows[1], rows[2])
}

/// Thin full-width top strip:
/// `MEC | device | READY/READ-ONLY | EC firmware | LIVE/... | clock`.
pub(crate) fn render_top_strip<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    let firmware = live
        .device()
        .ec_firmware_version
        .as_deref()
        .unwrap_or("N/A");
    let line = Line::from(vec![
        Span::styled(
            " MEC ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(theme.muted)),
        Span::styled(
            live.device().product_name.clone(),
            Style::default().fg(theme.foreground),
        ),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(
            support_mode_text(live.mode()).to_owned(),
            support_mode_style(live.mode(), theme).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(format!("EC {firmware}"), Style::default().fg(theme.muted)),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(
            telemetry_state_text(live.is_degraded(), live.current_snapshot().is_some()).to_owned(),
            telemetry_style(live.is_degraded(), live.current_snapshot().is_some(), theme)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(clock(), Style::default().fg(theme.muted)),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme.base_style()), area);
}

/// UTC clock for the top strip, std only.
pub(crate) fn clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Approved footer. The status segment follows the real support verdict;
/// span order and widths match the mouse hit regions exactly.
pub(crate) fn render_bottom_strip<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    let read_only = matches!(live.mode(), SupportMode::ReadOnly(_));
    let status = if read_only {
        FOOTER_STATUS_READ_ONLY
    } else {
        FOOTER_STATUS_READY
    };
    let status_style = if read_only {
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD)
    };
    let key = |k: &str| Span::styled(format!(" [{k}] "), Style::default().fg(theme.accent));
    let what = |w: &str| Span::styled(format!("{w} "), Style::default().fg(theme.muted));
    // Order and literals mirror FOOTER_FIXED plus Help/Quit so the mouse
    // module measures identical widths.
    let line = Line::from(vec![
        Span::styled(status.to_owned(), status_style),
        Span::styled("│ ", Style::default().fg(theme.muted)),
        key("1-9"),
        what("Select"),
        key("↑↓"),
        what("Navigate"),
        key("Enter"),
        what("Open"),
        key("?"),
        what("Help"),
        key("Q"),
        what("Quit"),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme.base_style()), area);
}

/// Card shell: thin border (accent when focused), theme surface, accent
/// title. Returns the inner content area.
pub(crate) fn card(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    focused: bool,
    theme: &Theme,
) -> Rect {
    let border = if focused {
        Style::default().fg(theme.accent).bg(theme.surface)
    } else {
        Style::default().fg(theme.border).bg(theme.surface)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .style(Style::default().bg(theme.surface))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// Cool telemetry meter: approved fill on a dark track. Missing values
/// pass `0.0` and render an empty muted track.
pub(crate) fn bar_line(frac: f64, width: usize, theme: &Theme) -> Line<'static> {
    let frac = frac.clamp(0.0, 1.0);
    let n = (frac * width as f64).round() as usize;
    Line::from(vec![
        Span::styled("█".repeat(n), Style::default().fg(theme.meter_fill)),
        Span::styled(
            "░".repeat(width.saturating_sub(n)),
            Style::default().fg(theme.meter_track),
        ),
    ])
}

/// Charge-window range track over 0-100 with the cool meter fill.
pub(crate) fn window_bar(start: u8, end: u8, width: usize, theme: &Theme) -> Line<'static> {
    let a = (f64::from(start) / 100.0 * width as f64).round() as usize;
    let b = (f64::from(end) / 100.0 * width as f64).round() as usize;
    let mut spans = Vec::new();
    for i in 0..width {
        let (ch, color) = if i >= a && i < b {
            ("█", theme.meter_fill)
        } else {
            ("─", theme.meter_track)
        };
        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
    }
    Line::from(spans)
}

/// Muted separator sized to its card.
pub(crate) fn sep(width: usize, theme: &Theme) -> Line<'static> {
    Line::styled("─".repeat(width.max(4)), Style::default().fg(theme.muted))
}

/// Border inset: content origin inside an ALL-borders card. Pure so
/// renderers and mouse hit-testing share identical geometry.
pub(crate) fn inset(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
}

/// Stack only below the full-screen minimum; stacking inside short
/// percentage bands otherwise hides selected controls and drafts.
pub(crate) fn is_narrow(area: Rect) -> bool {
    area.width < 60
}

/// Horizontal pair with a 1-cell gap on wide terminals; vertical stack on
/// narrow ones. Returns (first, second): left/right on wide, top/bottom
/// on narrow.
pub(crate) fn hpair(area: Rect, first_pct: u16) -> (Rect, Rect) {
    if is_narrow(area) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(first_pct),
                Constraint::Length(1),
                Constraint::Percentage(100 - first_pct),
            ])
            .split(area);
        (rows[0], rows[2])
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(first_pct),
                Constraint::Length(1),
                Constraint::Percentage(100 - first_pct),
            ])
            .split(area);
        (cols[0], cols[2])
    }
}

/// Vertical split into two bands.
pub(crate) fn vsplit2(area: Rect, top_pct: u16) -> (Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(top_pct),
            Constraint::Percentage(100 - top_pct),
        ])
        .split(area);
    (rows[0], rows[1])
}

/// Vertical split into three bands.
pub(crate) fn vsplit3(area: Rect, top_pct: u16, mid_pct: u16) -> (Rect, Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(top_pct),
            Constraint::Percentage(mid_pct),
            Constraint::Percentage(100 - top_pct - mid_pct),
        ])
        .split(area);
    (rows[0], rows[1], rows[2])
}

/// Card paragraph style: primary text on the card surface.
pub(crate) fn card_style(theme: &Theme) -> Style {
    Style::default().fg(theme.foreground).bg(theme.surface)
}

/// Clickable geometry for one migrated screen: card bodies in focus
/// order plus interactive row rects in selection order. Pure data so
/// mouse mapping stays unit-testable without a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ScreenRegions {
    /// Card rects in focus order.
    pub cards: Vec<Rect>,
    /// Interactive row rects in selection order (control rows, profile
    /// rows). Empty for display-only screens.
    pub rows: Vec<Rect>,
}

fn contains(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height
}

/// Row rect inside a card inner area at a content line offset.
pub(crate) fn row_rect(inner: Rect, line: usize) -> Rect {
    Rect {
        x: inner.x,
        y: inner.y + line as u16,
        width: inner.width,
        height: 1,
    }
}

/// Hit-tests a point against screen regions.
pub(crate) fn hit_row(rows: &[Rect], col: u16, row: u16) -> Option<usize> {
    rows.iter().position(|area| contains(*area, col, row))
}

/// Hit-tests a point against card bodies.
pub(crate) fn hit_card(cards: &[Rect], col: u16, row: u16) -> Option<usize> {
    cards.iter().position(|area| contains(*area, col, row))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_literals_match_mouse_measurements() {
        // The mouse module measures these exact strings; any edit here
        // must land there too or hit regions drift from labels.
        assert_eq!(FOOTER_STATUS_READY, " ✓ READY  ");
        assert_eq!(FOOTER_STATUS_READ_ONLY, " ✓ READ-ONLY  ");
        assert_eq!(FOOTER_HELP, "Help ");
        assert_eq!(FOOTER_QUIT, "Quit ");
        assert_eq!(FOOTER_Q_KEY, " [Q] ");
        assert_eq!(
            FOOTER_FIXED,
            &[
                "│ ",
                " [1-9] ",
                "Select ",
                " [↑↓] ",
                "Navigate ",
                " [Enter] ",
                "Open ",
                " [?] ",
            ]
        );
    }

    #[test]
    fn clock_renders_eight_char_time() {
        let clock = super::clock();
        assert_eq!(clock.len(), 8);
        assert_eq!(clock.chars().filter(|c| *c == ':').count(), 2);
    }

    #[test]
    fn bars_clamp_without_panic() {
        let theme = Theme::default();
        let _ = bar_line(2.0, 4, &theme);
        let _ = bar_line(-1.0, 4, &theme);
        let _ = bar_line(0.5, 0, &theme);
        let _ = window_bar(0, 100, 0, &theme);
    }
}
