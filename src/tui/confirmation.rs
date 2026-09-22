//! Typed pending confirmations and result notices.
//!
//! [`PendingMutation`] is the single modal confirmation state: either one
//! typed [`HardwareCommand`] or one retained [`Profile`] with safe display
//! metadata. It never stores paths, shell strings, EC addresses, or raw
//! write data. The stored preview is presentation only; production
//! execution re-evaluates everything fresh.
//!
//! [`Notice`] is the minimal post-attempt banner: success or failure text
//! rendered with semantic styles. No queues, timers, or animation.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::hardware::{Capabilities, HardwareCommand, HardwareSnapshot, SupportMode};
use crate::profiles::{Profile, ProfilePlanner};

use super::theme::Theme;

/// Where a pending profile came from, for honest display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    /// One of the five built-ins.
    Builtin,
    /// A valid custom file retained in the catalog.
    Custom,
}

impl ProfileSource {
    /// Display text: built-in vs custom.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "built-in",
            Self::Custom => "custom",
        }
    }
}

/// Retained profile awaiting confirmation: pure data plus safe metadata.
/// The profile was resolved (built-in) or retained (custom) without
/// reopening files; execution still re-evaluates fresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePending {
    profile: Profile,
    slug: String,
    source: ProfileSource,
}

impl ProfilePending {
    /// Retains profile data with display metadata.
    pub fn new(profile: Profile, slug: String, source: ProfileSource) -> Self {
        Self {
            profile,
            slug,
            source,
        }
    }

    /// Retained profile data.
    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// Stable slug for display.
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Built-in vs custom.
    pub fn source(&self) -> ProfileSource {
        self.source
    }
}

/// Single modal confirmation: at most one exists at a time. While one
/// exists, navigation and new edits are blocked; only confirm or cancel
/// applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingMutation {
    /// One typed hardware command awaiting a single confirm.
    Command(HardwareCommand),
    /// One retained profile awaiting a single apply.
    Profile(ProfilePending),
}

/// Minimal post-attempt banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    kind: NoticeKind,
    message: String,
}

/// Success vs failure styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Green success.
    Success,
    /// Red failure.
    Failure,
}

impl Notice {
    /// Success banner.
    pub fn success(message: String) -> Self {
        Self {
            kind: NoticeKind::Success,
            message,
        }
    }

    /// Failure banner with a Display-rendered safe error.
    pub fn failure(message: String) -> Self {
        Self {
            kind: NoticeKind::Failure,
            message,
        }
    }

    /// Styling verdict.
    pub fn kind(&self) -> NoticeKind {
        self.kind
    }

    /// Banner text (Display only, never Debug).
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// Title for the modal dialog.
pub(crate) fn confirmation_title(pending: &PendingMutation) -> &'static str {
    match pending {
        PendingMutation::Command(_) => " Confirm Hardware Change ",
        PendingMutation::Profile(_) => " Confirm Profile Apply ",
    }
}

/// Centered dialog geometry with saturating math so tiny and zero areas
/// stay panic-free.
fn overlay_area(area: Rect, line_count: usize) -> Rect {
    let width = area.width.saturating_sub(4).min(64);
    let height = area.height.saturating_sub(2).min(line_count as u16 + 2);
    let x = area.x.saturating_add(area.width.saturating_sub(width) / 2);
    let y = area
        .y
        .saturating_add(area.height.saturating_sub(height) / 2);
    Rect::new(x, y, width, height)
}

/// One review table row: setting, current, requested, and whether the
/// requested value differs. Presentation only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRow {
    /// Setting name (e.g. "Fan Mode").
    pub setting: String,
    /// Current snapshot value (or "unknown").
    pub current: String,
    /// Requested value.
    pub requested: String,
    /// True when requested differs from current.
    pub changed: bool,
}

/// Pure review table for one pending mutation, derived from the same
/// typed request/current helpers as before plus the existing profile
/// preview. No paths, no RPM.
pub(crate) fn review_rows(
    pending: &PendingMutation,
    snapshot: Option<&HardwareSnapshot>,
    mode: &SupportMode,
    capabilities: &Capabilities,
) -> Vec<ReviewRow> {
    match pending {
        PendingMutation::Command(command) => {
            let requested_full = super::controls::command_text(command);
            let (setting, requested) = requested_full
                .split_once(": ")
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
                .unwrap_or((requested_full.clone(), requested_full));
            let current = super::controls::current_text(command, snapshot);
            vec![ReviewRow {
                changed: current != requested,
                setting,
                current,
                requested,
            }]
        }
        PendingMutation::Profile(request) => {
            let preview = ProfilePlanner::preview(request.profile(), mode, capabilities);
            preview
                .entries()
                .iter()
                .filter_map(|entry| {
                    let applicable = matches!(
                        entry.status(),
                        crate::profiles::ProfilePreviewStatus::Applicable
                    );
                    if !applicable {
                        return None;
                    }
                    let requested_full = super::controls::command_text(entry.command());
                    let (setting, requested) = requested_full
                        .split_once(": ")
                        .map(|(a, b)| (a.to_owned(), b.to_owned()))
                        .unwrap_or((requested_full.clone(), requested_full));
                    let current = super::controls::current_text(entry.command(), snapshot);
                    Some(ReviewRow {
                        changed: current != requested,
                        setting,
                        current,
                        requested,
                    })
                })
                .collect()
        }
    }
}

/// Renders the modal confirmation as the approved review workspace:
/// REVIEW CHANGES with a SETTING/CURRENT/REQUESTED/RESULT table, change
/// counts, an explicit nothing-applied statement, and Cancel/Apply
/// actions. Keyboard semantics are unchanged: Enter applies once, Esc
/// cancels. Safe for tiny and zero areas.
pub(crate) fn render_confirmation(
    frame: &mut Frame,
    area: Rect,
    pending: &PendingMutation,
    snapshot: Option<&HardwareSnapshot>,
    mode: &SupportMode,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let rows = review_rows(pending, snapshot, mode, capabilities);
    let changed = rows.iter().filter(|row| row.changed).count();
    let mut lines = vec![
        Line::styled(
            confirmation_title(pending).trim().to_owned(),
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("{:<18}", "SETTING"),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<14}", "CURRENT"),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{:<14}", "REQUESTED"),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "RESULT",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];
    for row in &rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<18}", row.setting),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("{:<14}", row.current),
                Style::default().fg(theme.muted),
            ),
            Span::styled(
                format!("{:<14}", row.requested),
                if row.changed {
                    Style::default()
                        .fg(theme.warning)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.muted)
                },
            ),
            Span::styled(
                if row.changed { "CHANGE" } else { "SAME" }.to_owned(),
                if row.changed {
                    Style::default().fg(theme.warning)
                } else {
                    Style::default().fg(theme.muted)
                },
            ),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        format!(
            "{} will change · {} already matches",
            match changed {
                1 => "1 setting".to_owned(),
                n => format!("{n} settings"),
            },
            rows.len() - changed,
        ),
        Style::default().fg(theme.foreground),
    ));
    lines.push(Line::styled(
        "Nothing has been applied yet.",
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[Esc] ", Style::default().fg(theme.accent)),
        Span::styled("Cancel  ", Style::default().fg(theme.muted)),
        Span::styled("[Enter] ", Style::default().fg(theme.accent)),
        Span::styled("Apply", Style::default().fg(theme.muted)),
    ]));
    let overlay = overlay_area(area, lines.len().max(8));
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(theme.base_style())
        .border_style(Style::default().fg(theme.warning))
        .title(Line::styled(
            " REVIEW CHANGES ".to_owned(),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    let text: Vec<Line<'static>> = lines;
    frame.render_widget(
        Paragraph::new(Text::from(text)).style(theme.base_style()),
        inner,
    );
}

/// Renders the post-attempt result as a full-width band: green for
/// success, red for failure, with the actual result or error detail.
/// Safe for tiny and zero areas.
pub(crate) fn render_notice(frame: &mut Frame, area: Rect, notice: &Notice, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let (edge, glyph) = match notice.kind() {
        NoticeKind::Success => (theme.success, "✓"),
        NoticeKind::Failure => (theme.danger, "✕"),
    };
    let style = Style::default().fg(edge).add_modifier(Modifier::BOLD);
    let band = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: 3.min(area.height.saturating_sub(1).max(1)),
    };
    if band.is_empty() {
        return;
    }
    frame.render_widget(Clear, band);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(theme.base_style())
        .border_style(style)
        .title(" Result ");
    let inner = block.inner(band);
    frame.render_widget(block, band);
    frame.render_widget(
        Paragraph::new(Text::from(vec![Line::from(vec![
            Span::styled(format!("{glyph} "), style),
            Span::styled(notice.message().to_owned(), style),
        ])]))
        .style(theme.base_style()),
        inner,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::FanMode;

    fn command() -> HardwareCommand {
        HardwareCommand::SetFanMode(FanMode::try_from("silent").unwrap())
    }

    #[test]
    fn profile_source_labels_are_stable() {
        assert_eq!(ProfileSource::Builtin.as_str(), "built-in");
        assert_eq!(ProfileSource::Custom.as_str(), "custom");
    }

    #[test]
    fn pending_variants_hold_typed_data() {
        let command_pending = PendingMutation::Command(command());
        assert!(matches!(command_pending, PendingMutation::Command(_)));
    }

    #[test]
    fn notice_preserves_message() {
        let notice = Notice::success("Applied Fan Mode: silent".to_owned());
        assert_eq!(notice.kind(), NoticeKind::Success);
        assert_eq!(notice.message(), "Applied Fan Mode: silent");
        let failure = Notice::failure("Action failed: gone".to_owned());
        assert_eq!(failure.kind(), NoticeKind::Failure);
    }

    #[test]
    fn review_rows_split_setting_current_requested() {
        use crate::hardware::SupportMode;
        let snapshot = crate::tui::screens::support::healthy_snapshot();
        let capabilities = crate::tui::screens::support::full_capabilities();
        let rows = review_rows(
            &PendingMutation::Command(command()),
            Some(&snapshot),
            &SupportMode::Ready,
            &capabilities,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].setting, "Fan Mode");
        assert_eq!(rows[0].current, "auto");
        assert_eq!(rows[0].requested, "silent");
        assert!(rows[0].changed);
    }

    #[test]
    fn review_rows_mark_same_values() {
        use crate::hardware::SupportMode;
        let snapshot = crate::tui::screens::support::healthy_snapshot();
        let capabilities = crate::tui::screens::support::full_capabilities();
        let same = PendingMutation::Command(HardwareCommand::SetFanMode(
            FanMode::try_from("auto").unwrap(),
        ));
        let rows = review_rows(&same, Some(&snapshot), &SupportMode::Ready, &capabilities);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].changed);
    }

    #[test]
    fn review_rows_cover_profile_preview() {
        use crate::hardware::SupportMode;
        use crate::profiles::BuiltinPreset;
        let capabilities = crate::tui::screens::support::full_capabilities();
        let profile = BuiltinPreset::Silent
            .resolve(&capabilities)
            .expect("resolves");
        let pending = PendingMutation::Profile(ProfilePending::new(
            profile,
            "silent".to_owned(),
            ProfileSource::Builtin,
        ));
        let snapshot = crate::tui::screens::support::healthy_snapshot();
        let rows = review_rows(
            &pending,
            Some(&snapshot),
            &SupportMode::Ready,
            &capabilities,
        );
        assert!(!rows.is_empty());
        assert!(
            rows.iter()
                .any(|row| row.setting == "Fan Mode" && row.requested == "silent" && row.changed)
        );
    }

    #[test]
    fn review_rows_never_mention_paths_or_shell() {
        use crate::hardware::SupportMode;
        let snapshot = crate::tui::screens::support::healthy_snapshot();
        let capabilities = crate::tui::screens::support::full_capabilities();
        for pending in [
            PendingMutation::Command(command()),
            PendingMutation::Profile(ProfilePending::new(
                crate::profiles::BuiltinPreset::Balanced
                    .resolve(&capabilities)
                    .expect("resolves"),
                "balanced".to_owned(),
                ProfileSource::Builtin,
            )),
        ] {
            let text = review_rows(
                &pending,
                Some(&snapshot),
                &SupportMode::Ready,
                &capabilities,
            )
            .iter()
            .map(|row| format!("{} {} {}", row.setting, row.current, row.requested))
            .collect::<Vec<_>>()
            .join("\n");
            for forbidden in ["/sys", "/bin/sh", "sh -c", "sudo", "pkexec", "RPM"] {
                assert!(!text.contains(forbidden), "{forbidden:?}");
            }
        }
    }
}
