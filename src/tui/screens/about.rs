//! Product information and explicit, read-only update discovery.
use crate::app::{AppAction, AppState, LiveHardware};
use crate::hardware::EcBackend;
use crate::tui::{shell, theme::Theme};
use crate::updates::UpdateStatus;
use ratatui::{Frame, layout::Rect, style::Style, text::Line, widgets::Paragraph};

fn cards(area: Rect) -> [Rect; 3] {
    let (_, workspace, _) = shell::shell_split(area);
    let (top, update) = shell::vsplit2(workspace, 55);
    let (info, project) = shell::hpair(top, 48);
    [info, project, update]
}

/// Shared visible actions. Small/empty viewports have no invisible hit regions.
pub(crate) fn buttons(area: Rect) -> Vec<(Rect, AppAction)> {
    if area.is_empty() {
        return Vec::new();
    }
    let regions = cards(area);
    let inner = shell::inset(regions[2]);
    let mut buttons = Vec::new();
    if inner.height >= 4 && inner.width >= 42 {
        let y = inner.y + inner.height - 1;
        buttons.push((Rect::new(inner.x, y, 20, 1), AppAction::CheckUpdates));
        buttons.push((Rect::new(inner.x + 23, y, 8, 1), AppAction::HideAbout));
    }
    for (index, rect) in regions.into_iter().enumerate() {
        if !rect.is_empty() {
            buttons.push((rect, AppAction::FocusCard(index)));
        }
    }
    buttons
}

pub(crate) fn render<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme.base_style()),
        area,
    );
    let (top, _, footer) = shell::shell_split(area);
    shell::render_top_strip(frame, top, live, theme);
    shell::render_bottom_strip(frame, footer, live, theme);
    let regions = cards(area);
    let info = shell::card(
        frame,
        regions[0],
        "ABOUT MEC",
        app.focused_card() == 0,
        theme,
    );
    let project = shell::card(frame, regions[1], "PROJECT", app.focused_card() == 1, theme);
    let inner = shell::card(frame, regions[2], "UPDATES", app.focused_card() == 2, theme);
    let status = match &app.update_status {
        UpdateStatus::NotChecked => "Not checked · no startup network request".into(),
        UpdateStatus::Checking => "Checking latest stable release…".into(),
        UpdateStatus::UpToDate(v) => format!("Up to date · latest stable {v}"),
        UpdateStatus::Available(v) => format!("Update available · latest stable {v}"),
        UpdateStatus::CurrentNewer(v) => format!("Current version is newer than stable {v}"),
        UpdateStatus::Failed(error) => format!("Check failed · {error}"),
    };
    let status_color = match app.update_status {
        UpdateStatus::UpToDate(_) => theme.success,
        UpdateStatus::Failed(_) => theme.danger,
        UpdateStatus::Available(_) | UpdateStatus::Checking => theme.warning,
        _ => theme.muted,
    };
    let device = live.device();
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                "[ MEC ] · MSI EC Control Center",
                Style::default().fg(theme.accent),
            ),
            Line::from(format!("Version: {}", env!("CARGO_PKG_VERSION"))),
            Line::from(format!(
                "Mode: {}",
                crate::tui::ui::support_mode_text(live.mode())
            )),
            Line::from(format!("Device: {}", device.product_name)),
            Line::from(format!(
                "EC: {}",
                device.ec_firmware_version.as_deref().unwrap_or("N/A")
            )),
            Line::styled(
                "Writes also require OS permission",
                Style::default().fg(theme.muted),
            ),
        ])
        .style(shell::card_style(theme)),
        info,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Created by: Yousef Osama"),
            Line::from("github.com/YousefE1bana"),
            Line::from("github.com/YousefE1bana/msi-ec-tui"),
            Line::from("License: MIT · Language: Rust"),
            Line::styled(
                "Driver: BeardOverflow/msi-ec",
                Style::default().fg(theme.muted),
            ),
        ])
        .style(shell::card_style(theme)),
        project,
    );
    let lines = vec![
        Line::from(format!("Current version: {}", env!("CARGO_PKG_VERSION"))),
        Line::styled(status, Style::default().fg(status_color)),
        Line::from("Explicit check · GitHub only · timeout 5 seconds"),
        Line::from("No telemetry sent. No automatic installation."),
        Line::from(crate::updates::RELEASES),
    ];
    let content = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    frame.render_widget(
        Paragraph::new(lines).style(shell::card_style(theme)),
        content,
    );
    for (region, action) in buttons(area) {
        let label = match action {
            AppAction::CheckUpdates => "[ CHECK FOR UPDATES ]",
            AppAction::HideAbout => "[ BACK ]",
            _ => continue,
        };
        frame.render_widget(
            Paragraph::new(label).style(Style::default().fg(theme.accent).bg(theme.surface)),
            region,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responsive_buttons_match_visible_actions_and_empty_area() {
        for (w, h) in [(160, 50), (120, 35), (100, 30), (80, 24)] {
            let area = Rect::new(0, 0, w, h);
            let (live, _) = super::super::support::live_for(
                vec![Ok(super::super::support::healthy_snapshot())],
                crate::hardware::SupportMode::Ready,
                1,
            );
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            let app = AppState::default();
            terminal
                .draw(|frame| render(frame, area, &app, &live, &Theme::default()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|c| c.symbol()).collect();
            for expected in [
                "ABOUT MEC",
                env!("CARGO_PKG_VERSION"),
                "Not checked",
                "CHECK FOR UPDATES",
                "No telemetry",
            ] {
                assert!(text.contains(expected), "{w}x{h}: {expected}");
            }
            for (rect, _) in buttons(area) {
                assert!(rect.right() <= w && rect.bottom() <= h);
            }
        }
        assert!(buttons(Rect::default()).is_empty());
    }
    #[test]
    fn compact_tiny_and_zero_about_render_without_invisible_update_buttons() {
        let (live, _) = super::super::support::live_for(
            vec![Ok(super::super::support::healthy_snapshot())],
            crate::hardware::SupportMode::Ready,
            1,
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
        for area in [
            Rect::new(0, 0, 50, 12),
            Rect::new(0, 0, 20, 5),
            Rect::default(),
        ] {
            terminal
                .draw(|frame| render(frame, area, &AppState::default(), &live, &Theme::default()))
                .unwrap();
            assert!(
                buttons(area)
                    .iter()
                    .all(|(_, action)| *action != AppAction::CheckUpdates)
            );
        }
    }
    #[test]
    fn about_clears_underlying_settings_text_and_borders() {
        let (live, _) = super::super::support::live_for(
            vec![Ok(super::super::support::healthy_snapshot())],
            crate::hardware::SupportMode::Ready,
            1,
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
        terminal
            .draw(|frame| {
                super::super::settings::render_settings(
                    frame,
                    frame.area(),
                    &live,
                    &crate::config::AppConfig::default(),
                );
                render(
                    frame,
                    frame.area(),
                    &AppState::default(),
                    &live,
                    &Theme::default(),
                );
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("ABOUT MEC"));
        for stale in [
            "Refresh interval",
            "Vim keys",
            "DISPLAY",
            "NOTE",
            "INTERFACE",
        ] {
            assert!(!text.contains(stale), "stale Settings text: {stale}");
        }
    }
}
