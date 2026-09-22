//! Profiles screen in the approved v1.1 card system.
//!
//! Renders the real built-in catalog plus prepared custom entries with
//! capability-aware availability. Viewing applies nothing: the renderer
//! only calls [`BuiltinPreset::resolve`] on injected data, never samples
//! hardware, discovers capabilities, loads profile files, or writes. The
//! profiles module owns every recipe; no mode policy lives here. There is
//! no authoritative active-profile tracking, so no ACTIVE label is shown:
//! rows carry availability plus the cursor selection.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware, ProfileSelection};
use crate::hardware::{Capabilities, EcBackend, HardwareSnapshot, SupportMode};
use crate::profiles::{BuiltinPreset, ProfilePlanner};

use crate::tui::controls::{command_text, current_text};
use crate::tui::profile_catalog::ProfileCatalog;
use crate::tui::shell;
use crate::tui::theme::Theme;
use crate::tui::ui::{capability_style, support_mode_style, support_mode_text};

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
        &AppState::default(),
        live,
        capabilities,
        catalog,
        selection,
        &Theme::default(),
    );
}

/// Theme-aware profiles renderer: approved shell plus real catalog state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_profiles_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &ProfileSelection,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme.base_style()),
        area,
    );
    let (top, workspace, footer) = shell::shell_split(area);
    shell::render_top_strip(frame, top, live, theme);
    shell::render_bottom_strip(frame, footer, live, theme);
    let row_count = profile_row_count(catalog);
    let regions = hit_regions(workspace, row_count);
    if regions.cards.len() != 4 {
        return;
    }
    let focus = app.focused_card();
    render_table_card(
        frame,
        regions.cards[0],
        capabilities,
        catalog,
        selection,
        live.current_snapshot(),
        focus == 0,
        theme,
    );
    render_details_card(
        frame,
        regions.cards[1],
        selection.index(),
        catalog,
        live.mode(),
        live.current_snapshot(),
        capabilities,
        focus == 1,
        theme,
    );
    render_summary_card(
        frame,
        regions.cards[2],
        selection.index(),
        catalog,
        live.mode(),
        live.current_snapshot(),
        capabilities,
        focus == 2,
        theme,
    );
    render_action_card(frame, regions.cards[3], live, focus == 3, theme);
}

/// Card layout in focus order: table, details, summary, action. Table
/// rows start below the header line in global row order; rows that cannot
/// fit stay undrawn and unclickable.
pub(crate) fn hit_regions(workspace: Rect, row_count: usize) -> shell::ScreenRegions {
    let (table, side) = shell::hpair(workspace, 60);
    let (side_top, side_rest) = shell::vsplit2(side, 40);
    let (side_mid, side_bot) = shell::vsplit2(side_rest, 55);
    let cards = vec![table, side_top, side_mid, side_bot];
    let inner = shell::inset(table);
    let fit = (inner.height.saturating_sub(1)) as usize;
    let rows = (0..row_count.min(fit))
        .map(|i| shell::row_rect(inner, i + 1))
        .collect();
    shell::ScreenRegions { cards, rows }
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

/// Resolved SHIFT/FAN/BOOST values for one row, or placeholders when the
/// row cannot resolve against current capabilities. Never invented.
fn row_values(
    row: ProfileRow,
    catalog: &ProfileCatalog,
    capabilities: &Capabilities,
) -> (String, String, String) {
    let profile = match row {
        ProfileRow::Builtin(preset) => preset.resolve(capabilities).ok(),
        ProfileRow::Custom(position) => catalog
            .customs()
            .get(position)
            .and_then(|entry| entry.profile().cloned()),
    };
    match profile {
        Some(profile) => {
            let perf = profile.performance();
            (
                perf.shift_mode()
                    .map(|m| m.as_str().to_owned())
                    .unwrap_or_else(|| "—".to_owned()),
                perf.fan_mode()
                    .map(|m| m.as_str().to_owned())
                    .unwrap_or_else(|| "—".to_owned()),
                perf.cooler_boost()
                    .map(|b| if b { "on".to_owned() } else { "off".to_owned() })
                    .unwrap_or_else(|| "—".to_owned()),
            )
        }
        None => ("—".to_owned(), "—".to_owned(), "—".to_owned()),
    }
}

/// Availability status for one row. No active-profile tracking exists, so
/// rows never claim ACTIVE.
fn row_status(
    row: ProfileRow,
    catalog: &ProfileCatalog,
    capabilities: &Capabilities,
) -> (&'static str, bool) {
    match row {
        ProfileRow::Builtin(preset) => {
            if preset.resolve(capabilities).is_ok() {
                ("Available", true)
            } else {
                ("Unavailable", false)
            }
        }
        ProfileRow::Custom(position) => match catalog.customs().get(position) {
            Some(entry) if entry.is_valid() => ("Available", true),
            Some(_) => ("Invalid", false),
            None => ("Unavailable", false),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn render_table_card(
    frame: &mut Frame,
    area: Rect,
    capabilities: &Capabilities,
    catalog: &ProfileCatalog,
    selection: &ProfileSelection,
    snapshot: Option<&HardwareSnapshot>,
    focused: bool,
    theme: &Theme,
) {
    let _ = snapshot;
    let inner = shell::card(frame, area, "PROFILES", focused, theme);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{:<16}", "PROFILE"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<9}", "SOURCE"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<10}", "SHIFT"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<10}", "FAN"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<7}", "BOOST"),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "STATUS",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    let fit = (inner.height.saturating_sub(1)) as usize;
    for index in 0..profile_row_count(catalog).min(fit.max(1)) {
        let Some(row) = profile_row(index, catalog) else {
            continue;
        };
        let selected = selection.index() == index;
        let (name, source) = match row {
            ProfileRow::Builtin(preset) => (preset.name().to_owned(), "built-in".to_owned()),
            ProfileRow::Custom(position) => (
                catalog.customs()[position]
                    .name()
                    .map(|n| n.as_str().to_owned())
                    .unwrap_or_else(|| catalog.customs()[position].slug().as_str().to_owned()),
                "custom".to_owned(),
            ),
        };
        let (shift, fan, boost) = row_values(row, catalog, capabilities);
        let (status, ok) = row_status(row, catalog, capabilities);
        let marker = if selected { "▸ " } else { "  " };
        lines.push(Line::from(vec![
            Span::styled(marker.to_owned(), Style::default().fg(theme.accent)),
            Span::styled(
                format!("{name:<15}"),
                if selected {
                    Style::default()
                        .fg(theme.foreground)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.foreground)
                },
            ),
            Span::styled(format!("{source:<9}"), Style::default().fg(theme.muted)),
            Span::styled(
                format!("{shift:<10}"),
                Style::default().fg(theme.foreground),
            ),
            Span::styled(format!("{fan:<10}"), Style::default().fg(theme.foreground)),
            Span::styled(format!("{boost:<7}"), Style::default().fg(theme.foreground)),
            Span::styled(
                status.to_owned(),
                if ok {
                    Style::default().fg(theme.success)
                } else {
                    Style::default().fg(theme.muted)
                },
            ),
        ]));
    }
    if catalog.customs().is_empty() {
        lines.push(Line::styled(
            "(none) custom profiles",
            Style::default().fg(theme.muted),
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Enter previews · review confirms · nothing applies early",
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_details_card(
    frame: &mut Frame,
    area: Rect,
    index: usize,
    catalog: &ProfileCatalog,
    mode: &SupportMode,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "PROFILE DETAILS", focused, theme);
    let lines = details_lines(index, catalog, mode, snapshot, capabilities);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_summary_card(
    frame: &mut Frame,
    area: Rect,
    index: usize,
    catalog: &ProfileCatalog,
    mode: &SupportMode,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "CHANGE SUMMARY", focused, theme);
    let mut lines = Vec::new();
    let rows = summary_rows(index, catalog, mode, snapshot, capabilities);
    if rows.is_empty() {
        lines.push(Line::styled(
            "No applicable changes for this row",
            Style::default().fg(theme.muted),
        ));
    }
    let mut changed = 0;
    for row in &rows {
        if row.changed {
            changed += 1;
            lines.push(Line::from(vec![
                Span::styled("· ", Style::default().fg(theme.warning)),
                Span::styled(
                    format!("{}: {} → {}", row.setting, row.current, row.requested),
                    Style::default().fg(theme.warning),
                ),
            ]));
        } else {
            lines.push(Line::styled(
                format!("· {}: {} (same)", row.setting, row.current),
                Style::default().fg(theme.muted),
            ));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        format!("{changed} changes · {} unchanged", rows.len() - changed),
        Style::default().fg(theme.muted),
    ));
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_action_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "ACTION / SAFETY", focused, theme);
    let lines = vec![
        Line::styled("[ REVIEW CHANGES ]", Style::default().fg(theme.accent)),
        Line::from(""),
        Line::from(vec![
            Span::styled("Access: ", Style::default().fg(theme.muted)),
            Span::styled(
                support_mode_text(live.mode()).to_owned(),
                support_mode_style(live.mode(), theme),
            ),
        ]),
        Line::styled(
            "Nothing applies until review is confirmed",
            Style::default().fg(theme.warning),
        ),
        Line::styled(
            "Enter previews · Esc cancels",
            Style::default().fg(theme.muted),
        ),
    ];
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

/// One previewed setting against current hardware state, derived from the
/// existing [`ProfilePlanner::preview`]. Presentation only.
struct SummaryRow {
    setting: String,
    current: String,
    requested: String,
    changed: bool,
}

fn summary_rows(
    index: usize,
    catalog: &ProfileCatalog,
    mode: &SupportMode,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
) -> Vec<SummaryRow> {
    let Some(row) = profile_row(index, catalog) else {
        return Vec::new();
    };
    let profile = match row {
        ProfileRow::Builtin(preset) => preset.resolve(capabilities).ok(),
        ProfileRow::Custom(position) => catalog
            .customs()
            .get(position)
            .and_then(|entry| entry.profile().cloned()),
    };
    let Some(profile) = profile else {
        return Vec::new();
    };
    let preview = ProfilePlanner::preview(&profile, mode, capabilities);
    preview
        .entries()
        .iter()
        .map(|entry| {
            let requested_full = command_text(entry.command());
            let (setting, requested) = requested_full
                .split_once(": ")
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
                .unwrap_or((requested_full.clone(), requested_full));
            let current = current_text(entry.command(), snapshot);
            let applicable = matches!(
                entry.status(),
                crate::profiles::ProfilePreviewStatus::Applicable
            );
            SummaryRow {
                setting,
                current: current.clone(),
                requested: requested.clone(),
                changed: applicable && current != requested,
            }
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use crate::hardware::SupportMode;

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::{hit_regions, render_profiles};
    use crate::tui::ProfileCatalog;

    fn text_with_catalog(
        capabilities: &crate::hardware::Capabilities,
        catalog: &ProfileCatalog,
    ) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        screen_text(160, 50, |frame| {
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

    #[test]
    fn renders_shell_and_card_headings() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        for heading in [
            "PROFILES",
            "PROFILE DETAILS",
            "CHANGE SUMMARY",
            "ACTION / SAFETY",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
    }

    #[test]
    fn renders_table_columns() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        for heading in ["PROFILE", "SOURCE", "SHIFT", "FAN", "BOOST", "STATUS"] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
    }

    #[test]
    fn lists_all_five_builtins_with_availability() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        for name in [
            "Balanced",
            "Silent",
            "Gaming",
            "Battery Saver",
            "Maximum Cooling",
        ] {
            assert!(text.contains(name), "{name:?} missing");
        }
        assert!(text.contains("Available"));
    }

    #[test]
    fn shows_resolved_preset_values_not_placeholders() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        assert!(text.contains("comfort") || text.contains("sport"));
        assert!(text.contains("(none) custom profiles") || text.contains("(none)"));
    }

    #[test]
    fn unavailable_preset_shows_placeholders_honestly() {
        // Empty every capability: recipes with zero surviving settings
        // fail closed, and their table cells stay honest placeholders.
        let mut caps = full_capabilities();
        caps.shift_modes.clear();
        caps.fan_modes.clear();
        caps.cooler_boost = false;
        caps.super_battery = false;
        let text = text_with_catalog(&caps, &ProfileCatalog::empty());
        assert!(text.contains("Unavailable"));
        assert!(text.contains("—"));
    }

    #[test]
    fn empty_customs_show_none_row() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        assert!(text.contains("(none)"));
    }

    #[test]
    fn details_show_selected_preset_and_preview() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        assert!(text.contains("Name: Balanced"));
        assert!(text.contains("Slug: balanced"));
        assert!(text.contains("Source: built-in"));
        assert!(text.contains("Preview:"));
    }

    #[test]
    fn summary_derives_changes_from_preview() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        assert!(text.contains("changes ·") || text.contains("unchanged"));
    }

    #[test]
    fn action_card_states_review_safety() {
        let text = text_with_catalog(&full_capabilities(), &ProfileCatalog::empty());
        assert!(text.contains("REVIEW CHANGES"));
        assert!(text.contains("Nothing applies until review is confirmed"));
    }

    #[test]
    fn read_only_preview_rejects_visibly() {
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot())],
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
            1,
        );
        let text = screen_text(160, 50, |frame| {
            render_profiles(
                frame,
                frame.area(),
                &live,
                &full_capabilities(),
                &ProfileCatalog::empty(),
                &crate::app::ProfileSelection::default(),
            );
        });
        assert!(text.contains("Rejected"));
    }

    #[test]
    fn hit_regions_match_drawn_profile_rows() {
        let catalog = ProfileCatalog::empty();
        let count = super::profile_row_count(&catalog);
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48), count);
        assert_eq!(regions.cards.len(), 4);
        assert_eq!(regions.rows.len(), count);
    }

    #[test]
    fn zero_area_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_profiles(
                    frame,
                    Rect::new(0, 0, 0, 0),
                    &live,
                    &full_capabilities(),
                    &ProfileCatalog::empty(),
                    &crate::app::ProfileSelection::default(),
                );
            })
            .expect("zero-area profiles draws");
    }

    #[test]
    fn prepared_customs_stay_inspectable() {
        let dir = tempfile::tempdir().expect("customs TempDir constructs");
        let store = crate::profiles::ProfileStore::new(dir.path().join("profiles"));
        std::fs::create_dir_all(store.directory()).expect("customs dir constructs");
        std::fs::write(
            store.directory().join("work.toml"),
            b"name = \"Inspect Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        )
        .expect("custom profile writes");
        let catalog = ProfileCatalog::from_store(&store);
        let text = text_with_catalog(&full_capabilities(), &catalog);
        assert!(text.contains("Inspect Work") || text.contains("work"));
        assert!(text.contains("custom"));
    }
}
