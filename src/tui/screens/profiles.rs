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

use crate::tui::profile_catalog::ProfileCatalog;
use crate::tui::theme::Theme;
use crate::tui::ui::{capability_style, render_panel, render_screen_shell};

/// Renders the capability-aware built-in catalog plus prepared custom
/// profiles with the default theme. Pure: draws only supplied data.
pub fn render_profiles<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
) {
    render_profiles_with_theme(frame, area, live, capabilities, catalog, &Theme::default());
}

/// Theme-aware profiles renderer behind the Task-5 API.
pub(crate) fn render_profiles_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Profiles", live, theme);
    let customs = custom_lines(catalog);
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(customs.len() as u16 + 2),
            Constraint::Min(0),
        ])
        .split(content);
    render_panel(
        frame,
        panels[0],
        " BUILT-IN PROFILES ",
        catalog_lines(capabilities, theme),
        theme,
    );
    render_panel(frame, panels[1], " CUSTOM PROFILES ", customs, theme);
    render_panel(
        frame,
        panels[2],
        " NOTE ",
        vec![Line::from(
            "Viewing applies nothing. Interactive apply is added in later PLAN-006 tasks.",
        )],
        theme,
    );
}

/// Custom-file rows from prepared catalog data: never capability
/// "Available" — validity means the file loaded and parsed, not that the
/// laptop can apply it. Unavailable listing degrades to one honest row.
pub(crate) fn custom_lines(catalog: &ProfileCatalog) -> Vec<Line<'static>> {
    if !catalog.customs_available() {
        return vec![Line::from("Custom profiles unavailable")];
    }
    if catalog.customs().is_empty() {
        return vec![Line::from("(none)")];
    }
    catalog
        .customs()
        .iter()
        .map(|entry| {
            if entry.is_valid() {
                Line::from(format!(
                    "{:<15} {:<15} Valid",
                    entry.name().map(|name| name.as_str()).unwrap_or(""),
                    entry.slug().as_str(),
                ))
            } else {
                Line::from(format!("{:<15} Invalid", entry.slug().as_str()))
            }
        })
        .collect()
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
    use crate::tui::ProfileCatalog;

    fn text_with_catalog(
        capabilities: &crate::hardware::Capabilities,
        catalog: &ProfileCatalog,
    ) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        screen_text(100, 30, |frame| {
            render_profiles(frame, frame.area(), &live, capabilities, catalog);
        })
    }

    fn text_with(capabilities: &crate::hardware::Capabilities) -> String {
        text_with_catalog(capabilities, &ProfileCatalog::empty())
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
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &ProfileCatalog::empty(),
            );
        });
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn renders_in_ready_mode() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &ProfileCatalog::empty(),
            );
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
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &ProfileCatalog::empty(),
            );
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
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &ProfileCatalog::empty(),
            );
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
                render_profiles(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &live,
                    &capabilities,
                    &ProfileCatalog::empty(),
                );
            })
            .expect("zero-area profiles draws");
    }

    fn catalog_with(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, ProfileCatalog) {
        use crate::profiles::ProfileStore;

        let dir = tempfile::tempdir().expect("catalog TempDir constructs");
        let store = ProfileStore::new(dir.path().join("profiles"));
        for (name, contents) in entries {
            std::fs::create_dir_all(store.directory()).expect("catalog dir constructs");
            std::fs::write(store.directory().join(name), contents).expect("profile writes");
        }
        let catalog = ProfileCatalog::from_store(&store);
        (dir, catalog)
    }

    const WORK_TOML: &[u8] = b"name = \"My Work\"\n\n[performance]\nfan_mode = \"silent\"\n";

    #[test]
    fn custom_section_header_renders() {
        assert!(text().contains("CUSTOM PROFILES"));
    }

    #[test]
    fn empty_catalog_renders_none() {
        assert!(text().contains("(none)"));
    }

    #[test]
    fn valid_custom_renders_name_slug_and_valid() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let text = text_with_catalog(&full_capabilities(), &catalog);
        assert!(text.contains("CUSTOM PROFILES"));
        assert!(text.contains("My Work"));
        assert!(text.contains("work"));
        assert!(text.contains("Valid"));
    }

    #[test]
    fn invalid_custom_renders_slug_as_invalid() {
        let (_dir, catalog) = catalog_with(&[("bad.toml", b"name = [unclosed\n")]);
        let text = text_with_catalog(&full_capabilities(), &catalog);
        assert!(text.contains("bad"));
        assert!(text.contains("Invalid"));
    }

    #[test]
    fn valid_custom_is_not_labeled_available() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let capabilities = crate::hardware::Capabilities::default();
        let text = text_with_catalog(&capabilities, &catalog);
        // Customs use Valid/Invalid; capability availability belongs to
        // built-ins only. The custom row must not claim Available.
        assert!(text.contains("Valid"));
        let custom_row = text
            .lines()
            .find(|line| line.contains("work"))
            .expect("custom row present");
        assert!(!custom_row.contains("Available"), "{custom_row:?}");
    }

    #[test]
    fn custom_rows_follow_lexical_order() {
        let (_dir, catalog) = catalog_with(&[
            ("work.toml", WORK_TOML),
            ("alpha.toml", WORK_TOML),
            ("bad.toml", b"name = [unclosed\n"),
        ]);
        let text = text_with_catalog(&full_capabilities(), &catalog);
        let alpha = text.find("alpha").expect("alpha present");
        let bad = text.find("bad").expect("bad present");
        let work = text.find("work").expect("work present");
        assert!(alpha < bad && bad < work);
    }

    #[test]
    fn unavailable_catalog_renders_honest_row() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::unavailable());
        assert!(text.contains("Custom profiles unavailable"));
    }

    #[test]
    fn custom_rendering_performs_zero_backend_calls() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let (live, calls) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        assert_eq!(calls.get(), 1);
        let _ = screen_text(100, 30, |frame| {
            render_profiles(frame, frame.area(), &live, &capabilities, &catalog);
        });
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn customs_render_in_read_only_mode() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot())],
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
            1,
        );
        let text = screen_text(100, 30, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                &crate::hardware::Capabilities::default(),
                &catalog,
            );
        });
        assert!(text.contains("CUSTOM PROFILES"));
        assert!(text.contains("My Work"));
    }
}
