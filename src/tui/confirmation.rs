//! Typed pending confirmations and result notices.
//!
//! [`PendingMutation`] is the single modal confirmation state: either one
//! typed [`HardwareCommand`] or one retained [`Profile`] with safe display
//! metadata. It never stores paths, shell strings, EC addresses, or raw
//! write data. The stored preview is presentation only; production
//! execution re-evaluates everything fresh.
//!
//! [`Notice`] is the minimal post-attempt banner: success or failure text
//! rendered with semantic styles and monotonic expiry. No execution.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::hardware::{Capabilities, HardwareCommand, HardwareSnapshot, SupportMode};
use crate::profiles::{Profile, ProfilePlanner, ProfileTransactionPlanner};

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
///
/// Notices are transient UI state: each carries its creation instant and
/// an expiry deadline (success ~3s, failure ~6s). The event loop
/// dismisses expired notices on tick/activity; expiry never implies
/// execution and never touches hardware.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    kind: NoticeKind,
    message: String,
    created: std::time::Instant,
}

/// How long a success band stays visible before auto-dismissal.
pub const SUCCESS_TTL: std::time::Duration = std::time::Duration::from_secs(3);
/// How long a failure band stays visible before auto-dismissal. Errors
/// persist longer than success so details can be read.
pub const FAILURE_TTL: std::time::Duration = std::time::Duration::from_secs(6);

/// Success vs failure styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Green success.
    Success,
    /// Red failure.
    Failure,
}

impl Notice {
    /// Success banner, stamped now. Callers must only build this after
    /// the executor reports verified success: the stamp controls
    /// visibility lifetime, never truth.
    pub fn success(message: String) -> Self {
        Self::stamped(NoticeKind::Success, message, std::time::Instant::now())
    }

    /// Failure banner with a Display-rendered safe error, stamped now.
    pub fn failure(message: String) -> Self {
        Self::stamped(NoticeKind::Failure, message, std::time::Instant::now())
    }

    /// Success banner with an explicit creation instant. Test seam for
    /// deterministic expiry without sleeping.
    #[cfg(test)]
    pub(crate) fn success_at(message: String, created: std::time::Instant) -> Self {
        Self::stamped(NoticeKind::Success, message, created)
    }

    /// Failure banner with an explicit creation instant. Test seam for
    /// deterministic expiry without sleeping.
    #[cfg(test)]
    pub(crate) fn failure_at(message: String, created: std::time::Instant) -> Self {
        Self::stamped(NoticeKind::Failure, message, created)
    }

    fn stamped(kind: NoticeKind, message: String, created: std::time::Instant) -> Self {
        Self {
            kind,
            message,
            created,
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

    /// Creation instant driving the visibility deadline.
    pub fn created_at(&self) -> std::time::Instant {
        self.created
    }

    /// Visibility deadline: success ~3s, failure ~6s.
    pub fn expires_at(&self) -> std::time::Instant {
        self.created
            + match self.kind {
                NoticeKind::Success => SUCCESS_TTL,
                NoticeKind::Failure => FAILURE_TTL,
            }
    }

    /// True once the visibility deadline has passed. Timeout only hides
    /// an already-produced notice; it never creates or verifies one.
    pub fn is_expired(&self, now: std::time::Instant) -> bool {
        now >= self.expires_at()
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
    let width = area.width.saturating_sub(4).min(100);
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
    /// Rejection or planning failure. Such a row must never claim SAME.
    pub unavailable: Option<String>,
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
        PendingMutation::Command(command) => vec![command_review_row(command, snapshot)],
        PendingMutation::Profile(request) => {
            profile_review_rows(request.profile(), snapshot, mode, capabilities)
        }
    }
}

fn command_review_row(command: &HardwareCommand, snapshot: Option<&HardwareSnapshot>) -> ReviewRow {
    let full = super::controls::command_text(command);
    let (setting, requested) = full
        .split_once(": ")
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .unwrap_or((full.clone(), full));
    let current = super::controls::current_text(command, snapshot);
    ReviewRow {
        changed: current != requested,
        unavailable: (current == "unknown").then(|| "Current value unavailable".to_owned()),
        setting,
        current,
        requested,
    }
}

/// Profile diffs use the existing pure transaction planner's changed /
/// unchanged sets. Rejected or unplannable requests keep their reason;
/// string formatting does not reimplement transaction rules.
pub(crate) fn profile_review_rows(
    profile: &Profile,
    snapshot: Option<&HardwareSnapshot>,
    mode: &SupportMode,
    capabilities: &Capabilities,
) -> Vec<ReviewRow> {
    let preview = ProfilePlanner::preview(profile, mode, capabilities);
    let plan = snapshot.map(|snapshot| ProfileTransactionPlanner::plan(&preview, snapshot));
    preview
        .entries()
        .iter()
        .map(|entry| {
            let mut row = command_review_row(entry.command(), snapshot);
            match entry.status() {
                crate::profiles::ProfilePreviewStatus::Rejected(error) => {
                    row.changed = false;
                    row.unavailable = Some(format!("Rejected: {error}"));
                }
                crate::profiles::ProfilePreviewStatus::Applicable => match &plan {
                    Some(Ok(plan)) => {
                        row.changed = plan
                            .steps()
                            .iter()
                            .any(|step| step.forward() == entry.command());
                        row.unavailable = None;
                    }
                    Some(Err(error)) => {
                        row.changed = false;
                        row.unavailable = Some(format!("Plan unavailable: {error}"));
                    }
                    None => {
                        row.changed = false;
                        row.unavailable = Some("Current snapshot unavailable".to_owned());
                    }
                },
            }
            row
        })
        .collect()
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
    if area.is_empty() {
        return;
    }
    let rows = review_rows(pending, snapshot, mode, capabilities);
    let changed = rows
        .iter()
        .filter(|row| row.changed && row.unavailable.is_none())
        .count();
    let unavailable = rows.iter().filter(|row| row.unavailable.is_some()).count();
    let inner_width = overlay_area(area, 0).width.saturating_sub(2) as usize;
    let accent = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(theme.muted);
    let warning = Style::default()
        .fg(theme.warning)
        .add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::styled(
        confirmation_title(pending).trim().to_owned(),
        accent,
    )];
    if let PendingMutation::Profile(request) = pending {
        lines.push(Line::from(format!(
            "Profile: {} ({})",
            request.profile().name().as_str(),
            request.source().as_str()
        )));
    }
    if inner_width >= 56 {
        lines.push(Line::styled(
            format!(
                "{:<18}{:<14}{:<14}RESULT",
                "SETTING", "CURRENT", "REQUESTED"
            ),
            accent,
        ));
    }
    for row in &rows {
        let result = if row.unavailable.is_some() {
            "UNAVAILABLE"
        } else if row.changed {
            "CHANGE"
        } else {
            "SAME"
        };
        let style = if row.changed || row.unavailable.is_some() {
            warning
        } else {
            muted
        };
        let table_line = Line::from(vec![
            Span::styled(format!("{:<18}", row.setting), muted),
            Span::styled(format!("{:<14}", row.current), muted),
            Span::styled(format!("{:<14}", row.requested), style),
            Span::styled(result, style),
        ]);
        if inner_width >= 56 && table_line.width() <= inner_width {
            lines.push(table_line);
        } else {
            // Minimum-width columns cannot contain long driver mode names.
            // Keep complete values on their own lines when necessary.
            lines.push(Line::styled(format!("{} — {result}", row.setting), style));
            for (label, value, value_style) in [
                ("Current: ", &row.current, muted),
                ("Requested: ", &row.requested, style),
            ] {
                let line = Line::from(vec![
                    Span::styled(label, muted),
                    Span::styled(value.clone(), value_style),
                ]);
                if line.width() <= inner_width {
                    lines.push(line);
                } else {
                    lines.push(Line::styled(label.trim().to_owned(), muted));
                    lines.push(Line::styled(value.clone(), value_style));
                }
            }
        }
    }
    lines.push(Line::styled(
        format!(
            "{changed} will change · {} already matches · {unavailable} unavailable",
            rows.len() - changed - unavailable
        ),
        muted,
    ));
    let safety_text = if inner_width < 32 {
        "Not applied yet"
    } else {
        "Nothing has been applied yet."
    };
    let mut footer = vec![Line::styled(safety_text, warning)];
    if inner_width < 32 {
        footer.push(Line::styled("Esc Cancel", accent));
        footer.push(Line::styled("Enter Apply", accent));
    } else {
        footer.push(Line::styled("[Esc] Cancel  [Enter] Apply", accent));
    }
    let overlay = overlay_area(area, lines.len() + footer.len() + 1);
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(theme.base_style())
        .border_style(Style::default().fg(theme.warning))
        .title(Line::styled(" REVIEW CHANGES ", accent));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    // Reserve safety actions independently of content height or wrapping.
    let bands = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(footer.len() as u16)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .style(theme.base_style()),
        bands[0],
    );
    frame.render_widget(
        Paragraph::new(Text::from(footer)).style(theme.base_style()),
        bands[1],
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
    fn success_deadline_is_three_seconds() {
        let created = std::time::Instant::now();
        let notice = Notice::success_at("Applied X".to_owned(), created);
        assert_eq!(notice.expires_at(), created + SUCCESS_TTL);
        assert_eq!(SUCCESS_TTL, std::time::Duration::from_secs(3));
        // Still visible before expiry.
        assert!(!notice.is_expired(created + std::time::Duration::from_secs(2)));
    }

    #[test]
    fn success_expires_after_deadline() {
        let created = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(4))
            .expect("recent past constructs");
        let notice = Notice::success_at("Applied X".to_owned(), created);
        assert!(notice.is_expired(std::time::Instant::now()));
    }

    #[test]
    fn error_deadline_is_six_seconds() {
        let created = std::time::Instant::now();
        let notice = Notice::failure_at("Action failed: x".to_owned(), created);
        assert_eq!(notice.expires_at(), created + FAILURE_TTL);
        assert_eq!(FAILURE_TTL, std::time::Duration::from_secs(6));
    }

    #[test]
    fn error_persists_longer_than_success() {
        // Four seconds in: a success band is gone, an error band stays.
        let created = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(4))
            .expect("recent past constructs");
        let now = std::time::Instant::now();
        assert!(Notice::success_at("ok".to_owned(), created).is_expired(now));
        assert!(!Notice::failure_at("bad".to_owned(), created).is_expired(now));
    }

    #[test]
    fn error_expires_after_its_deadline() {
        let created = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(7))
            .expect("recent past constructs");
        let notice = Notice::failure_at("Action failed: x".to_owned(), created);
        assert!(notice.is_expired(std::time::Instant::now()));
    }

    #[test]
    fn constructors_stamp_now() {
        let before = std::time::Instant::now();
        let success = Notice::success("ok".to_owned());
        let failure = Notice::failure("bad".to_owned());
        assert!(success.created_at() >= before);
        assert!(failure.created_at() >= before);
        assert!(!success.is_expired(std::time::Instant::now()));
        assert!(!failure.is_expired(std::time::Instant::now()));
    }

    #[test]
    fn band_renders_while_visible_and_reclaims_after_expiry() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let theme = crate::tui::theme::Theme::default();
        let draw = |notice: Option<&Notice>| {
            let backend = TestBackend::new(100, 30);
            let mut terminal = Terminal::new(backend).expect("test terminal constructs");
            terminal
                .draw(|frame| {
                    if let Some(notice) = notice {
                        render_notice(frame, frame.area(), notice, &theme);
                    }
                })
                .expect("notice draws");
            let mut text = String::new();
            for y in 0..30 {
                let mut line = String::new();
                for x in 0..100 {
                    line.push_str(terminal.backend().buffer()[(x, y)].symbol());
                }
                text.push_str(line.trim_end());
                text.push('\n');
            }
            text
        };
        // During: the approved band shows the actual message.
        let visible = Notice::success("Applied Fan Mode: silent".to_owned());
        let text = draw(Some(&visible));
        assert!(text.contains("Applied Fan Mode: silent"));
        assert!(text.contains("Result"));
        // After simulated expiry the caller renders nothing: no reserved
        // panel, no stale text.
        let text = draw(None);
        assert!(!text.contains("Applied Fan Mode: silent"));
    }

    #[test]
    fn band_zero_area_stays_safe() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        let theme = crate::tui::theme::Theme::default();
        let notice = Notice::failure("Action failed: gone".to_owned());
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_notice(frame, Rect::new(0, 0, 0, 0), &notice, &theme);
            })
            .expect("zero-area notice draws");
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
    #[test]
    fn battery_review_compares_complete_threshold_windows() {
        let mut snapshot = crate::tui::screens::support::healthy_snapshot();
        snapshot.battery_start_threshold = Some(70);
        let command = HardwareCommand::SetBatteryThreshold(
            crate::hardware::BatteryThreshold::from_end_percent(80).unwrap(),
        );
        let rows = review_rows(
            &PendingMutation::Command(command),
            Some(&snapshot),
            &SupportMode::Ready,
            &crate::tui::screens::support::full_capabilities(),
        );
        assert_eq!(rows[0].requested, "70%->80%");
        assert!(!rows[0].changed);
    }

    #[test]
    fn full_profile_review_preserves_safety_at_required_and_compact_sizes() {
        let mut snapshot = crate::tui::screens::support::healthy_snapshot();
        snapshot.battery_start_threshold = Some(70);
        let mut caps = crate::tui::screens::support::full_capabilities();
        caps.super_battery = true;
        let profile = Profile::parse_toml(
            r#"name = "Six settings"
[performance]
shift_mode = "sport"
fan_mode = "silent"
cooler_boost = true
super_battery = true
[battery]
charge_end_threshold = 80
[device]
keyboard_backlight = 3
"#,
        )
        .unwrap();
        let pending = PendingMutation::Profile(ProfilePending::new(
            profile,
            "six".to_owned(),
            ProfileSource::Custom,
        ));
        for (w, h) in [
            (160, 50),
            (120, 35),
            (100, 30),
            (80, 24),
            (50, 16),
            (40, 10),
        ] {
            let text = crate::tui::screens::support::screen_text(w, h, |frame| {
                render_confirmation(
                    frame,
                    frame.area(),
                    &pending,
                    Some(&snapshot),
                    &SupportMode::Ready,
                    &caps,
                    &Theme::default(),
                )
            });
            assert!(text.contains("applied yet"), "{w}x{h}: {text}");
            assert!(text.contains("Cancel"), "{w}x{h}: {text}");
            assert!(text.contains("Apply"), "{w}x{h}: {text}");
            if w >= 80 {
                for setting in [
                    "Shift Mode",
                    "Fan Mode",
                    "Cooler Boost",
                    "Super Battery",
                    "Battery Limit",
                    "Keyboard Backlight",
                ] {
                    assert!(text.contains(setting), "{w}x{h}: {setting}");
                }
                assert!(text.contains("70%->80%"));
                assert!(text.contains("Six settings"));
            }
        }
    }

    #[test]
    fn profile_review_uses_transaction_plan_and_preserves_missing_baselines() {
        let profile = Profile::parse_toml(
            r#"name = "Window"
[battery]
charge_end_threshold = 80
"#,
        )
        .unwrap();
        let caps = crate::tui::screens::support::full_capabilities();
        let mut snapshot = crate::tui::screens::support::healthy_snapshot();
        snapshot.battery_start_threshold = Some(70);
        let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &caps);
        let plan = ProfileTransactionPlanner::plan(&preview, &snapshot).unwrap();
        let rows = profile_review_rows(&profile, Some(&snapshot), &SupportMode::Ready, &caps);
        assert_eq!(
            rows.iter().filter(|row| row.changed).count(),
            plan.steps().len()
        );
        assert_eq!(
            rows.iter().filter(|row| !row.changed).count(),
            plan.unchanged().len()
        );
        assert!(!rows[0].changed);
        snapshot.battery_start_threshold = None;
        let rows = profile_review_rows(&profile, Some(&snapshot), &SupportMode::Ready, &caps);
        assert!(
            rows[0]
                .unavailable
                .as_ref()
                .unwrap()
                .contains("unavailable")
        );
    }

    #[test]
    fn long_mode_review_preserves_full_current_and_requested_values() {
        let current = "c".repeat(64);
        let requested = "r".repeat(64);
        let mut snapshot = crate::tui::screens::support::healthy_snapshot();
        snapshot.fan_mode = Some(FanMode::try_from(current.as_str()).unwrap());
        let pending = PendingMutation::Command(HardwareCommand::SetFanMode(
            FanMode::try_from(requested.as_str()).unwrap(),
        ));
        for (w, h) in [(160, 50), (120, 35), (100, 30), (80, 24)] {
            let text = crate::tui::screens::support::screen_text(w, h, |frame| {
                render_confirmation(
                    frame,
                    frame.area(),
                    &pending,
                    Some(&snapshot),
                    &SupportMode::Ready,
                    &crate::tui::screens::support::full_capabilities(),
                    &Theme::default(),
                )
            });
            assert!(text.contains(&current), "{w}x{h}: {text}");
            assert!(text.contains(&requested), "{w}x{h}: {text}");
            assert!(text.contains("Cancel"));
            assert!(text.contains("Apply"));
        }
        let mut caps = crate::tui::screens::support::full_capabilities();
        caps.fan_modes
            .push(FanMode::try_from(requested.as_str()).unwrap());
        caps.shift_modes
            .push(crate::hardware::ShiftMode::try_from(requested.as_str()).unwrap());
        caps.super_battery = true;
        snapshot.shift_mode = Some(crate::hardware::ShiftMode::try_from(current.as_str()).unwrap());
        snapshot.battery_start_threshold = Some(70);
        let profile = Profile::parse_toml(&format!(
            r#"name = "Six long settings"
[performance]
shift_mode = "{requested}"
fan_mode = "{requested}"
cooler_boost = true
super_battery = true
[battery]
charge_end_threshold = 80
[device]
keyboard_backlight = 3
"#
        ))
        .unwrap();
        let pending = PendingMutation::Profile(ProfilePending::new(
            profile,
            "long".to_owned(),
            ProfileSource::Custom,
        ));
        let text = crate::tui::screens::support::screen_text(80, 24, |frame| {
            render_confirmation(
                frame,
                frame.area(),
                &pending,
                Some(&snapshot),
                &SupportMode::Ready,
                &caps,
                &Theme::default(),
            )
        });
        assert_eq!(text.matches(&current).count(), 2, "{text}");
        assert_eq!(text.matches(&requested).count(), 2, "{text}");
        assert!(text.contains("Keyboard Backlight"));
        assert!(text.contains("Nothing has been applied yet."));
        assert!(text.contains("Apply"));
    }
}
