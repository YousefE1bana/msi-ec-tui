//! Non-mutating command palette model and overlay.
//!
//! [`CommandPalette`] owns open/closed state plus the selected row index
//! only: no filesystem handles, no hardware handles, no execution
//! callbacks. Every command is navigation or overlay management; the
//! palette intentionally offers no hardware-changing actions, so opening
//! it can never initiate a mutation.
//!
//! Stable command order: the seven screens in canonical `1..7` order,
//! then Notifications, Clear Notifications, Help, Quit, then one theme row
//! per [`ThemeName`](super::theme::ThemeName) in stable identity order.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::Screen;

use super::theme::Theme;

/// One palette row. Screen rows mirror digit navigation; the rest manage
/// overlays or quit. Deliberately no mutation commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    /// Jump to Dashboard (same as `1`).
    GoDashboard,
    /// Jump to Performance (same as `2`).
    GoPerformance,
    /// Jump to Fans (same as `3`).
    GoFans,
    /// Jump to Battery (same as `4`).
    GoBattery,
    /// Jump to Devices (same as `5`).
    GoDevices,
    /// Jump to Profiles (same as `6`).
    GoProfiles,
    /// Jump to Diagnostics (same as `7`).
    GoDiagnostics,
    /// Open the read-only notification history overlay.
    Notifications,
    /// Clear notification history and close the palette.
    ClearNotifications,
    /// Close the palette, then open Help.
    Help,
    /// Close the palette, then request application quit.
    Quit,
    /// Close the palette and switch to the MSI Dark theme.
    ThemeMsiDark,
    /// Close the palette and switch to the Terminal theme.
    ThemeTerminal,
    /// Close the palette and switch to the Light theme.
    ThemeLight,
}

impl PaletteCommand {
    /// All commands in stable display order.
    pub const ALL: [PaletteCommand; 14] = [
        PaletteCommand::GoDashboard,
        PaletteCommand::GoPerformance,
        PaletteCommand::GoFans,
        PaletteCommand::GoBattery,
        PaletteCommand::GoDevices,
        PaletteCommand::GoProfiles,
        PaletteCommand::GoDiagnostics,
        PaletteCommand::Notifications,
        PaletteCommand::ClearNotifications,
        PaletteCommand::Help,
        PaletteCommand::Quit,
        PaletteCommand::ThemeMsiDark,
        PaletteCommand::ThemeTerminal,
        PaletteCommand::ThemeLight,
    ];

    /// Stable user-facing label.
    pub fn label(self) -> &'static str {
        match self {
            PaletteCommand::GoDashboard => "Dashboard",
            PaletteCommand::GoPerformance => "Performance",
            PaletteCommand::GoFans => "Fans",
            PaletteCommand::GoBattery => "Battery",
            PaletteCommand::GoDevices => "Devices",
            PaletteCommand::GoProfiles => "Profiles",
            PaletteCommand::GoDiagnostics => "Diagnostics",
            PaletteCommand::Notifications => "Notifications",
            PaletteCommand::ClearNotifications => "Clear Notifications",
            PaletteCommand::Help => "Help",
            PaletteCommand::Quit => "Quit",
            PaletteCommand::ThemeMsiDark => "Theme: MSI Dark",
            PaletteCommand::ThemeTerminal => "Theme: Terminal",
            PaletteCommand::ThemeLight => "Theme: Light",
        }
    }

    /// Theme destination for theme rows; `None` otherwise.
    pub fn theme(self) -> Option<super::theme::ThemeName> {
        match self {
            PaletteCommand::ThemeMsiDark => Some(super::theme::ThemeName::MsiDark),
            PaletteCommand::ThemeTerminal => Some(super::theme::ThemeName::Terminal),
            PaletteCommand::ThemeLight => Some(super::theme::ThemeName::Light),
            _ => None,
        }
    }

    /// Screen destination for navigation rows; `None` otherwise.
    pub fn screen(self) -> Option<Screen> {
        match self {
            PaletteCommand::GoDashboard => Some(Screen::Dashboard),
            PaletteCommand::GoPerformance => Some(Screen::Performance),
            PaletteCommand::GoFans => Some(Screen::Fans),
            PaletteCommand::GoBattery => Some(Screen::Battery),
            PaletteCommand::GoDevices => Some(Screen::Devices),
            PaletteCommand::GoProfiles => Some(Screen::Profiles),
            PaletteCommand::GoDiagnostics => Some(Screen::Diagnostics),
            PaletteCommand::Notifications
            | PaletteCommand::ClearNotifications
            | PaletteCommand::Help
            | PaletteCommand::Quit
            | PaletteCommand::ThemeMsiDark
            | PaletteCommand::ThemeTerminal
            | PaletteCommand::ThemeLight => None,
        }
    }
}

/// Open/closed state plus selected row. Clamp-safe: selection wraps
/// deterministically and can never index out of bounds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandPalette {
    open: bool,
    selected: usize,
}

impl CommandPalette {
    /// Whether the overlay is visible.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the overlay, keeping the current selection.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Closes the overlay, keeping the selection for next time.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Selected row index.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Selected command (index always in range via wrapping moves).
    pub fn selected(&self) -> PaletteCommand {
        PaletteCommand::ALL[self.selected % PaletteCommand::ALL.len()]
    }

    /// Moves to the next row, wrapping past the last row to the first.
    pub fn move_down(&mut self) {
        self.selected = (self.selected + 1) % PaletteCommand::ALL.len();
    }

    /// Moves to the previous row, wrapping past the first row to the last.
    pub fn move_up(&mut self) {
        self.selected = (self.selected + PaletteCommand::ALL.len() - 1) % PaletteCommand::ALL.len();
    }
}

/// Centered overlay geometry with saturating math so tiny and zero areas
/// stay panic-free.
fn overlay_area(area: Rect, line_count: usize) -> Rect {
    let width = area.width.saturating_sub(4).min(48);
    let height = area.height.saturating_sub(2).min(line_count as u16 + 2);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

/// Renders the palette above the underlying screen. The selected row uses
/// the semantic primary role plus bold with a `>` marker. Safe for tiny
/// and zero areas.
pub(crate) fn render_palette(
    frame: &mut Frame,
    area: Rect,
    palette: &CommandPalette,
    theme: &Theme,
) {
    let rows: Vec<Line<'static>> = PaletteCommand::ALL
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let text = command.label().to_owned();
            if index == palette.selected_index() % PaletteCommand::ALL.len() {
                Line::styled(
                    format!("> {text}"),
                    Style::default()
                        .fg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Line::from(format!("  {text}"))
            }
        })
        .collect();
    let overlay = overlay_area(area, rows.len());
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(theme.base_style())
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            " Command Palette ".to_owned(),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    frame.render_widget(
        Paragraph::new(Text::from(rows)).style(theme.base_style()),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_by_default() {
        assert!(!CommandPalette::default().is_open());
    }

    #[test]
    fn initial_selection_is_first_row() {
        assert_eq!(
            CommandPalette::default().selected(),
            PaletteCommand::GoDashboard
        );
        assert_eq!(CommandPalette::default().selected_index(), 0);
    }

    #[test]
    fn open_and_close() {
        let mut palette = CommandPalette::default();
        palette.open();
        assert!(palette.is_open());
        palette.close();
        assert!(!palette.is_open());
    }

    #[test]
    fn move_down_traverses_all_rows_then_wraps() {
        let mut palette = CommandPalette::default();
        for expected in 1..PaletteCommand::ALL.len() {
            palette.move_down();
            assert_eq!(palette.selected_index(), expected);
        }
        palette.move_down();
        assert_eq!(palette.selected_index(), 0);
        assert_eq!(palette.selected(), PaletteCommand::GoDashboard);
    }

    #[test]
    fn move_up_wraps_from_first_to_last() {
        let mut palette = CommandPalette::default();
        palette.move_up();
        assert_eq!(palette.selected_index(), PaletteCommand::ALL.len() - 1);
        assert_eq!(palette.selected(), PaletteCommand::ThemeLight);
        palette.move_up();
        assert_eq!(palette.selected(), PaletteCommand::ThemeTerminal);
    }

    #[test]
    fn command_order_is_stable() {
        let labels: Vec<&str> = PaletteCommand::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Dashboard",
                "Performance",
                "Fans",
                "Battery",
                "Devices",
                "Profiles",
                "Diagnostics",
                "Notifications",
                "Clear Notifications",
                "Help",
                "Quit",
                "Theme: MSI Dark",
                "Theme: Terminal",
                "Theme: Light",
            ]
        );
    }

    #[test]
    fn screen_rows_mirror_digit_navigation() {
        let cases = [
            (PaletteCommand::GoDashboard, Screen::Dashboard),
            (PaletteCommand::GoPerformance, Screen::Performance),
            (PaletteCommand::GoFans, Screen::Fans),
            (PaletteCommand::GoBattery, Screen::Battery),
            (PaletteCommand::GoDevices, Screen::Devices),
            (PaletteCommand::GoProfiles, Screen::Profiles),
            (PaletteCommand::GoDiagnostics, Screen::Diagnostics),
        ];
        for (command, screen) in cases {
            assert_eq!(command.screen(), Some(screen));
        }
    }

    #[test]
    fn utility_rows_have_no_screen() {
        for command in [
            PaletteCommand::Notifications,
            PaletteCommand::ClearNotifications,
            PaletteCommand::Help,
            PaletteCommand::Quit,
            PaletteCommand::ThemeMsiDark,
            PaletteCommand::ThemeTerminal,
            PaletteCommand::ThemeLight,
        ] {
            assert_eq!(command.screen(), None);
        }
    }

    #[test]
    fn theme_rows_map_to_theme_identities() {
        use super::super::theme::ThemeName;
        assert_eq!(
            PaletteCommand::ThemeMsiDark.theme(),
            Some(ThemeName::MsiDark)
        );
        assert_eq!(
            PaletteCommand::ThemeTerminal.theme(),
            Some(ThemeName::Terminal)
        );
        assert_eq!(PaletteCommand::ThemeLight.theme(), Some(ThemeName::Light));
        assert_eq!(PaletteCommand::GoFans.theme(), None);
        assert_eq!(PaletteCommand::Quit.theme(), None);
    }

    #[test]
    fn palette_offers_no_mutation_commands() {
        for command in PaletteCommand::ALL {
            let label = command.label().to_lowercase();
            for forbidden in ["apply", "set ", "boost", "battery limit", "fan mode"] {
                // "Battery"/"Fans" screen names are navigation, not writes;
                // mutation verbs must never appear.
                assert!(!label.contains(forbidden), "{label:?}");
            }
        }
    }
}
