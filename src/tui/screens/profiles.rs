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
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use crate::app::{LiveHardware, ProfileSelection};
use crate::hardware::{Capabilities, EcBackend, HardwareSnapshot, SupportMode};
use crate::profiles::{BuiltinPreset, ProfilePlanner};

use crate::tui::controls::{command_text, current_text};
use crate::tui::profile_catalog::ProfileCatalog;
use crate::tui::theme::Theme;
use crate::tui::ui::{capability_style, render_panel, render_screen_shell};

/// Renders the profile list, selected details, and pure capability preview
/// with the default theme. Pure: draws only supplied data; viewing applies
/// nothing and never authorizes execution.
pub fn render_profiles<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &ProfileSelection,
) {
    render_profiles_with_theme(
        frame,
        area,
        live,
        capabilities,
        catalog,
        selection,
        &Theme::default(),
    );
}

/// Theme-aware profiles renderer behind the Task-5 API.
pub(crate) fn render_profiles_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &ProfileSelection,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Profiles", live, theme);
    let rows = profile_rows(capabilities, catalog, selection.index(), theme);
    let details = details_lines(
        selection.index(),
        catalog,
        live.mode(),
        live.current_snapshot(),
        capabilities,
    );
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(rows.len() as u16 + 2),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(content);
    render_panel(frame, panels[0], " PROFILE LIST ", rows, theme);
    render_panel(frame, panels[1], " DETAILS / PREVIEW ", details, theme);
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

/// Total selectable rows: five built-ins, then custom entries in catalog
/// lexical order. Never zero in practice; zero is still safe.
pub(crate) fn profile_row_count(catalog: &ProfileCatalog) -> usize {
    BuiltinPreset::all().len() + catalog.customs().len()
}

/// Which catalog row an index addresses, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProfileRow {
    /// One of the five built-ins in stable order.
    Builtin(BuiltinPreset),
    /// Custom entry by position in catalog lexical order.
    Custom(usize),
}

/// Maps a row index to its catalog entry. Invalid custom entries stay
/// addressable so their status remains inspectable.
pub(crate) fn profile_row(index: usize, catalog: &ProfileCatalog) -> Option<ProfileRow> {
    let builtins = BuiltinPreset::all();
    if index < builtins.len() {
        return Some(ProfileRow::Builtin(builtins[index]));
    }
    catalog
        .customs()
        .get(index - builtins.len())
        .map(|_| ProfileRow::Custom(index - builtins.len()))
}

/// Combined selectable rows with the selected marker. The marker uses the
/// semantic primary role plus bold; unselected rows keep their
/// capability/validity styling.
fn profile_rows(
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selected: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut rows = catalog_lines(capabilities, selected, theme);
    rows.extend(custom_lines(
        catalog,
        BuiltinPreset::all().len(),
        selected,
        theme,
    ));
    rows
}

/// Custom-file rows from prepared catalog data: never capability
/// "Available" — validity means the file loaded and parsed, not that the
/// laptop can apply it. Unavailable listing degrades to one honest row.
/// `base` is the global index of the first custom row; `selected` marks it.
pub(crate) fn custom_lines(
    catalog: &ProfileCatalog,
    base: usize,
    selected: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if !catalog.customs_available() {
        return vec![Line::from("Custom profiles unavailable")];
    }
    if catalog.customs().is_empty() {
        return vec![Line::from("(none)")];
    }
    catalog
        .customs()
        .iter()
        .enumerate()
        .map(|(position, entry)| {
            let marked = selected == base + position;
            let text = if entry.is_valid() {
                format!(
                    "{:<15} {:<15} Valid",
                    entry.name().map(|name| name.as_str()).unwrap_or(""),
                    entry.slug().as_str(),
                )
            } else {
                format!("{:<15} Invalid", entry.slug().as_str())
            };
            styled_row(marked, text, theme)
        })
        .collect()
}

/// One catalog row per built-in preset in stable order. Availability comes
/// solely from [`BuiltinPreset::resolve`]: an `Unavailable` result — or a
/// conservative error state for an unexpected internal definition — never
/// claims availability and never panics. `selected` is the global row
/// index; built-ins occupy rows `0..5`.
pub(crate) fn catalog_lines(
    capabilities: &Capabilities,
    selected: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    BuiltinPreset::all()
        .iter()
        .enumerate()
        .map(|(position, preset)| {
            let available = preset.resolve(capabilities).is_ok();
            let text = format!(
                "{:<15} {:<15} {}",
                preset.name(),
                preset.slug(),
                if available {
                    "Available"
                } else {
                    "Unavailable"
                },
            );
            marked_line(
                selected == position,
                text,
                capability_style(available, theme),
                theme,
            )
        })
        .collect()
}

/// Selected-row marker: primary plus bold. Unselected rows keep the
/// caller's plain presentation.
fn styled_row(marked: bool, text: String, theme: &Theme) -> Line<'static> {
    marked_line(marked, text, Style::default(), theme)
}

/// Marker with an explicit unselected style.
fn marked_line(marked: bool, text: String, plain: Style, theme: &Theme) -> Line<'static> {
    if marked {
        Line::styled(
            format!("> {text}"),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Line::styled(format!("  {text}"), plain)
    }
}

/// Details and pure capability preview for one selected row. The preview
/// comes from [`ProfilePlanner::preview`] against the supplied live mode
/// and startup capabilities: descriptive only, never authorization, never
/// execution. Unavailable built-ins and invalid customs show status
/// without any fabricated profile or preview.
pub(crate) fn details_lines(
    index: usize,
    catalog: &ProfileCatalog,
    mode: &SupportMode,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
) -> Vec<Line<'static>> {
    let Some(row) = profile_row(index, catalog) else {
        return vec![Line::from("No profile selected")];
    };
    match row {
        ProfileRow::Builtin(preset) => {
            let mut lines = vec![
                Line::from(format!("Name: {}", preset.name())),
                Line::from(format!("Slug: {}", preset.slug())),
                Line::from("Source: built-in"),
            ];
            match preset.resolve(capabilities) {
                Ok(profile) => {
                    lines.push(Line::from("Capability: Available"));
                    lines.extend(preview_lines(&profile, mode, snapshot, capabilities));
                }
                Err(_) => {
                    lines.push(Line::from("Capability: Unavailable"));
                    lines.push(Line::from("Preview: unavailable preset"));
                }
            }
            lines
        }
        ProfileRow::Custom(position) => {
            let entry = &catalog.customs()[position];
            let mut lines = vec![
                Line::from(format!("Slug: {}", entry.slug().as_str())),
                Line::from("Source: custom"),
            ];
            match entry.profile() {
                Some(profile) => {
                    lines.push(Line::from(format!("Name: {}", profile.name())));
                    lines.push(Line::from("File: Valid"));
                    lines.extend(preview_lines(profile, mode, snapshot, capabilities));
                }
                None => {
                    lines.push(Line::from("Status: Invalid file"));
                    lines.push(Line::from("Preview: unavailable"));
                }
            }
            lines
        }
    }
}

/// Pure preview rows: each requested command with its current value and
/// its typed validation result. READ-ONLY mode rejects here visibly; a
/// Valid file is never presented as hardware-applicable on that basis.
fn preview_lines(
    profile: &crate::profiles::Profile,
    mode: &SupportMode,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
) -> Vec<Line<'static>> {
    let preview = ProfilePlanner::preview(profile, mode, capabilities);
    let mut lines = vec![Line::from("Preview:")];
    for entry in preview.entries() {
        let status = match entry.status() {
            crate::profiles::ProfilePreviewStatus::Applicable => "Applicable".to_owned(),
            crate::profiles::ProfilePreviewStatus::Rejected(error) => {
                format!("Rejected: {error}")
            }
        };
        lines.push(Line::from(format!(
            "{} (current: {}) — {status}",
            command_text(entry.command()),
            current_text(entry.command(), snapshot),
            status = status,
        )));
    }
    lines
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
            render_profiles(
                frame,
                frame.area(),
                &live,
                capabilities,
                catalog,
                &crate::app::ProfileSelection::default(),
            );
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
                &crate::app::ProfileSelection::default(),
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
                &crate::app::ProfileSelection::default(),
            );
        });
        assert!(text.contains("PROFILE LIST"));
        assert!(text.contains("DETAILS / PREVIEW"));
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
                &crate::app::ProfileSelection::default(),
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
                &crate::app::ProfileSelection::default(),
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
                    &crate::app::ProfileSelection::default(),
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
        assert!(text().contains("PROFILE LIST"));
    }

    #[test]
    fn empty_catalog_renders_none() {
        assert!(text().contains("(none)"));
    }

    #[test]
    fn valid_custom_renders_name_slug_and_valid() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let text = text_with_catalog(&full_capabilities(), &catalog);
        assert!(text.contains("PROFILE LIST"));
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
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &catalog,
                &crate::app::ProfileSelection::default(),
            );
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
                &crate::app::ProfileSelection::default(),
            );
        });
        assert!(text.contains("PROFILE LIST"));
        assert!(text.contains("My Work"));
    }

    // ---- Task 3: selection order + details/preview ----

    fn text_with_selection(
        capabilities: &crate::hardware::Capabilities,
        catalog: &ProfileCatalog,
        index: usize,
    ) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut selection = crate::app::ProfileSelection::default();
        for _ in 0..index {
            selection.move_down(super::profile_row_count(catalog));
        }
        screen_text(100, 30, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                capabilities,
                catalog,
                &selection,
            );
        })
    }

    fn details_text(
        index: usize,
        catalog: &ProfileCatalog,
        mode: &SupportMode,
        capabilities: &crate::hardware::Capabilities,
    ) -> String {
        super::details_lines(
            index,
            catalog,
            mode,
            Some(&healthy_snapshot()),
            capabilities,
        )
        .into_iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn row_count_is_five_builtins_plus_customs() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        assert_eq!(
            super::profile_row_count(&catalog),
            BuiltinPreset::all().len() + 1
        );
        assert_eq!(
            super::profile_row_count(&ProfileCatalog::empty()),
            BuiltinPreset::all().len()
        );
    }

    #[test]
    fn rows_place_builtins_before_customs_in_catalog_order() {
        let (_dir, catalog) = catalog_with(&[
            ("work.toml", WORK_TOML),
            ("alpha.toml", WORK_TOML),
            ("bad.toml", b"name = [unclosed\n"),
        ]);
        // Built-ins occupy 0..5 in stable order.
        for (index, preset) in BuiltinPreset::all().into_iter().enumerate() {
            assert_eq!(
                super::profile_row(index, &catalog),
                Some(super::ProfileRow::Builtin(preset))
            );
        }
        // Customs follow in lexical order: alpha, bad, work.
        assert_eq!(
            super::profile_row(5, &catalog),
            Some(super::ProfileRow::Custom(0))
        );
        assert_eq!(
            super::profile_row(6, &catalog),
            Some(super::ProfileRow::Custom(1))
        );
        assert_eq!(
            super::profile_row(7, &catalog),
            Some(super::ProfileRow::Custom(2))
        );
        assert_eq!(super::profile_row(8, &catalog), None);
        assert_eq!(catalog.customs()[0].slug().as_str(), "alpha");
        assert_eq!(catalog.customs()[1].slug().as_str(), "bad");
        assert_eq!(catalog.customs()[2].slug().as_str(), "work");
    }

    #[test]
    fn rendered_list_places_builtins_before_customs() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let text = text_with_catalog(&full_capabilities(), &catalog);
        let gaming = text.find("Gaming").expect("builtin present");
        let work = text.find("work").expect("custom present");
        assert!(gaming < work);
    }

    #[test]
    fn invalid_custom_row_is_addressable() {
        let (_dir, catalog) = catalog_with(&[("bad.toml", b"name = [unclosed\n")]);
        assert_eq!(
            super::profile_row(5, &catalog),
            Some(super::ProfileRow::Custom(0))
        );
        assert!(!catalog.customs()[0].is_valid());
        let details = details_text(5, &catalog, &SupportMode::Ready, &full_capabilities());
        assert!(details.contains("Invalid"));
    }

    #[test]
    fn move_down_traverses_builtins_into_customs() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let count = super::profile_row_count(&catalog);
        assert_eq!(count, 6);
        let mut selection = crate::app::ProfileSelection::default();
        assert_eq!(selection.index(), 0);
        for expected in 1..count {
            selection.move_down(count);
            assert_eq!(selection.index(), expected);
        }
        // Wrap is deterministic: past the last custom returns to first built-in.
        selection.move_down(count);
        assert_eq!(selection.index(), 0);
    }

    #[test]
    fn move_up_wraps_deterministically() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let count = super::profile_row_count(&catalog);
        let mut selection = crate::app::ProfileSelection::default();
        selection.move_up(count);
        assert_eq!(selection.index(), count - 1);
        selection.move_up(count);
        assert_eq!(selection.index(), count - 2);
    }

    #[test]
    fn selection_never_indexes_out_of_bounds() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let count = super::profile_row_count(&catalog);
        let mut selection = crate::app::ProfileSelection::default();
        for _ in 0..(count * 3 + 1) {
            selection.move_down(count);
            assert!(super::profile_row(selection.index(), &catalog).is_some());
        }
        for _ in 0..(count * 3 + 1) {
            selection.move_up(count);
            assert!(super::profile_row(selection.index(), &catalog).is_some());
        }
        selection.clamp(count);
        assert!(super::profile_row(selection.index(), &catalog).is_some());
        // Empty set stays pinned without a row.
        let mut empty = crate::app::ProfileSelection::default();
        empty.move_down(0);
        empty.move_up(0);
        assert_eq!(empty.index(), 0);
        assert!(super::profile_row(0, &ProfileCatalog::empty()).is_some());
        assert!(super::profile_row(99, &ProfileCatalog::empty()).is_none());
    }

    #[test]
    fn selected_builtin_details_render_exact_source_name_slug() {
        let details = details_text(
            0,
            &ProfileCatalog::empty(),
            &SupportMode::Ready,
            &full_capabilities(),
        );
        assert!(details.contains("Name: Balanced"));
        assert!(details.contains("Slug: balanced"));
        assert!(details.contains("Source: built-in"));
    }

    #[test]
    fn selected_custom_details_render_source_name_slug() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let details = details_text(5, &catalog, &SupportMode::Ready, &full_capabilities());
        assert!(details.contains("Slug: work"));
        assert!(details.contains("Source: custom"));
        assert!(details.contains("Name: My Work"));
        assert!(details.contains("File: Valid"));
    }

    #[test]
    fn invalid_custom_details_show_invalid_without_fabricated_preview() {
        let (_dir, catalog) = catalog_with(&[("bad.toml", b"name = [unclosed\n")]);
        let details = details_text(5, &catalog, &SupportMode::Ready, &full_capabilities());
        assert!(details.contains("Slug: bad"));
        assert!(details.contains("Source: custom"));
        assert!(details.contains("Invalid"));
        assert!(details.contains("Preview: unavailable"));
        assert!(!details.contains("Applicable"));
        assert!(!details.contains("Fan Mode"));
    }

    #[test]
    fn builtin_availability_comes_from_preset_resolve() {
        let empty_caps = crate::hardware::Capabilities::default();
        let available = details_text(
            0,
            &ProfileCatalog::empty(),
            &SupportMode::Ready,
            &full_capabilities(),
        );
        assert!(available.contains("Capability: Available"));
        let unavailable = details_text(
            0,
            &ProfileCatalog::empty(),
            &SupportMode::Ready,
            &empty_caps,
        );
        assert!(unavailable.contains("Capability: Unavailable"));
        assert!(unavailable.contains("Preview: unavailable preset"));
        assert!(!unavailable.contains("Applicable"));
    }

    #[test]
    fn preview_applicability_comes_from_planner() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let applicable = details_text(5, &catalog, &SupportMode::Ready, &full_capabilities());
        assert!(applicable.contains("Preview:"));
        assert!(applicable.contains("Applicable"));
        let rejected = details_text(
            5,
            &catalog,
            &SupportMode::Ready,
            &crate::hardware::Capabilities::default(),
        );
        assert!(rejected.contains("Rejected:"));
    }

    #[test]
    fn read_only_preview_rejects_visibly() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let mode = SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable);
        let details = details_text(5, &catalog, &mode, &full_capabilities());
        assert!(details.contains("Rejected:"));
        assert!(details.contains("read-only"));
        let builtin = details_text(0, &ProfileCatalog::empty(), &mode, &full_capabilities());
        assert!(builtin.contains("Rejected:"));
    }

    #[test]
    fn full_render_shows_list_details_and_selected_marker() {
        let text = text_with_selection(&full_capabilities(), &ProfileCatalog::empty(), 0);
        assert!(text.contains("PROFILE LIST"));
        assert!(text.contains("DETAILS / PREVIEW"));
        assert!(text.contains("> Balanced"));
        assert!(text.contains("Source: built-in"));
        let moved = text_with_selection(&full_capabilities(), &ProfileCatalog::empty(), 2);
        assert!(moved.contains("> Gaming"));
        assert!(!moved.contains("> Balanced"));
    }

    #[test]
    fn selected_custom_marker_and_details_render() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let text = text_with_selection(&full_capabilities(), &catalog, 5);
        assert!(text.contains("> My Work"));
        assert!(text.contains("Source: custom"));
        assert!(text.contains("Slug: work"));
    }

    #[test]
    fn rendering_with_selection_performs_zero_backend_calls() {
        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let (live, calls) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut selection = crate::app::ProfileSelection::default();
        for _ in 0..5 {
            selection.move_down(super::profile_row_count(&catalog));
        }
        assert_eq!(calls.get(), 1);
        let _ = screen_text(100, 30, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &catalog,
                &selection,
            );
        });
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn rendering_needs_no_filesystem_after_catalog_is_prepared() {
        let dir = tempfile::tempdir().expect("prepared TempDir constructs");
        let catalog = {
            let store = crate::profiles::ProfileStore::new(dir.path().join("profiles"));
            std::fs::create_dir_all(store.directory()).expect("profile dir constructs");
            std::fs::write(store.directory().join("work.toml"), WORK_TOML).expect("profile writes");
            ProfileCatalog::from_store(&store)
        };
        // Remove the source files: the retained catalog must render alone.
        std::fs::remove_dir_all(dir.path().join("profiles")).expect("source removed");
        let text = text_with_catalog(&full_capabilities(), &catalog);
        assert!(text.contains("My Work"));
        assert!(text.contains("Valid"));
        let details = details_text(5, &catalog, &SupportMode::Ready, &full_capabilities());
        assert!(details.contains("Name: My Work"));
    }

    #[test]
    fn compact_identifies_selected_profile_and_status() {
        use super::super::super::responsive::render_compact_screen;
        use crate::app::AppState;
        use crate::tui::theme::Theme;

        let (_dir, catalog) = catalog_with(&[("work.toml", WORK_TOML)]);
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let mut app = AppState::default();
        app.apply(crate::app::AppAction::GoTo(crate::app::Screen::Profiles));
        let mut selection = crate::app::ProfileSelection::default();
        for _ in 0..5 {
            selection.move_down(super::profile_row_count(&catalog));
        }
        let text = screen_text(50, 16, |frame| {
            render_compact_screen(
                frame,
                frame.area(),
                &app,
                &live,
                &capabilities,
                &catalog,
                &selection,
                &Theme::default(),
            );
        });
        assert!(text.contains("Profiles"));
        assert!(text.contains("> My Work"));
        assert!(text.contains("Valid"));
    }

    #[test]
    fn tiny_profiles_screen_remains_safe() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let text = screen_text(20, 8, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                &capabilities,
                &ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
            );
        });
        assert!(!text.is_empty());
    }
}
