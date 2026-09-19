//! Read-only screen collection with AppState dispatcher.
//!
//! Each screen renders already-sampled [`LiveHardware`] state plus injected
//! startup [`Capabilities`]. Renderers never sample, discover, or probe.
//! The dispatcher owns the shared navigation chrome and renders the help
//! overlay last so it sits above the active screen.

pub(crate) mod battery;
pub(crate) mod dashboard;
pub(crate) mod devices;
pub(crate) mod diagnostics;
pub(crate) mod fans;
pub(crate) mod performance;
pub(crate) mod profiles;
#[cfg(test)]
pub(crate) mod support;

pub use battery::render_battery;
pub use dashboard::render_dashboard;
pub use devices::render_devices;
pub use diagnostics::render_diagnostics;
pub use fans::render_fans;
pub use performance::render_performance;
pub use profiles::render_profiles;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware, Screen};
use crate::hardware::{Capabilities, EcBackend};

use crate::tui::profile_catalog::ProfileCatalog;
use crate::tui::responsive::{LayoutTier, layout_tier, render_compact_screen};
use crate::tui::theme::Theme;
use crate::tui::ui::render_compact;

/// Renders the screen selected by `app` with the default theme.
///
/// Help visibility is honored: a visible overlay renders above the screen.
#[allow(clippy::too_many_arguments)]
pub fn render_screen<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &crate::app::ProfileSelection,
    controls: &crate::tui::editing::ControlState,
    palette: &crate::tui::palette::CommandPalette,
    notifications: &crate::tui::notifications::NotificationCenter,
    notifications_open: bool,
) {
    render_screen_with_theme(
        frame,
        area,
        app,
        live,
        capabilities,
        catalog,
        selection,
        controls,
        palette,
        notifications,
        notifications_open,
        &Theme::default(),
    );
}

/// Theme-aware dispatcher.
///
/// Layering is deterministic: active/compact screen at the bottom, then the
/// result notice, then the modal confirmation, then the command palette,
/// then the notification history, then help on top. Tiny skips overlays
/// and stays a safe fallback.
#[allow(clippy::too_many_arguments)]
pub fn render_screen_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &crate::app::ProfileSelection,
    controls: &crate::tui::editing::ControlState,
    palette: &crate::tui::palette::CommandPalette,
    notifications: &crate::tui::notifications::NotificationCenter,
    notifications_open: bool,
    theme: &Theme,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    render_navigation(frame, rows[0], app, theme);
    let tier = layout_tier(area);
    match tier {
        LayoutTier::Full => {
            render_active_screen(
                frame,
                rows[1],
                app,
                live,
                capabilities,
                catalog,
                selection,
                controls,
                theme,
            );
        }
        LayoutTier::Compact => {
            render_compact_screen(
                frame,
                rows[1],
                app,
                live,
                capabilities,
                catalog,
                selection,
                controls,
                theme,
            );
        }
        LayoutTier::Tiny => render_compact(frame, rows[1]),
    }
    if !matches!(tier, LayoutTier::Tiny) {
        render_overlays(
            frame,
            area,
            live,
            capabilities,
            controls,
            palette,
            notifications,
            notifications_open,
            theme,
        );
    }
    if app.help_visible() {
        super::help::render_help(frame, area, theme);
    }
}

/// Dispatches the active screen. Split from [`render_screen_with_theme`]
/// so the chrome/overlay orchestration stays readable.
#[allow(clippy::too_many_arguments)]
fn render_active_screen<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &crate::app::ProfileSelection,
    controls: &crate::tui::editing::ControlState,
    theme: &Theme,
) {
    match app.current_screen() {
        Screen::Dashboard => {
            dashboard::render_dashboard_with_theme(frame, area, live, theme);
        }
        Screen::Performance => {
            performance::render_performance_with_theme(
                frame,
                area,
                live,
                capabilities,
                controls,
                theme,
            );
        }
        Screen::Fans => {
            fans::render_fans_with_theme(frame, area, live, capabilities, controls, theme);
        }
        Screen::Battery => {
            battery::render_battery_with_theme(frame, area, live, capabilities, controls, theme);
        }
        Screen::Devices => {
            devices::render_devices_with_theme(frame, area, live, capabilities, controls, theme);
        }
        Screen::Profiles => {
            profiles::render_profiles_with_theme(
                frame,
                area,
                live,
                capabilities,
                catalog,
                selection,
                theme,
            );
        }
        Screen::Diagnostics => {
            diagnostics::render_diagnostics_with_theme(frame, area, live, capabilities, theme);
        }
    }
}

/// Renders notice, confirmation, palette, then notification history
/// above the screen, matching input precedence. Help renders last and
/// stays on top. Zero-area safe via saturating overlay geometry.
#[allow(clippy::too_many_arguments)]
fn render_overlays<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    controls: &crate::tui::editing::ControlState,
    palette: &crate::tui::palette::CommandPalette,
    notifications: &crate::tui::notifications::NotificationCenter,
    notifications_open: bool,
    theme: &Theme,
) {
    if let Some(notice) = controls.notice() {
        crate::tui::confirmation::render_notice(frame, area, notice, theme);
    }
    if let Some(pending) = controls.pending() {
        crate::tui::confirmation::render_confirmation(
            frame,
            area,
            pending,
            live.current_snapshot(),
            live.mode(),
            capabilities,
            theme,
        );
    }
    if palette.is_open() {
        crate::tui::palette::render_palette(frame, area, palette, theme);
    }
    if notifications_open {
        crate::tui::notifications::render_notifications(frame, area, notifications, theme);
    }
}

/// Shared navigation row in canonical [`Screen::ALL`] order. Labels shrink
/// with the terminal: full names when they fit, short names below that,
/// and the current screen alone on narrow displays. The active entry uses
/// the primary role plus bold; the rest stay muted.
fn render_navigation(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    let full: Vec<String> = Screen::ALL
        .iter()
        .enumerate()
        .map(|(index, screen)| format!("{} {}", index + 1, screen.title()))
        .collect();
    let short: Vec<String> = Screen::ALL
        .iter()
        .enumerate()
        .map(|(index, screen)| format!("{} {}", index + 1, short_title(*screen)))
        .collect();
    let line = if area.width as usize >= full.join("  ").len() {
        navigation_line(&full, app, theme)
    } else if area.width as usize >= short.join("  ").len() {
        navigation_line(&short, app, theme)
    } else {
        narrow_navigation_line(app, theme)
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Abbreviated navigation labels for medium terminals.
fn short_title(screen: Screen) -> &'static str {
    match screen {
        Screen::Dashboard => "Dash",
        Screen::Performance => "Perf",
        Screen::Fans => "Fans",
        Screen::Battery => "Batt",
        Screen::Devices => "Dev",
        Screen::Profiles => "Prof",
        Screen::Diagnostics => "Diag",
    }
}

fn navigation_entry_style(active: bool, theme: &Theme) -> Style {
    if active {
        Style::default()
            .fg(theme.primary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.muted)
    }
}

fn navigation_line(entries: &[String], app: &AppState, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    for (index, screen) in Screen::ALL.iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            entries[index].clone(),
            navigation_entry_style(*screen == app.current_screen(), theme),
        ));
    }
    Line::from(spans)
}

fn narrow_navigation_line(app: &AppState, theme: &Theme) -> Line<'static> {
    let position = Screen::ALL
        .iter()
        .position(|screen| *screen == app.current_screen())
        .expect("current screen is a member of ALL");
    Line::from(vec![
        Span::styled(
            format!("{}/7 {}", position + 1, app.current_screen().title()),
            navigation_entry_style(true, theme),
        ),
        Span::styled(" • ? Help • Q Quit", navigation_entry_style(false, theme)),
    ])
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;
    use ratatui::style::Modifier;

    use crate::app::{AppAction, AppState, Screen};
    use crate::hardware::{BackendError, SupportMode};

    use super::support::{
        first_cell_style, full_capabilities, healthy_snapshot, live_for, screen_text,
    };
    use super::{
        render_battery, render_devices, render_diagnostics, render_fans, render_performance,
        render_profiles, render_screen, render_screen_with_theme,
    };
    use crate::tui::theme::Theme;

    fn app_on(screen: Screen) -> AppState {
        let mut app = AppState::default();
        app.apply(AppAction::GoTo(screen));
        app
    }

    fn dispatched(screen: Screen) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(screen);
        screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        })
    }

    #[test]
    fn dashboard_dispatch_renders_dashboard() {
        assert!(dispatched(Screen::Dashboard).contains("THERMALS"));
    }

    #[test]
    fn performance_dispatch_renders_performance() {
        assert!(dispatched(Screen::Performance).contains("Available Shift Modes"));
    }

    #[test]
    fn fans_dispatch_renders_fans() {
        assert!(dispatched(Screen::Fans).contains("CPU Fan Telemetry"));
    }

    #[test]
    fn battery_dispatch_renders_battery() {
        assert!(dispatched(Screen::Battery).contains("Threshold Control"));
    }

    #[test]
    fn devices_dispatch_renders_devices() {
        assert!(dispatched(Screen::Devices).contains("Fn Key"));
    }

    #[test]
    fn diagnostics_dispatch_renders_diagnostics() {
        assert!(dispatched(Screen::Diagnostics).contains("Manufacturer"));
    }

    #[test]
    fn profiles_dispatch_renders_profiles() {
        let text = dispatched(Screen::Profiles);
        assert!(text.contains("PROFILE LIST"));
        assert!(text.contains("DETAILS / PREVIEW"));
        assert!(text.contains("(none)"));
        assert!(text.contains("6 Profiles"));
    }

    #[test]
    fn profiles_dispatch_renders_prepared_customs() {
        use crate::profiles::ProfileStore;

        let dir = tempfile::tempdir().expect("dispatch TempDir constructs");
        let store = ProfileStore::new(dir.path().join("profiles"));
        std::fs::create_dir_all(store.directory()).expect("dispatch dir constructs");
        std::fs::write(
            store.directory().join("work.toml"),
            b"name = \"Dispatch Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        )
        .expect("dispatch profile writes");
        let catalog = crate::tui::ProfileCatalog::from_store(&store);
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Profiles);
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &catalog,
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(text.contains("Dispatch Work"));
        assert!(text.contains("Valid"));
    }

    #[test]
    fn navigation_renders_all_seven_screen_names() {
        let text = dispatched(Screen::Dashboard);
        for name in [
            "Dashboard",
            "Performance",
            "Fans",
            "Battery",
            "Devices",
            "Profiles",
            "Diagnostics",
        ] {
            assert!(text.contains(name), "{name:?} missing from navigation");
        }
    }

    #[test]
    fn navigation_renders_digits() {
        let text = dispatched(Screen::Dashboard);
        for digit in ["1", "2", "3", "4", "5", "6", "7"] {
            assert!(text.contains(digit), "{digit:?} missing from navigation");
        }
    }

    #[test]
    fn navigation_follows_canonical_order() {
        let text = dispatched(Screen::Dashboard);
        let mut positions = Vec::new();
        for name in [
            "1 Dashboard",
            "2 Performance",
            "3 Fans",
            "4 Battery",
            "5 Devices",
            "6 Profiles",
            "7 Diagnostics",
        ] {
            positions.push(text.find(name).expect("{name:?} missing"));
        }
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn active_dashboard_entry_uses_primary_style() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let theme = Theme::default();
        let (foreground, modifier) = first_cell_style(100, 30, "1 Dashboard", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("navigation entry present");
        assert_eq!(foreground, theme.primary);
        assert!(modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn goto_fans_marks_fans_entry_active() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(100, 30, "3 Fans", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("fans entry present");
        assert_eq!(foreground, theme.primary);
        let (dashboard_foreground, _) = first_cell_style(100, 30, "1 Dashboard", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("dashboard entry present");
        assert_eq!(dashboard_foreground, theme.muted);
    }

    #[test]
    fn goto_profiles_marks_profiles_entry_active() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Profiles);
        let theme = Theme::default();
        let (foreground, modifier) = first_cell_style(100, 30, "6 Profiles", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("profiles entry present");
        assert_eq!(foreground, theme.primary);
        assert!(modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn only_one_entry_is_styled_active() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Battery);
        let theme = Theme::default();
        let mut active = 0;
        for label in [
            "1 Dashboard",
            "2 Performance",
            "3 Fans",
            "4 Battery",
            "5 Devices",
            "6 Profiles",
            "7 Diagnostics",
        ] {
            let (foreground, _) = first_cell_style(100, 30, label, |frame| {
                render_screen_with_theme(
                    frame,
                    frame.area(),
                    &app,
                    &live,
                    &capabilities,
                    &crate::tui::ProfileCatalog::empty(),
                    &crate::app::ProfileSelection::default(),
                    &crate::tui::editing::ControlState::default(),
                    &crate::tui::palette::CommandPalette::default(),
                    &crate::tui::notifications::NotificationCenter::new(),
                    false,
                    &theme,
                );
            })
            .expect("entry present");
            if foreground == theme.primary {
                active += 1;
            }
        }
        assert_eq!(active, 1);
    }

    #[test]
    fn ready_uses_success_role() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(100, 30, "READY", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("READY present");
        assert_eq!(foreground, theme.success);
    }

    #[test]
    fn read_only_uses_warning_role() {
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot())],
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
            1,
        );
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(100, 30, "READ-ONLY", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("READ-ONLY present");
        assert_eq!(foreground, theme.warning);
    }

    #[test]
    fn live_uses_success_role() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(100, 30, "LIVE", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("LIVE present");
        assert_eq!(foreground, theme.success);
    }

    #[test]
    fn waiting_uses_muted_role() {
        let (live, _) = live_for(Vec::new(), SupportMode::Ready, 0);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(100, 30, "WAITING", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("WAITING present");
        assert_eq!(foreground, theme.muted);
    }

    #[test]
    fn degraded_uses_danger_role() {
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot()), Err(BackendError::Unavailable)],
            SupportMode::Ready,
            2,
        );
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(100, 30, "DEGRADED", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("DEGRADED present");
        assert_eq!(foreground, theme.danger);
    }

    #[test]
    fn render_screen_still_works_without_theme() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(text.contains("CPU Fan Telemetry"));
    }

    #[test]
    fn custom_theme_drives_active_styling() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Devices);
        let theme = Theme {
            primary: ratatui::style::Color::Red,
            ..Theme::default()
        };
        let (foreground, _) = first_cell_style(100, 30, "5 Devices", |frame| {
            render_screen_with_theme(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
                &theme,
            );
        })
        .expect("devices entry present");
        assert_eq!(foreground, ratatui::style::Color::Red);
    }

    #[test]
    fn rendering_all_screens_performs_zero_backend_calls() {
        let (live, calls) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        assert_eq!(calls.get(), 1);
        for screen in Screen::ALL {
            let app = app_on(screen);
            let _ = screen_text(100, 30, |frame| {
                render_screen(
                    frame,
                    frame.area(),
                    &app,
                    &live,
                    &capabilities,
                    &crate::tui::ProfileCatalog::empty(),
                    &crate::app::ProfileSelection::default(),
                    &crate::tui::editing::ControlState::default(),
                    &crate::tui::palette::CommandPalette::default(),
                    &crate::tui::notifications::NotificationCenter::new(),
                    false,
                );
            });
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn every_screen_survives_tiny_area() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        for screen in Screen::ALL {
            let app = app_on(screen);
            let text = screen_text(20, 8, |frame| {
                render_screen(
                    frame,
                    frame.area(),
                    &app,
                    &live,
                    &capabilities,
                    &crate::tui::ProfileCatalog::empty(),
                    &crate::app::ProfileSelection::default(),
                    &crate::tui::editing::ControlState::default(),
                    &crate::tui::palette::CommandPalette::default(),
                    &crate::tui::notifications::NotificationCenter::new(),
                    false,
                );
            });
            assert!(text.contains("Terminal too small"), "{screen:?}");
        }
    }

    #[test]
    fn every_screen_survives_zero_area() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        for screen in Screen::ALL {
            let app = app_on(screen);
            let backend = TestBackend::new(10, 5);
            let mut terminal = Terminal::new(backend).expect("test terminal constructs");
            terminal
                .draw(|frame| {
                    render_screen(
                        frame,
                        Rect::new(0, 0, 0, 0),
                        &app,
                        &live,
                        &capabilities,
                        &crate::tui::ProfileCatalog::empty(),
                        &crate::app::ProfileSelection::default(),
                        &crate::tui::editing::ControlState::default(),
                        &crate::tui::palette::CommandPalette::default(),
                        &crate::tui::notifications::NotificationCenter::new(),
                        false,
                    );
                })
                .expect("zero-area screen draws");
        }
    }

    #[test]
    fn confirmation_overlay_sits_above_command_screen() {
        use crate::tui::confirmation::PendingMutation;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let mut controls = crate::tui::editing::ControlState::default();
        assert!(controls.begin_edit(
            Screen::Fans,
            live.current_snapshot(),
            &capabilities,
            live.mode(),
        ));
        assert!(controls.confirm(live.mode(), &capabilities));
        assert!(matches!(
            controls.pending(),
            Some(PendingMutation::Command(_))
        ));
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &controls,
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(text.contains("Fans"));
        assert!(text.contains("Confirm Hardware Change"));
        assert!(text.contains("Requested: Fan Mode:"));
        assert!(!text.contains("RPM"));
    }

    #[test]
    fn confirmation_overlay_sits_above_profile_screen() {
        use crate::profiles::BuiltinPreset;
        use crate::profiles::ProfilePlanner;
        use crate::tui::confirmation::{PendingMutation, ProfilePending, ProfileSource};
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Profiles);
        let profile = BuiltinPreset::Balanced
            .resolve(&capabilities)
            .expect("balanced resolves");
        assert!(ProfilePlanner::preview(&profile, live.mode(), &capabilities).is_applicable());
        let mut controls = crate::tui::editing::ControlState::default();
        controls.set_profile_pending(ProfilePending::new(
            profile,
            "balanced".to_owned(),
            ProfileSource::Builtin,
        ));
        assert!(matches!(
            controls.pending(),
            Some(PendingMutation::Profile(_))
        ));
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &controls,
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(text.contains("Profiles"));
        assert!(text.contains("Confirm Profile Apply"));
        assert!(text.contains("Balanced"));
    }

    #[test]
    fn zero_area_confirmation_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let mut controls = crate::tui::editing::ControlState::default();
        assert!(controls.begin_edit(
            Screen::Fans,
            live.current_snapshot(),
            &capabilities,
            live.mode(),
        ));
        assert!(controls.confirm(live.mode(), &capabilities));
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_screen(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &app,
                    &live,
                    &capabilities,
                    &crate::tui::ProfileCatalog::empty(),
                    &crate::app::ProfileSelection::default(),
                    &controls,
                    &crate::tui::palette::CommandPalette::default(),
                    &crate::tui::notifications::NotificationCenter::new(),
                    false,
                );
            })
            .expect("zero-area confirmation draws");
    }

    #[test]
    fn tiny_mode_with_pending_remains_safe() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let mut controls = crate::tui::editing::ControlState::default();
        assert!(controls.begin_edit(
            Screen::Fans,
            live.current_snapshot(),
            &capabilities,
            live.mode(),
        ));
        assert!(controls.confirm(live.mode(), &capabilities));
        let text = screen_text(20, 8, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &controls,
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(!text.is_empty());
    }

    #[test]
    fn help_overlay_takes_precedence_over_confirmation() {
        use crate::app::AppAction;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = app_on(Screen::Fans);
        app.apply(AppAction::ShowHelp);
        let mut controls = crate::tui::editing::ControlState::default();
        assert!(controls.begin_edit(
            Screen::Fans,
            live.current_snapshot(),
            &capabilities,
            live.mode(),
        ));
        assert!(controls.confirm(live.mode(), &capabilities));
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &controls,
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        // Help renders last and stays on top; confirmation still exists below.
        assert!(text.contains("MEC Help"));
    }

    #[test]
    fn success_notice_renders_with_semantic_text() {
        use crate::tui::confirmation::Notice;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let mut controls = crate::tui::editing::ControlState::default();
        controls.set_notice(Notice::success("Applied Fan Mode: silent".to_owned()));
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &controls,
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(text.contains("Applied Fan Mode: silent"));
        assert!(text.contains("Result"));
    }

    #[test]
    fn failure_notice_renders_display_error() {
        use crate::tui::confirmation::Notice;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Fans);
        let mut controls = crate::tui::editing::ControlState::default();
        controls.set_notice(Notice::failure("Action failed: gone".to_owned()));
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &controls,
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        });
        assert!(text.contains("Action failed: gone"));
    }

    #[test]
    fn individual_renderers_stay_reachable() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let _ = screen_text(100, 30, |frame| {
            render_performance(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        let _ = screen_text(100, 30, |frame| {
            render_fans(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        let _ = screen_text(100, 30, |frame| {
            render_battery(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        let _ = screen_text(100, 30, |frame| {
            render_devices(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::editing::ControlState::default(),
            );
        });
        let _ = screen_text(100, 30, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
            );
        });
        let _ = screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
    }

    fn overlay_text(
        screen: Screen,
        width: u16,
        height: u16,
        palette: &crate::tui::palette::CommandPalette,
        notifications: &crate::tui::notifications::NotificationCenter,
        notifications_open: bool,
    ) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(screen);
        screen_text(width, height, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                palette,
                notifications,
                notifications_open,
            );
        })
    }

    fn open_palette() -> crate::tui::palette::CommandPalette {
        let mut palette = crate::tui::palette::CommandPalette::default();
        palette.open();
        palette
    }

    fn center_with_entries() -> crate::tui::notifications::NotificationCenter {
        use crate::tui::confirmation::Notice;
        let mut center = crate::tui::notifications::NotificationCenter::new();
        center.push(Notice::success("Applied Fan Mode: silent".to_owned()));
        center.push(Notice::failure("Action failed: gone".to_owned()));
        center
    }

    #[test]
    fn palette_overlay_sits_above_screen() {
        let palette = open_palette();
        let center = crate::tui::notifications::NotificationCenter::new();
        let text = overlay_text(Screen::Dashboard, 100, 30, &palette, &center, false);
        assert!(text.contains("Command Palette"));
        assert!(text.contains("THERMALS"));
        assert!(text.contains("> Dashboard"));
        assert!(text.contains("Clear Notifications"));
    }

    #[test]
    fn notifications_overlay_lists_entries_above_screen() {
        let palette = crate::tui::palette::CommandPalette::default();
        let center = center_with_entries();
        let text = overlay_text(Screen::Dashboard, 100, 30, &palette, &center, true);
        assert!(text.contains("Notifications"));
        assert!(text.contains("Applied Fan Mode: silent"));
        assert!(text.contains("Action failed: gone"));
        assert!(text.contains("THERMALS"));
    }

    #[test]
    fn notifications_overlay_empty_state_is_honest() {
        let palette = crate::tui::palette::CommandPalette::default();
        let center = crate::tui::notifications::NotificationCenter::new();
        let text = overlay_text(Screen::Dashboard, 100, 30, &palette, &center, true);
        assert!(text.contains("No notifications yet"));
    }

    #[test]
    fn compact_palette_overlay_remains_useful() {
        let palette = open_palette();
        let center = crate::tui::notifications::NotificationCenter::new();
        let text = overlay_text(Screen::Dashboard, 50, 16, &palette, &center, false);
        assert!(text.contains("Command Palette"));
    }

    #[test]
    fn zero_area_palette_and_notifications_do_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Dashboard);
        let palette = open_palette();
        let center = center_with_entries();
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_screen(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &app,
                    &live,
                    &capabilities,
                    &crate::tui::ProfileCatalog::empty(),
                    &crate::app::ProfileSelection::default(),
                    &crate::tui::editing::ControlState::default(),
                    &palette,
                    &center,
                    true,
                );
            })
            .expect("zero-area overlays draw");
    }

    #[test]
    fn tiny_mode_with_overlays_remains_safe() {
        let palette = open_palette();
        let center = center_with_entries();
        let text = overlay_text(Screen::Dashboard, 20, 8, &palette, &center, true);
        assert!(!text.is_empty());
    }

    #[test]
    fn help_overlay_takes_precedence_over_palette_and_notifications() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = app_on(Screen::Dashboard);
        app.apply(AppAction::ShowHelp);
        let palette = open_palette();
        let center = center_with_entries();
        let text = screen_text(100, 30, |frame| {
            render_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &crate::tui::ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
                &crate::tui::editing::ControlState::default(),
                &palette,
                &center,
                true,
            );
        });
        assert!(text.contains("MEC Help"));
    }
}
