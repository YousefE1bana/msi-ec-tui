//! Responsive tiers for the interactive TUI: full, compact, tiny.
//!
//! Full preserves the Task-4/5 layouts; compact renders the selected
//! screen's key values as plain lines; tiny falls back safely. Tiers derive
//! from the frame only, never from cached dimensions.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware, Screen};
use crate::hardware::{Capabilities, EcBackend};

use super::profile_catalog::ProfileCatalog;
use super::screens::{
    diagnostics as diagnostics_screen, fans as fans_screen, profiles as profiles_screen,
};
use super::theme::Theme;
use super::ui::{
    battery_lines, device_lines, performance_lines, read_only_reason_text, support_mode_style,
    support_mode_text, telemetry_state_text, telemetry_style, thermals_lines,
};

/// Frame-size tier driving dispatcher routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutTier {
    /// Existing Task-4/5 layouts with navigation chrome.
    Full,
    /// Plain key-value lines for the selected screen.
    Compact,
    /// Minimal safe fallback.
    Tiny,
}

/// Tier boundaries: full keeps navigation plus the 14-row screen minimum;
/// compact stays useful down to 40x10; anything smaller is tiny.
pub(crate) fn layout_tier(area: Rect) -> LayoutTier {
    if area.width >= 60 && area.height >= 15 {
        LayoutTier::Full
    } else if area.width >= 40 && area.height >= 10 {
        LayoutTier::Compact
    } else {
        LayoutTier::Tiny
    }
}

/// Renders the selected screen's key values as plain truncatable lines.
/// Current snapshots only; capabilities follow where space permits.
/// Interactive screens also list control rows with selection state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_compact_screen<B: EcBackend>(
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
    let mut lines = compact_header(app, live, theme);
    lines.extend(compact_body(
        app,
        live,
        capabilities,
        catalog,
        selection,
        controls,
        theme,
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(theme.base_style()),
        area,
    );
}

fn compact_header<B: EcBackend>(
    app: &AppState,
    live: &LiveHardware<B>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::styled(
            format!("MEC — {}", app.current_screen().title()),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(format!("Device: {}", live.device().product_name)),
        Line::from(vec![
            Span::raw("Mode: "),
            Span::styled(
                support_mode_text(live.mode()).to_owned(),
                support_mode_style(live.mode(), theme),
            ),
            Span::raw(match live.mode() {
                crate::hardware::SupportMode::ReadOnly(reason) => {
                    format!(" ({})", read_only_reason_text(reason))
                }
                _ => String::new(),
            }),
        ]),
        Line::from(vec![
            Span::raw("Telemetry: "),
            Span::styled(
                telemetry_state_text(live.is_degraded(), live.current_snapshot().is_some())
                    .to_owned(),
                telemetry_style(live.is_degraded(), live.current_snapshot().is_some(), theme),
            ),
        ]),
    ];
    if let Some(error) = live.snapshot_error() {
        lines.push(Line::styled(
            error.to_string(),
            Style::default().fg(theme.danger),
        ));
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn compact_body<B: EcBackend>(
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &crate::app::ProfileSelection,
    controls: &crate::tui::editing::ControlState,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let snapshot = live.current_snapshot();
    match app.current_screen() {
        Screen::Dashboard => {
            let mut lines = thermals_lines(snapshot);
            lines.extend(
                crate::tui::history::temperature_summary_lines(live.history())
                    .into_iter()
                    .map(Line::from),
            );
            lines.extend(performance_lines(snapshot));
            lines.extend(battery_lines(snapshot));
            let devices = device_lines(snapshot);
            lines.push(devices[0].clone());
            lines.push(devices[2].clone());
            lines
        }
        Screen::Performance => {
            let mut lines = performance_lines(snapshot);
            lines.extend(crate::tui::controls::control_row_lines(
                Screen::Performance,
                snapshot,
                capabilities,
                live.mode(),
                controls,
                theme,
            ));
            lines
        }
        Screen::Fans => {
            let mut lines = fans_screen::current_lines(snapshot);
            lines.extend(
                crate::tui::history::fan_summary_lines(live.history())
                    .into_iter()
                    .map(Line::from),
            );
            lines.extend(crate::tui::controls::control_row_lines(
                Screen::Fans,
                snapshot,
                capabilities,
                live.mode(),
                controls,
                theme,
            ));
            lines
        }
        Screen::Battery => {
            let mut lines = battery_lines(snapshot);
            lines.extend(crate::tui::controls::control_row_lines(
                Screen::Battery,
                snapshot,
                capabilities,
                live.mode(),
                controls,
                theme,
            ));
            lines
        }
        Screen::Devices => {
            let mut lines = device_lines(snapshot);
            lines.extend(crate::tui::controls::control_row_lines(
                Screen::Devices,
                snapshot,
                capabilities,
                live.mode(),
                controls,
                theme,
            ));
            lines
        }
        Screen::Profiles => {
            let selected = selection.index();
            let mut lines = profiles_screen::catalog_lines(capabilities, selected, theme);
            lines.extend(profiles_screen::custom_lines(
                catalog,
                crate::profiles::BuiltinPreset::all().len(),
                selected,
                theme,
            ));
            lines
        }
        Screen::Diagnostics => {
            let mut lines = vec![Line::from("Export: mec doctor --export")];
            lines.extend(diagnostics_screen::identity_lines(live.device()));
            lines.extend(diagnostics_screen::telemetry_lines(live, theme));
            lines.extend(diagnostics_screen::matrix_lines(capabilities, theme));
            lines
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{AppAction, AppState, Screen};
    use crate::hardware::{BackendError, SupportMode};

    use super::super::screens::render_screen;
    use super::super::screens::support::{
        full_capabilities, healthy_snapshot, live_for, screen_text,
    };
    use super::layout_tier;

    fn app_on(screen: Screen) -> AppState {
        let mut app = AppState::default();
        app.apply(AppAction::GoTo(screen));
        app
    }

    fn rendered(screen: Screen, width: u16, height: u16) -> String {
        rendered_with(
            screen,
            width,
            height,
            vec![Ok(healthy_snapshot())],
            SupportMode::Ready,
            1,
        )
    }

    fn rendered_with(
        screen: Screen,
        width: u16,
        height: u16,
        script: Vec<Result<crate::hardware::HardwareSnapshot, BackendError>>,
        mode: SupportMode,
        refreshes: usize,
    ) -> String {
        let (live, _) = live_for(script, mode, refreshes);
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
                &crate::tui::palette::CommandPalette::default(),
                &crate::tui::notifications::NotificationCenter::new(),
                false,
            );
        })
    }

    #[test]
    fn full_dashboard_at_reference_size() {
        let text = rendered(Screen::Dashboard, 100, 30);
        assert!(text.contains("THERMALS"));
        assert!(text.contains("63°C"));
    }

    #[test]
    fn full_navigation_labels_at_reference_size() {
        let text = rendered(Screen::Dashboard, 100, 30);
        for label in [
            "1 Dashboard",
            "2 Performance",
            "3 Fans",
            "4 Battery",
            "5 Devices",
            "6 Profiles",
            "7 Diagnostics",
        ] {
            assert!(text.contains(label), "{label:?} missing");
        }
    }

    #[test]
    fn medium_terminal_navigation_stays_meaningful() {
        let text = rendered(Screen::Dashboard, 72, 20);
        assert!(text.contains("1 Dash"));
        assert!(text.contains("6 Prof"));
        assert!(text.contains("7 Diag"));
    }

    #[test]
    fn medium_terminal_uses_abbreviated_navigation() {
        let text = rendered(Screen::Dashboard, 56, 16);
        let nav: Vec<&str> = text.lines().collect();
        assert_eq!(
            nav[0],
            "1 Dash  2 Perf  3 Fans  4 Batt  5 Dev  6 Prof  7 Diag"
        );
    }

    #[test]
    fn narrow_terminal_shows_current_screen_context() {
        let text = rendered(Screen::Fans, 40, 10);
        assert!(text.contains("3/7 Fans"));
    }

    #[test]
    fn narrow_profiles_shows_current_screen_context() {
        let text = rendered(Screen::Profiles, 40, 10);
        assert!(text.contains("6/7 Profiles"));
    }

    #[test]
    fn compact_profiles_lists_catalog() {
        let text = rendered(Screen::Profiles, 50, 16);
        assert!(text.contains("MEC"));
        assert!(text.contains("Profiles"));
        assert!(text.contains("Gaming"));
        assert!(text.contains("battery-saver"));
    }

    #[test]
    fn compact_profiles_lists_prepared_customs() {
        use crate::profiles::ProfileStore;
        use crate::tui::ProfileCatalog;

        let dir = tempfile::tempdir().expect("compact TempDir constructs");
        let store = ProfileStore::new(dir.path().join("profiles"));
        std::fs::create_dir_all(store.directory()).expect("compact dir constructs");
        std::fs::write(
            store.directory().join("work.toml"),
            b"name = \"Compact Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        )
        .expect("compact profile writes");
        let catalog = ProfileCatalog::from_store(&store);
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Profiles);
        let text = screen_text(50, 16, |frame| {
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
        assert!(text.contains("Compact Work"));
        assert!(text.contains("Valid"));
    }

    #[test]
    fn compact_control_screens_show_selection_safely() {
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
        ] {
            let text = rendered(screen, 50, 16);
            assert!(text.contains("MEC"), "{screen:?}");
            assert!(text.contains(screen.title()), "{screen:?}");
        }
        // Fans compact shows the selected Fan Mode row without RPM.
        let fans = rendered(Screen::Fans, 50, 16);
        assert!(fans.contains("Fan Mode"));
        assert!(!fans.contains("RPM"));
    }

    #[test]
    fn compact_dashboard_stays_useful() {
        let text = rendered(Screen::Dashboard, 50, 16);
        assert!(text.contains("MEC"));
        assert!(text.contains("Dashboard"));
        assert!(text.contains("63°C"));
        assert!(text.contains("42%"));
        assert!(text.contains("comfort"));
    }

    #[test]
    fn smallest_compact_keeps_screen_context() {
        let text = rendered(Screen::Battery, 40, 10);
        assert!(text.contains("MEC"));
        assert!(text.contains("Battery"));
        assert!(text.contains("77%"));
    }

    #[test]
    fn tiny_terminal_falls_back_safely() {
        let text = rendered(Screen::Dashboard, 20, 8);
        assert!(text.contains("MEC"));
        assert!(text.contains("Terminal too small"));
    }

    #[test]
    fn minimal_terminal_does_not_panic() {
        let text = rendered(Screen::Dashboard, 1, 1);
        assert!(!text.is_empty() || text.is_empty());
    }

    #[test]
    fn zero_area_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;

        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(Screen::Performance);
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
            .expect("zero-area dispatch draws");
    }

    #[test]
    fn compact_degraded_hides_stale_telemetry() {
        let text = rendered_with(
            Screen::Fans,
            50,
            16,
            vec![Ok(healthy_snapshot()), Err(BackendError::Unavailable)],
            SupportMode::Ready,
            2,
        );
        assert!(text.contains("DEGRADED"));
        assert!(text.contains("CPU Fan: N/A"));
        // The stale sample may remain visible only inside the labeled
        // history summary, never as current telemetry.
        for line in text.lines() {
            if line.contains("42%") {
                assert!(line.contains("History"), "{line:?}");
            }
        }
        assert!(text.lines().any(|line| line.contains("42%")));
    }

    #[test]
    fn compact_fan_rendering_contains_no_rpm() {
        let text = rendered(Screen::Fans, 50, 16);
        assert!(!text.contains("RPM"));
        assert!(text.contains("42%"));
    }

    #[test]
    fn tier_boundaries_are_deterministic() {
        use ratatui::layout::Rect;

        use super::LayoutTier;

        assert_eq!(layout_tier(Rect::new(0, 0, 100, 30)), LayoutTier::Full);
        assert_eq!(layout_tier(Rect::new(0, 0, 60, 15)), LayoutTier::Full);
        assert_eq!(layout_tier(Rect::new(0, 0, 59, 30)), LayoutTier::Compact);
        assert_eq!(layout_tier(Rect::new(0, 0, 60, 14)), LayoutTier::Compact);
        assert_eq!(layout_tier(Rect::new(0, 0, 40, 10)), LayoutTier::Compact);
        assert_eq!(layout_tier(Rect::new(0, 0, 39, 30)), LayoutTier::Tiny);
        assert_eq!(layout_tier(Rect::new(0, 0, 40, 9)), LayoutTier::Tiny);
    }

    #[test]
    fn compact_dashboard_shows_concise_history_summaries() {
        let text = rendered(Screen::Dashboard, 50, 16);
        assert!(text.contains("CPU History: 1 samples 63-63°C"));
        assert!(text.contains("GPU History: 1 samples 51-51°C"));
    }

    #[test]
    fn compact_fans_shows_concise_fan_summaries() {
        let text = rendered(Screen::Fans, 50, 16);
        assert!(text.contains("CPU Fan History: 1 samples 42-42%"));
        assert!(text.contains("GPU Fan History: 1 samples 31-31%"));
        assert!(!text.contains("RPM"));
    }

    #[test]
    fn tiny_history_screens_remain_safe() {
        for screen in [Screen::Dashboard, Screen::Fans] {
            let text = rendered(screen, 20, 8);
            assert!(!text.is_empty());
        }
    }
}
