//! Presentation-driven mouse input. Regions produce semantic actions only.
//! Editing, review, cancellation and confirmation remain owned by TuiApp.
//! This module never creates command intent or calls an executor.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{AppAction, Screen};

use super::responsive::{LayoutTier, layout_tier};

/// Approved menu rows in order: the seven production screens, a Settings
/// entry, and Exit. All eight production screens have their own route.
pub const MENU_LABELS: [&str; 9] = [
    "Dashboard",
    "Performance",
    "Fans",
    "Battery",
    "Devices",
    "Profiles",
    "Diagnostics",
    "Settings",
    "Exit",
];

/// Index of the Settings row inside [`MENU_LABELS`].
pub const SETTINGS_ROW: usize = 7;
/// Index of the Exit row inside [`MENU_LABELS`].
pub const EXIT_ROW: usize = 8;

/// Content lines above the first menu row inside the control card:
/// author, homepage, separator, options header.
pub(crate) const MENU_FIRST_ROW_OFFSET: u16 = 4;

/// Minimum workspace width for the side-by-side card grid. Narrower
/// terminals stack the six cards vertically.
const WIDE_GRID_WIDTH: u16 = 110;

/// Clickable geometry for one dashboard frame. Pure data so mapping stays
/// unit-testable without a terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DashboardRegions {
    /// Nine full-width menu row rects in [`MENU_LABELS`] order.
    pub menu_rows: Vec<Rect>,
    /// Six card rects in focus order: control, thermals, cooling, power,
    /// performance, device.
    pub cards: Vec<Rect>,
    /// Footer Help label rect.
    pub foot_help: Rect,
    /// Footer Quit label rect.
    pub foot_quit: Rect,
}

/// Footer label geometry shared by the renderer and the hit tester.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FooterRegions {
    /// Existing Select label opens the non-mutating command palette.
    pub palette: Rect,
    /// Help label rect.
    pub help: Rect,
    /// Quit label rect.
    pub quit: Rect,
}

pub(crate) fn contains(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x && col < area.right() && row >= area.y && row < area.bottom()
}

/// Dashboard content area: the v1.1 dashboard owns the full frame (the
/// legacy navigation row stays hidden there), so hit-testing uses the
/// frame area directly.
pub fn dashboard_area(full: Rect) -> Rect {
    full
}

/// Splits the dashboard area into top strip, workspace, and footer.
/// Mirrors the renderer shell exactly.
pub fn dashboard_shell(area: Rect) -> (Rect, Rect, Rect) {
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

/// Computes footer Help/Quit label rects for `footer` (the shell footer
/// row). `read_only` selects the READ-ONLY status width so rects match the
/// drawn spans exactly. Widths derive from [`super::shell`] literals so
/// labels and regions can never drift apart.
pub fn footer_regions(footer: Rect, read_only: bool) -> FooterRegions {
    use super::shell::{
        FOOTER_FIXED, FOOTER_HELP, FOOTER_Q_KEY, FOOTER_QUIT, FOOTER_SELECT,
        FOOTER_STATUS_READ_ONLY, FOOTER_STATUS_READY,
    };
    let status = if read_only {
        FOOTER_STATUS_READ_ONLY
    } else {
        FOOTER_STATUS_READY
    };
    let mut x = footer.x + ratatui::text::Line::raw(status).width() as u16;
    let mut palette = Rect::default();
    for part in FOOTER_FIXED {
        if *part == FOOTER_SELECT {
            palette = super::shell::text_region(footer, x.saturating_sub(footer.x), part);
        }
        x += ratatui::text::Line::raw(*part).width() as u16;
    }
    let help = Rect {
        x,
        y: footer.y,
        width: FOOTER_HELP.len() as u16,
        height: 1,
    };
    x += help.width + FOOTER_Q_KEY.len() as u16;
    let quit = Rect {
        x,
        y: footer.y,
        width: FOOTER_QUIT.len() as u16,
        height: 1,
    };
    FooterRegions {
        palette,
        help: super::shell::text_region(footer, help.x.saturating_sub(footer.x), FOOTER_HELP),
        quit: super::shell::text_region(footer, quit.x.saturating_sub(footer.x), FOOTER_QUIT),
    }
}

/// Computes dashboard hit regions for `full` (the whole frame). Returns
/// `None` outside the full tier or when the workspace cannot host rows:
/// mouse stays inert instead of guessing.
pub fn dashboard_regions(full: Rect) -> Option<DashboardRegions> {
    if layout_tier(full) != LayoutTier::Full {
        return None;
    }
    let area = dashboard_area(full);
    let (_top, workspace, footer) = dashboard_shell(area);
    if workspace.width == 0 || workspace.height == 0 {
        return None;
    }
    let cards = grid_cards(workspace);
    if cards.len() != 6 {
        return None;
    }
    // Menu rows live inside the control card (cards[0]) below its header
    // content. Border consumes one cell; content starts at inner.y.
    let inner = super::shell::inset(cards[0]);
    let menu_rows = (0..MENU_LABELS.len())
        .map(|i| super::shell::row_rect(inner, usize::from(MENU_FIRST_ROW_OFFSET) + i))
        .collect();
    let foot = footer_regions(footer, false);
    let foot_help = foot.help;
    let foot_quit = foot.quit;
    Some(DashboardRegions {
        menu_rows,
        cards,
        foot_help,
        foot_quit,
    })
}

/// Six card rects in focus order. Wide terminals use the approved
/// two-column grid; narrow terminals stack vertically. Shared with the
/// dashboard renderer so hit regions always match drawn cards.
pub(crate) fn grid_cards(workspace: Rect) -> Vec<Rect> {
    if workspace.width >= WIDE_GRID_WIDTH {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Ratio(1, 2),
                Constraint::Length(1),
                Constraint::Ratio(1, 2),
            ])
            .split(workspace);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Percentage(32),
                Constraint::Percentage(30),
            ])
            .split(cols[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(38),
                Constraint::Percentage(32),
                Constraint::Percentage(30),
            ])
            .split(cols[2]);
        vec![left[0], right[0], left[1], right[1], left[2], right[2]]
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
                Constraint::Ratio(1, 6),
            ])
            .split(workspace)
            .to_vec()
    }
}

/// Browsing and palette mapping. Mutation-related regions are resolved by
/// TuiApp only after enforcing the same overlay precedence as keyboard input.
pub(crate) fn navigation_action(
    full: Rect,
    current: Screen,
    palette_open: bool,
    profile_view: (usize, usize),
    read_only: bool,
    event: MouseEvent,
) -> Option<AppAction> {
    if layout_tier(full) != LayoutTier::Full {
        return None;
    }
    let area = dashboard_area(full);
    let (_top, workspace, footer) = super::shell::shell_split(area);
    if workspace.width == 0 || workspace.height == 0 {
        return None;
    }
    let col = event.column;
    let row = event.row;
    // Footer shortcuts work on every screen; the overlay never covers
    // the footer row by construction.
    let foot = footer_regions(footer, read_only);
    let foot_help = foot.help;
    let foot_quit = foot.quit;
    if palette_open {
        return match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if contains(foot_help, col, row) {
                    // Dropped by palette precedence, like keyboard `?`.
                    Some(AppAction::ToggleHelp)
                } else if contains(foot_quit, col, row) {
                    Some(AppAction::Quit)
                } else {
                    palette_row_at(full, col, row).map(AppAction::ActivatePaletteRow)
                }
            }
            MouseEventKind::ScrollUp
                if contains(
                    super::palette::overlay_area(full, super::palette::PaletteCommand::ALL.len()),
                    col,
                    row,
                ) =>
            {
                Some(AppAction::MoveUp)
            }
            MouseEventKind::ScrollDown
                if contains(
                    super::palette::overlay_area(full, super::palette::PaletteCommand::ALL.len()),
                    col,
                    row,
                ) =>
            {
                Some(AppAction::MoveDown)
            }
            _ => None,
        };
    }
    if event.kind == MouseEventKind::Down(MouseButton::Left) && contains(foot.palette, col, row) {
        return Some(AppAction::TogglePalette);
    }
    if current == Screen::Dashboard {
        return dashboard_action(full, col, row, event.kind, foot_help, foot_quit);
    }
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if contains(foot_help, col, row) {
                return Some(AppAction::ToggleHelp);
            }
            if contains(foot_quit, col, row) {
                return Some(AppAction::Quit);
            }
            let regions = screen_regions(current, workspace, profile_view.0)?;
            if let Some(index) = super::shell::hit_row(&regions.rows, col, row) {
                return Some(match current {
                    Screen::Profiles => AppAction::SelectProfileRow(
                        index
                            + super::screens::profiles::visible_row_start(
                                workspace,
                                profile_view.0,
                                profile_view.1,
                            ),
                    ),
                    _ => AppAction::SelectControlRow(index),
                });
            }
            if let Some(index) = super::shell::hit_card(&regions.cards, col, row) {
                return Some(AppAction::FocusCard(index));
            }
            None
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let regions = screen_regions(current, workspace, profile_view.0)?;
            let over_rows = regions.rows.iter().any(|area| contains(*area, col, row));
            if over_rows && matches!(current, Screen::Profiles) {
                // Profile rows move like keyboard arrows on Profiles.
                return Some(match event.kind {
                    MouseEventKind::ScrollUp => AppAction::MoveUp,
                    _ => AppAction::MoveDown,
                });
            }
            if over_rows && is_control_screen(current) {
                return Some(match event.kind {
                    MouseEventKind::ScrollUp => AppAction::MoveUp,
                    _ => AppAction::MoveDown,
                });
            }
            None
        }
        _ => None,
    }
}

/// Hit only visible buttons; right click, drag, wheel and empty regions are inert.
pub(crate) fn button_action(regions: &[(Rect, AppAction)], event: MouseEvent) -> Option<AppAction> {
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    regions
        .iter()
        .find(|(rect, _)| contains(*rect, event.column, event.row))
        .map(|(_, action)| *action)
}

#[cfg(test)]
fn action_for_mouse(
    full: Rect,
    current: Screen,
    palette_open: bool,
    profile_view: (usize, usize),
    event: MouseEvent,
) -> Option<AppAction> {
    navigation_action(full, current, palette_open, profile_view, false, event)
}

/// Screen regions for mouse mapping: each migrated screen's shared
/// layout, so clicks land on the drawn rows and cards.
pub(crate) fn screen_regions(
    current: Screen,
    workspace: Rect,
    profile_rows: usize,
) -> Option<super::shell::ScreenRegions> {
    match current {
        Screen::Performance => Some(super::screens::performance::hit_regions(workspace)),
        Screen::Fans => Some(super::screens::fans::hit_regions(workspace)),
        Screen::Battery => Some(super::screens::battery::hit_regions(workspace)),
        Screen::Devices => Some(super::screens::devices::hit_regions(workspace)),
        Screen::Profiles => Some(super::screens::profiles::hit_regions(
            workspace,
            profile_rows,
        )),
        Screen::Diagnostics => Some(super::screens::diagnostics::hit_regions(workspace)),
        Screen::Settings => Some(super::screens::settings::hit_regions(workspace)),
        Screen::Dashboard => None,
    }
}

fn is_control_screen(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::Performance | Screen::Fans | Screen::Battery | Screen::Devices
    )
}

/// Palette row under a point, if any. Geometry mirrors the overlay so
/// clicks always land on drawn rows.
fn palette_row_at(full: Rect, col: u16, row: u16) -> Option<usize> {
    use super::palette::{PaletteCommand, overlay_area};
    let overlay = overlay_area(full, PaletteCommand::ALL.len());
    let inner = Rect {
        x: overlay.x + 1,
        y: overlay.y + 1,
        width: overlay.width.saturating_sub(2),
        height: overlay.height.saturating_sub(2),
    };
    if !contains(inner, col, row) {
        return None;
    }
    let index = (row - inner.y) as usize;
    if index < PaletteCommand::ALL.len() {
        Some(index)
    } else {
        None
    }
}

fn dashboard_action(
    full: Rect,
    col: u16,
    row: u16,
    kind: MouseEventKind,
    foot_help: Rect,
    foot_quit: Rect,
) -> Option<AppAction> {
    let regions = dashboard_regions(full)?;
    match kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(index) = regions
                .menu_rows
                .iter()
                .position(|area| contains(*area, col, row))
            {
                return Some(menu_action(index));
            }
            if contains(foot_help, col, row) {
                return Some(AppAction::ToggleHelp);
            }
            if contains(foot_quit, col, row) {
                return Some(AppAction::Quit);
            }
            if let Some(index) = regions
                .cards
                .iter()
                .position(|area| contains(*area, col, row))
            {
                return Some(AppAction::FocusCard(index));
            }
            None
        }
        MouseEventKind::ScrollUp => {
            if in_menu_or_control(&regions, col, row) {
                Some(AppAction::MoveUp)
            } else {
                None
            }
        }
        MouseEventKind::ScrollDown => {
            if in_menu_or_control(&regions, col, row) {
                Some(AppAction::MoveDown)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Menu row for a menu index: production screens jump directly, Exit
/// quits. Digit and click share this mapping.
fn menu_action(index: usize) -> AppAction {
    match index {
        SETTINGS_ROW => AppAction::GoTo(Screen::Settings),
        EXIT_ROW => AppAction::Quit,
        _ => AppAction::GoTo(Screen::ALL[index.min(Screen::ALL.len() - 1)]),
    }
}

/// Wheel zone: menu rows or the control card body.
fn in_menu_or_control(regions: &DashboardRegions, col: u16, row: u16) -> bool {
    regions
        .menu_rows
        .iter()
        .any(|area| contains(*area, col, row))
        || regions
            .cards
            .first()
            .is_some_and(|area| contains(*area, col, row))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;

    use crate::app::{AppAction, Screen};

    use super::{
        EXIT_ROW, SETTINGS_ROW, dashboard_area, dashboard_regions, dashboard_shell, footer_regions,
    };
    use super::{action_for_mouse, menu_action};

    const FULL: Rect = Rect {
        x: 0,
        y: 0,
        width: 160,
        height: 50,
    };

    fn click(col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    fn wheel(up: bool, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: if up {
                MouseEventKind::ScrollUp
            } else {
                MouseEventKind::ScrollDown
            },
            column: col,
            row,
            modifiers: KeyModifiers::empty(),
        }
    }

    #[test]
    fn regions_cover_nine_menu_rows_and_six_cards() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        assert_eq!(regions.menu_rows.len(), 9);
        assert_eq!(regions.cards.len(), 6);
    }

    #[test]
    fn menu_rows_stack_vertically_inside_control_card() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        for pair in regions.menu_rows.windows(2) {
            assert_eq!(pair[1].y, pair[0].y + 1);
            assert_eq!(pair[1].x, pair[0].x);
        }
        assert!(regions.menu_rows.iter().all(|row| {
            let card = &regions.cards[0];
            row.x >= card.x
                && row.x + row.width <= card.x + card.width
                && row.y > card.y
                && row.y < card.y + card.height
        }));
    }

    #[test]
    fn menu_clicks_navigate_to_production_screens() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        for (index, screen) in Screen::ALL.iter().enumerate() {
            let row = &regions.menu_rows[index];
            assert_eq!(
                action_for_mouse(
                    FULL,
                    Screen::Dashboard,
                    false,
                    (5, 0),
                    click(row.x + 1, row.y)
                ),
                Some(AppAction::GoTo(*screen)),
                "row {index} must navigate"
            );
        }
    }

    #[test]
    fn settings_row_lands_on_settings() {
        assert_eq!(menu_action(SETTINGS_ROW), AppAction::GoTo(Screen::Settings));
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[SETTINGS_ROW];
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                click(row.x + 1, row.y)
            ),
            Some(AppAction::GoTo(Screen::Settings))
        );
    }

    #[test]
    fn exit_row_quits() {
        assert_eq!(menu_action(EXIT_ROW), AppAction::Quit);
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[EXIT_ROW];
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                click(row.x + 1, row.y)
            ),
            Some(AppAction::Quit)
        );
    }

    #[test]
    fn card_clicks_focus_without_navigating() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        for (index, card) in regions.cards.iter().enumerate() {
            // Bottom area of each card stays clear of menu rows (menu
            // rows sit at the top of the control card only).
            let col = card.x + 2;
            let row = card.y + card.height.saturating_sub(2);
            assert!(
                !regions
                    .menu_rows
                    .iter()
                    .any(|menu| { menu.x <= col && col < menu.x + menu.width && menu.y == row }),
                "probe point must avoid menu rows"
            );
            assert_eq!(
                action_for_mouse(FULL, Screen::Dashboard, false, (5, 0), click(col, row)),
                Some(AppAction::FocusCard(index)),
                "card {index} must focus"
            );
        }
    }

    #[test]
    fn menu_row_wins_over_card_body() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[2];
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                click(row.x + 1, row.y)
            ),
            Some(AppAction::GoTo(Screen::Fans))
        );
    }

    #[test]
    fn footer_clicks_toggle_help_and_quit() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                click(regions.foot_help.x + 1, regions.foot_help.y)
            ),
            Some(AppAction::ToggleHelp)
        );
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                click(regions.foot_quit.x + 1, regions.foot_quit.y)
            ),
            Some(AppAction::Quit)
        );
    }

    #[test]
    fn footer_rects_match_drawn_labels() {
        let area = super::dashboard_area(FULL);
        let (_top, _workspace, footer) = dashboard_shell(area);
        let regions = footer_regions(footer, false);
        // Help label starts after the fixed segments; Quit follows Help+[Q].
        assert!(regions.help.x > footer.x);
        assert!(regions.quit.x > regions.help.x);
        assert_eq!(regions.help.y, footer.y);
        assert_eq!(regions.quit.y, footer.y);
    }

    #[test]
    fn wheel_over_menu_moves_like_arrows() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[0];
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                wheel(true, row.x + 1, row.y)
            ),
            Some(AppAction::MoveUp)
        );
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                wheel(false, row.x + 1, row.y)
            ),
            Some(AppAction::MoveDown)
        );
    }

    #[test]
    fn wheel_outside_menu_is_inert() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        let card = &regions.cards[1];
        let col = card.x + 2;
        let row = card.y + 2;
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                wheel(true, col, row)
            ),
            None
        );
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                wheel(false, col, row)
            ),
            None
        );
    }

    #[test]
    fn control_row_clicks_select_without_editing() {
        // Clicking a control row enters keyboard-identical selection
        // state: no editor opens, nothing stages, nothing executes.
        for (screen, rows) in [
            (Screen::Performance, 4),
            (Screen::Fans, 2),
            (Screen::Battery, 2),
            (Screen::Devices, 5),
        ] {
            let area = super::super::shell::shell_split(FULL).1;
            let regions = super::screen_regions(screen, area, 5).expect("screen maps");
            assert_eq!(regions.rows.len(), rows, "{screen:?} row count");
            let row = &regions.rows[0];
            assert_eq!(
                action_for_mouse(FULL, screen, false, (5, 0), click(row.x + 1, row.y)),
                Some(AppAction::SelectControlRow(0)),
                "{screen:?} must select row 0"
            );
            let last = &regions.rows[rows - 1];
            assert_eq!(
                action_for_mouse(FULL, screen, false, (5, 0), click(last.x + 1, last.y)),
                Some(AppAction::SelectControlRow(rows - 1)),
                "{screen:?} must select last row"
            );
        }
    }

    #[test]
    fn profile_row_clicks_select_without_applying() {
        let area = super::super::shell::shell_split(FULL).1;
        let regions = super::screen_regions(Screen::Profiles, area, 5).expect("profiles maps");
        assert_eq!(regions.rows.len(), 5);
        let row = &regions.rows[2];
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Profiles,
                false,
                (5, 0),
                click(row.x + 1, row.y)
            ),
            Some(AppAction::SelectProfileRow(2))
        );
    }

    #[test]
    fn display_screens_map_cards_only() {
        // Diagnostics and Settings carry no rows: only card focus and
        // footer shortcuts map.
        for screen in [Screen::Diagnostics, Screen::Settings] {
            let area = super::super::shell::shell_split(FULL).1;
            let regions = super::screen_regions(screen, area, 5).expect("screen maps");
            assert!(regions.rows.is_empty(), "{screen:?} has no rows");
            let card = &regions.cards[0];
            assert_eq!(
                action_for_mouse(FULL, screen, false, (5, 0), click(card.x + 2, card.y + 2)),
                Some(AppAction::FocusCard(0))
            );
        }
    }

    #[test]
    fn palette_clicks_activate_through_existing_path() {
        use super::super::palette::{PaletteCommand, overlay_area};
        let overlay = overlay_area(FULL, PaletteCommand::ALL.len());
        let row_y = overlay.y + 1 + 2;
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                true,
                (5, 0),
                click(overlay.x + 3, row_y)
            ),
            Some(AppAction::ActivatePaletteRow(2))
        );
        // Outside clicks have no actionable region.
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, true, (5, 0), click(2, 30)),
            None
        );
    }

    #[test]
    fn palette_wheel_moves_selection() {
        use super::super::palette::{PaletteCommand, overlay_area};
        let overlay = overlay_area(FULL, PaletteCommand::ALL.len());
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Fans,
                true,
                (5, 0),
                wheel(true, overlay.x + 3, overlay.y + 3)
            ),
            Some(AppAction::MoveUp)
        );
    }

    #[test]
    fn non_left_buttons_and_motion_are_inert() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[0];
        for kind in [
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::Moved,
            MouseEventKind::Drag(MouseButton::Left),
        ] {
            let event = MouseEvent {
                kind,
                column: row.x + 1,
                row: row.y,
                modifiers: KeyModifiers::empty(),
            };
            assert_eq!(
                action_for_mouse(FULL, Screen::Dashboard, false, (5, 0), event),
                None,
                "{kind:?} must be inert"
            );
        }
    }

    #[test]
    fn clicks_outside_regions_are_inert() {
        // The top strip carries state but no actions in P1.
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, false, (5, 0), click(150, 0)),
            None,
            "top strip must stay inert in P1"
        );
    }

    #[test]
    fn narrow_full_tier_stacks_cards_vertically() {
        let area = Rect::new(0, 0, 100, 30);
        let regions = dashboard_regions(area).expect("100x30 is full tier");
        assert_eq!(regions.cards.len(), 6);
        for pair in regions.cards.windows(2) {
            assert!(pair[1].y > pair[0].y, "cards must stack");
            assert_eq!(pair[1].x, pair[0].x);
        }
        assert_eq!(regions.menu_rows.len(), 9);
    }

    #[test]
    fn compact_and_tiny_areas_map_to_none() {
        assert_eq!(dashboard_regions(Rect::new(0, 0, 50, 16)), None);
        assert_eq!(dashboard_regions(Rect::new(0, 0, 20, 8)), None);
        assert_eq!(dashboard_regions(Rect::new(0, 0, 0, 0)), None);
    }

    #[test]
    fn dashboard_area_is_the_full_frame() {
        // The legacy navigation row stays hidden on the dashboard, so the
        // approved shell (and hit-testing) starts at the first frame row.
        assert_eq!(dashboard_area(FULL), FULL);
    }

    #[test]
    fn mouse_outputs_stay_within_non_mutating_allowlist() {
        // Every action the mapper can emit must be navigation, help,
        // quit, focus, or row movement: nothing reaches the executor.
        let regions = dashboard_regions(FULL).expect("full area maps");
        let mut points = Vec::new();
        for row in &regions.menu_rows {
            points.push((row.x + 1, row.y));
        }
        for card in &regions.cards {
            points.push((card.x + 1, card.y + 1));
        }
        points.push((regions.foot_help.x + 1, regions.foot_help.y));
        points.push((regions.foot_quit.x + 1, regions.foot_quit.y));
        for (col, row) in points {
            if let Some(action) =
                action_for_mouse(FULL, Screen::Dashboard, false, (5, 0), click(col, row))
            {
                assert!(
                    matches!(
                        action,
                        AppAction::GoTo(_)
                            | AppAction::Quit
                            | AppAction::ToggleHelp
                            | AppAction::FocusCard(_)
                    ),
                    "{action:?} must stay non-mutating"
                );
            }
        }
        let row = &regions.menu_rows[0];
        assert!(matches!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
                false,
                (5, 0),
                wheel(true, row.x + 1, row.y)
            ),
            Some(AppAction::MoveUp)
        ));
    }
    #[test]
    fn scrolled_profile_click_selects_the_drawn_row() {
        let full = Rect::new(0, 0, 80, 24);
        let workspace = super::super::shell::shell_split(full).1;
        let regions = super::screen_regions(Screen::Profiles, workspace, 45).unwrap();
        let last = regions.rows.last().unwrap();
        assert_eq!(
            action_for_mouse(
                full,
                Screen::Profiles,
                false,
                (45, 44),
                click(last.x + 1, last.y)
            ),
            Some(AppAction::SelectProfileRow(44))
        );
    }
}
