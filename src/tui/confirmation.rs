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
use ratatui::text::{Line, Text};
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

/// Pure dialog content: exact typed request plus current state for commands,
/// retained name/slug/source plus pure preview for profiles. Preview is
/// presentation only, never authorization. No paths, no RPM.
pub(crate) fn confirmation_lines(
    pending: &PendingMutation,
    snapshot: Option<&HardwareSnapshot>,
    mode: &SupportMode,
    capabilities: &Capabilities,
) -> Vec<String> {
    match pending {
        PendingMutation::Command(command) => {
            vec![
                format!("Requested: {}", super::controls::command_text(command)),
                format!(
                    "Current: {}",
                    super::controls::current_text(command, snapshot)
                ),
                "Enter Confirm once / Esc Cancel".to_owned(),
                "One confirm executes at most once.".to_owned(),
            ]
        }
        PendingMutation::Profile(request) => {
            let mut lines = vec![
                format!(
                    "Profile: {} ({})",
                    request.profile().name(),
                    request.source().as_str()
                ),
                format!("Slug: {}", request.slug()),
                format!("Source: {}", request.source().as_str()),
            ];
            let preview = ProfilePlanner::preview(request.profile(), mode, capabilities);
            lines.push("Preview:".to_owned());
            for entry in preview.entries() {
                let status = match entry.status() {
                    crate::profiles::ProfilePreviewStatus::Applicable => "Applicable".to_owned(),
                    crate::profiles::ProfilePreviewStatus::Rejected(error) => {
                        format!("Rejected: {error}")
                    }
                };
                lines.push(format!(
                    "{} — {status}",
                    super::controls::command_text(entry.command())
                ));
            }
            lines.push("Enter Confirm once / Esc Cancel".to_owned());
            lines
        }
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

/// Renders the modal confirmation above the underlying screen. Safe for
/// tiny and zero areas.
pub(crate) fn render_confirmation(
    frame: &mut Frame,
    area: Rect,
    pending: &PendingMutation,
    snapshot: Option<&HardwareSnapshot>,
    mode: &SupportMode,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let lines = confirmation_lines(pending, snapshot, mode, capabilities);
    let overlay = overlay_area(area, lines.len());
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning))
        .title(Line::styled(
            confirmation_title(pending).to_owned(),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    let text: Vec<Line<'static>> = lines.into_iter().map(Line::from).collect();
    frame.render_widget(Paragraph::new(Text::from(text)), inner);
}

/// Renders the post-attempt banner. Success uses the success role, failure
/// uses danger. Safe for tiny and zero areas.
pub(crate) fn render_notice(frame: &mut Frame, area: Rect, notice: &Notice, theme: &Theme) {
    let style = match notice.kind() {
        NoticeKind::Success => Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD),
        NoticeKind::Failure => Style::default()
            .fg(theme.danger)
            .add_modifier(Modifier::BOLD),
    };
    let overlay = overlay_area(area, 1);
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(" Result ");
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    frame.render_widget(
        Paragraph::new(Text::from(vec![Line::styled(
            notice.message().to_owned(),
            style,
        )])),
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
    fn command_dialog_shows_exact_request_and_current() {
        use crate::hardware::SupportMode;
        let snapshot = crate::tui::screens::support::healthy_snapshot();
        let capabilities = crate::tui::screens::support::full_capabilities();
        let lines = confirmation_lines(
            &PendingMutation::Command(command()),
            Some(&snapshot),
            &SupportMode::Ready,
            &capabilities,
        );
        let text = lines.join("\n");
        assert!(text.contains("Requested: Fan Mode: silent"));
        assert!(text.contains("Current: auto"));
        assert!(text.contains("Confirm once"));
        assert!(!text.contains("RPM"));
        assert!(!text.contains("/sys"));
    }

    #[test]
    fn profile_dialog_shows_retained_metadata_and_preview() {
        use crate::hardware::SupportMode;
        use crate::profiles::BuiltinPreset;
        let capabilities = crate::tui::screens::support::full_capabilities();
        let profile = BuiltinPreset::Balanced
            .resolve(&capabilities)
            .expect("resolves");
        let pending = PendingMutation::Profile(ProfilePending::new(
            profile,
            "balanced".to_owned(),
            ProfileSource::Builtin,
        ));
        let snapshot = crate::tui::screens::support::healthy_snapshot();
        let lines = confirmation_lines(
            &pending,
            Some(&snapshot),
            &SupportMode::Ready,
            &capabilities,
        );
        let text = lines.join("\n");
        assert!(text.contains("balanced"));
        assert!(text.contains("built-in"));
        assert!(text.contains("Preview:"));
        assert!(!text.contains("/sys"));
    }

    #[test]
    fn dialog_never_mentions_paths_or_shell() {
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
            let text = confirmation_lines(
                &pending,
                Some(&snapshot),
                &SupportMode::Ready,
                &capabilities,
            )
            .join("\n");
            for forbidden in ["/sys", "/bin/sh", "sh -c", "sudo", "pkexec"] {
                assert!(!text.contains(forbidden), "{forbidden:?}");
            }
        }
    }
}
