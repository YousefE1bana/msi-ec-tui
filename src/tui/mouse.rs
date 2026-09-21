//! Dashboard mouse geometry and non-mutating action mapping.
//!
//! Pure layout math shared by the dashboard renderer and the mouse event
//! path so hit regions always match what is drawn. Mouse may only produce
//! navigation, help, quit, card-focus, and row-move intents: every output
//! is an existing [`AppAction`] with zero hardware reach.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::{AppAction, Screen};

use super::responsive::{LayoutTier, layout_tier};

/// Approved menu rows in order: the seven production screens, a Settings
/// entry, and Exit. Production owns seven screens; Settings has no screen
/// and routes to the closest existing read-only surface (Diagnostics).
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
    /// Help label rect.
    pub help: Rect,
    /// Quit label rect.
    pub quit: Rect,
}

fn contains(area: Rect, col: u16, row: u16) -> bool {
    col >= area.x && col < area.x + area.width && row >= area.y && row < area.y + area.height
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
/// drawn spans exactly.
pub fn footer_regions(footer: Rect, read_only: bool) -> FooterRegions {
    // Segments in draw order with exact ASCII widths.
    let status = if read_only {
        " ✓ READ-ONLY  "
    } else {
        " ✓ READY  "
    };
    let fixed: &[&str] = &[
        status,
        "│ ",
        " [1-9] ",
        "Select ",
        " [↑↓] ",
        "Navigate ",
        " [Enter] ",
        "Open ",
        " [?] ",
    ];
    let mut x = footer.x;
    for part in fixed {
        x += part.len() as u16;
    }
    let help = Rect {
        x,
        y: footer.y,
        width: "Help ".len() as u16,
        height: 1,
    };
    x += help.width;
    x += " [Q] ".len() as u16;
    let quit = Rect {
        x,
        y: footer.y,
        width: "Quit ".len() as u16,
        height: 1,
    };
    FooterRegions { help, quit }
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
    let inner_x = cards[0].x + 1;
    let inner_y = cards[0].y + 1;
    let inner_w = cards[0].width.saturating_sub(2);
    let menu_rows = (0..MENU_LABELS.len())
        .map(|i| Rect {
            x: inner_x,
            y: inner_y + MENU_FIRST_ROW_OFFSET + i as u16,
            width: inner_w,
            height: 1,
        })
        .collect();
    let foot = footer_regions(footer, false);
    // READ-ONLY widens the status segment; recompute against the wider
    // layout and keep the widest rects so clicks land in either mode.
    let foot_ro = footer_regions(footer, true);
    let foot_help = union_row(foot.help, foot_ro.help);
    let foot_quit = union_row(foot.quit, foot_ro.quit);
    Some(DashboardRegions {
        menu_rows,
        cards,
        foot_help,
        foot_quit,
    })
}

/// Widest single-row span covering both rects (same row by construction).
fn union_row(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let end = (a.x + a.width).max(b.x + b.width);
    Rect {
        x,
        y: a.y,
        width: end.saturating_sub(x),
        height: 1,
    }
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

/// Maps one mouse event to a non-mutating [`AppAction`]. Returns `None`
/// for anything inert: non-dashboard screens, non-full tiers, unmapped
/// areas, non-left buttons, and hover/drag motion.
///
/// Menu rows win over card bodies; the footer wins over nothing (it never
/// overlaps cards). Wheel over the menu or control card moves like the
/// keyboard arrows; wheel elsewhere is inert.
pub fn action_for_mouse(full: Rect, current: Screen, event: MouseEvent) -> Option<AppAction> {
    if current != Screen::Dashboard {
        return None;
    }
    let regions = dashboard_regions(full)?;
    let col = event.column;
    let row = event.row;
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(index) = regions
                .menu_rows
                .iter()
                .position(|area| contains(*area, col, row))
            {
                return Some(menu_action(index));
            }
            if contains(regions.foot_help, col, row) {
                return Some(AppAction::ToggleHelp);
            }
            if contains(regions.foot_quit, col, row) {
                return Some(AppAction::Quit);
            }
            if let Some(index) = regions
                .cards
                .iter()
                .position(|area| contains(*area, col, row))
            {
                return Some(AppAction::FocusDashboardCard(index));
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

/// Menu row for a menu index: production screens jump directly, Settings
/// lands on Diagnostics (closest existing read-only surface), Exit quits.
fn menu_action(index: usize) -> AppAction {
    match index {
        SETTINGS_ROW => AppAction::GoTo(Screen::Diagnostics),
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
                action_for_mouse(FULL, Screen::Dashboard, click(row.x + 1, row.y)),
                Some(AppAction::GoTo(*screen)),
                "row {index} must navigate"
            );
        }
    }

    #[test]
    fn settings_row_lands_on_diagnostics() {
        assert_eq!(
            menu_action(SETTINGS_ROW),
            AppAction::GoTo(Screen::Diagnostics)
        );
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[SETTINGS_ROW];
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, click(row.x + 1, row.y)),
            Some(AppAction::GoTo(Screen::Diagnostics))
        );
    }

    #[test]
    fn exit_row_quits() {
        assert_eq!(menu_action(EXIT_ROW), AppAction::Quit);
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[EXIT_ROW];
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, click(row.x + 1, row.y)),
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
                action_for_mouse(FULL, Screen::Dashboard, click(col, row)),
                Some(AppAction::FocusDashboardCard(index)),
                "card {index} must focus"
            );
        }
    }

    #[test]
    fn menu_row_wins_over_card_body() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[2];
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, click(row.x + 1, row.y)),
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
                click(regions.foot_help.x + 1, regions.foot_help.y)
            ),
            Some(AppAction::ToggleHelp)
        );
        assert_eq!(
            action_for_mouse(
                FULL,
                Screen::Dashboard,
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
            action_for_mouse(FULL, Screen::Dashboard, wheel(true, row.x + 1, row.y)),
            Some(AppAction::MoveUp)
        );
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, wheel(false, row.x + 1, row.y)),
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
            action_for_mouse(FULL, Screen::Dashboard, wheel(true, col, row)),
            None
        );
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, wheel(false, col, row)),
            None
        );
    }

    #[test]
    fn non_dashboard_screens_ignore_mouse() {
        let regions = dashboard_regions(FULL).expect("full area maps");
        let row = &regions.menu_rows[0];
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
            Screen::Profiles,
            Screen::Diagnostics,
        ] {
            assert_eq!(
                action_for_mouse(FULL, screen, click(row.x + 1, row.y)),
                None,
                "{screen:?} must ignore mouse in P1"
            );
        }
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
                action_for_mouse(FULL, Screen::Dashboard, event),
                None,
                "{kind:?} must be inert"
            );
        }
    }

    #[test]
    fn clicks_outside_regions_are_inert() {
        // The top strip carries state but no actions in P1.
        assert_eq!(
            action_for_mouse(FULL, Screen::Dashboard, click(150, 0)),
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
            if let Some(action) = action_for_mouse(FULL, Screen::Dashboard, click(col, row)) {
                assert!(
                    matches!(
                        action,
                        AppAction::GoTo(_)
                            | AppAction::Quit
                            | AppAction::ToggleHelp
                            | AppAction::FocusDashboardCard(_)
                    ),
                    "{action:?} must stay non-mutating"
                );
            }
        }
        let row = &regions.menu_rows[0];
        assert!(matches!(
            action_for_mouse(FULL, Screen::Dashboard, wheel(true, row.x + 1, row.y)),
            Some(AppAction::MoveUp)
        ));
    }
}
