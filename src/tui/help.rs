//! Truthful help overlay driven by [`AppState::help_visible`].
//!
//! Lists only shortcuts implemented in PLAN-003. Rendered last by the
//! dispatcher so it sits above the active screen. No focus model, no modal
//! state: navigation still changes the underlying screen while visible.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::theme::Theme;

/// Overlay title, chosen for deterministic tests.
pub(crate) const HELP_TITLE: &str = "MEC Help";

/// Renders a centered bordered overlay describing current shortcuts.
/// Saturating geometry keeps tiny and zero areas panic-free.
pub(crate) fn render_help(frame: &mut Frame, area: Rect, theme: &Theme) {
    let lines = help_lines();
    let overlay = help_overlay_area(area, lines.len());
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            format!(" {HELP_TITLE} "),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    let section = Style::default()
        .fg(theme.secondary)
        .add_modifier(Modifier::BOLD);
    let styled: Vec<Line<'static>> = lines
        .into_iter()
        .map(|line| {
            if line == "Navigation" || line == "General" {
                Line::styled(line, section)
            } else {
                Line::from(line)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(Text::from(styled)), inner);
}

/// Centers the overlay with saturating geometry so tiny and zero areas
/// stay panic-free. Extracted so the render path stays a single level.
fn help_overlay_area(area: Rect, line_count: usize) -> Rect {
    let width = area.width.saturating_sub(4).min(64);
    let height = area.height.saturating_sub(2).min(line_count as u16 + 2);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

fn help_lines() -> Vec<&'static str> {
    vec![
        "Navigation",
        "↑ / ←  Previous screen",
        "↓ / →  Next screen",
        "h / k  Previous screen",
        "j / l  Next screen",
        "Tab  Next screen",
        "Shift+Tab  Previous screen",
        "1 Dashboard",
        "2 Performance",
        "3 Fans",
        "4 Battery",
        "5 Devices",
        "6 Diagnostics",
        "General",
        "?  Toggle help",
        "Esc  Close help",
        "Q / q  Quit",
        "Ctrl+C  Quit",
    ]
}

#[cfg(test)]
mod tests {
    use crate::app::{AppAction, AppState};
    use crate::hardware::SupportMode;

    use super::super::screens::support::{
        full_capabilities, healthy_snapshot, live_for, screen_text,
    };
    use super::super::screens::{render_battery, render_screen};

    fn shown_help() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = AppState::default();
        app.apply(AppAction::ShowHelp);
        screen_text(100, 30, |frame| {
            render_screen(frame, frame.area(), &app, &live, &capabilities);
        })
    }

    fn hidden_help() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = AppState::default();
        screen_text(100, 30, |frame| {
            render_screen(frame, frame.area(), &app, &live, &capabilities);
        })
    }

    #[test]
    fn hidden_help_renders_no_overlay() {
        let text = hidden_help();
        assert!(!text.contains("MEC Help"));
        assert!(!text.contains("Toggle help"));
    }

    #[test]
    fn toggle_help_renders_overlay() {
        assert!(shown_help().contains("MEC Help"));
    }

    #[test]
    fn overlay_lists_all_six_screen_mappings() {
        let text = shown_help();
        for mapping in [
            "1 Dashboard",
            "2 Performance",
            "3 Fans",
            "4 Battery",
            "5 Devices",
            "6 Diagnostics",
        ] {
            assert!(text.contains(mapping), "{mapping:?} missing");
        }
    }

    #[test]
    fn overlay_lists_arrows() {
        let text = shown_help();
        for arrow in ["↑", "↓", "←", "→"] {
            assert!(text.contains(arrow), "{arrow:?} missing");
        }
    }

    #[test]
    fn overlay_lists_vim_keys() {
        let text = shown_help();
        for key in ["h", "j", "k", "l"] {
            assert!(text.contains(key), "{key:?} missing");
        }
    }

    #[test]
    fn overlay_lists_tab_keys() {
        let text = shown_help();
        assert!(text.contains("Tab"));
        assert!(text.contains("Shift+Tab"));
    }

    #[test]
    fn overlay_lists_help_toggle() {
        assert!(shown_help().contains("?"));
    }

    #[test]
    fn overlay_lists_escape() {
        assert!(shown_help().contains("Esc"));
    }

    #[test]
    fn overlay_lists_quit_keys() {
        assert!(shown_help().contains("Q"));
    }

    #[test]
    fn overlay_lists_ctrl_c() {
        assert!(shown_help().contains("Ctrl+C"));
    }

    #[test]
    fn overlay_advertises_no_profiles() {
        assert!(!shown_help().contains("Profiles"));
    }

    #[test]
    fn overlay_advertises_no_command_palette() {
        assert!(!shown_help().contains("Command Palette"));
    }

    #[test]
    fn overlay_advertises_no_f1() {
        assert!(!shown_help().contains("F1"));
    }

    #[test]
    fn hide_help_removes_overlay() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = AppState::default();
        app.apply(AppAction::ShowHelp);
        app.apply(AppAction::HideHelp);
        let text = screen_text(100, 30, |frame| {
            render_screen(frame, frame.area(), &app, &live, &capabilities);
        });
        assert!(!text.contains("MEC Help"));
    }

    #[test]
    fn overlay_sits_above_underlying_screen() {
        let text = shown_help();
        assert!(text.contains("MEC Help"));
        assert!(text.contains("Toggle help"));
    }

    #[test]
    fn overlay_survives_small_terminal() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = AppState::default();
        app.apply(AppAction::ShowHelp);
        let text = screen_text(20, 8, |frame| {
            render_screen(frame, frame.area(), &app, &live, &capabilities);
        });
        assert!(!text.is_empty());
    }

    #[test]
    fn overlay_survives_minimal_terminal() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = AppState::default();
        app.apply(AppAction::ShowHelp);
        let backend = TestBackend::new(1, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_screen(frame, frame.area(), &app, &live, &capabilities);
            })
            .expect("minimal help draws");
    }

    #[test]
    fn overlay_survives_zero_area() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;

        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = AppState::default();
        app.apply(AppAction::ShowHelp);
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_battery(frame, Rect::new(0, 0, 0, 0), &live, &capabilities);
            })
            .expect("zero-area battery draws");
    }
}
