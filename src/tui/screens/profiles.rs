//! Read-only built-in profile catalog: capability-aware availability.
//!
//! Renders the five built-in presets in stable catalog order with their
//! display names, stable slugs, and whether each resolves against the
//! already-supplied startup capabilities. Viewing applies nothing: the
//! renderer only calls [`BuiltinPreset::resolve`] on injected data, never
//! samples hardware, discovers capabilities, loads profile files, or
//! writes. The profiles module owns every recipe; no mode policy lives
//! here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, EcBackend};
use crate::profiles::BuiltinPreset;

use crate::tui::theme::Theme;
use crate::tui::ui::{capability_style, render_panel, render_screen_shell};

/// Renders the capability-aware built-in catalog with the default theme.
pub fn render_profiles<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    render_profiles_with_theme(frame, area, live, capabilities, &Theme::default());
}

/// Theme-aware profiles renderer behind the Task-5 API.
pub(crate) fn render_profiles_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Profiles", live, theme);
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " BUILT-IN PROFILES ",
        catalog_lines(capabilities, theme),
        theme,
    );
    render_panel(
        frame,
        panels[1],
        " NOTE ",
        vec![Line::from(
            "Viewing applies nothing. Custom profile browsing and interactive apply are added in later PLAN-006 tasks.",
        )],
        theme,
    );
}

/// One catalog row per built-in preset in stable order. Availability comes
/// solely from [`BuiltinPreset::resolve`]: an `Unavailable` result — or a
/// conservative error state for an unexpected internal definition — never
/// claims availability and never panics.
pub(crate) fn catalog_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
    BuiltinPreset::all()
        .iter()
        .map(|preset| {
            let available = preset.resolve(capabilities).is_ok();
            Line::styled(
                format!(
                    "{:<15} {:<15} {}",
                    preset.name(),
                    preset.slug(),
                    if available {
                        "Available"
                    } else {
                        "Unavailable"
                    },
                ),
                capability_style(available, theme),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::hardware::SupportMode;

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::render_profiles;
    use crate::profiles::BuiltinPreset;

    fn text_with(capabilities: &crate::hardware::Capabilities) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        screen_text(100, 30, |frame| {
            render_profiles(frame, frame.area(), &live, capabilities);
        })
    }

    fn text() -> String {
        text_with(&full_capabilities())
    }

    #[test]
    fn screen_identifies_profiles() {
        assert!(text().contains("Profiles"));
    }

    #[test]
    fn lists_all_five_display_names_in_stable_order() {
        let text = text();
        let mut positions = Vec::new();
        for name in [
            "Balanced",
            "Silent",
            "Gaming",
            "Battery Saver",
            "Maximum Cooling",
        ] {
            positions.push(text.find(name).expect("{name:?} missing"));
        }
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn lists_all_five_stable_slugs() {
        let text = text();
        for slug in [
            "balanced",
            "silent",
            "gaming",
            "battery-saver",
            "maximum-cooling",
        ] {
            assert!(text.contains(slug), "{slug:?} missing");
        }
    }

    #[test]
    fn full_capabilities_show_available_presets() {
        let text = text();
        assert!(text.contains("Available"));
        assert!(!text.contains("Unavailable"));
    }

    #[test]
    fn omission_comes_from_preset_resolution_not_tui() {
        // Cooler-only capabilities: Gaming resolves to cooler_boost alone,
        // with unsupported fields omitted by the presets module.
        let capabilities = crate::hardware::Capabilities {
            cooler_boost: true,
            ..Default::default()
        };
        let profile = BuiltinPreset::Gaming
            .resolve(&capabilities)
            .expect("cooler-only gaming must resolve");
        assert_eq!(profile.performance().fan_mode(), None);
        assert_eq!(profile.performance().shift_mode(), None);
        assert_eq!(profile.performance().cooler_boost(), Some(true));
        assert!(text_with(&capabilities).contains("Available"));
    }

    #[test]
    fn empty_capabilities_show_unavailable_presets() {
        let text = text_with(&crate::hardware::Capabilities::default());
        assert!(text.contains("Unavailable"));
    }

    #[test]
    fn rendering_performs_zero_backend_calls() {
        let (live, calls) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        assert_eq!(calls.get(), 1);
        let _ = screen_text(100, 30, |frame| {
            render_profiles(frame, frame.area(), &live, &capabilities);
        });
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn renders_in_ready_mode() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_profiles(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("BUILT-IN PROFILES"));
    }

    #[test]
    fn renders_in_read_only_mode_without_mutation() {
        let (live, calls) = live_for(
            vec![Ok(healthy_snapshot())],
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
            1,
        );
        let capabilities = crate::hardware::Capabilities::default();
        let text = screen_text(100, 30, |frame| {
            render_profiles(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Profiles"));
        assert!(text.contains("Unavailable"));
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn states_viewing_applies_nothing() {
        assert!(text().contains("Viewing applies nothing"));
    }

    #[test]
    fn tiny_terminal_falls_back_safely() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(20, 8, |frame| {
            render_profiles(frame, frame.area(), &live, &capabilities);
        });
        assert!(!text.is_empty());
    }

    #[test]
    fn zero_area_render_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;

        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_profiles(frame, Rect::new(0, 0, 0, 0), &live, &capabilities);
            })
            .expect("zero-area profiles draws");
    }
}
