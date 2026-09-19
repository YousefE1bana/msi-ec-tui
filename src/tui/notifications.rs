//! Bounded in-memory notification history.
//!
//! [`NotificationCenter`] records one [`Notice`] per mutation attempt that
//! reaches the TUI executor (success or failure). Exactly 16 entries are
//! kept; the oldest is evicted first. Stable insertion order, no timers,
//! no background threads, no disk persistence, no filesystem I/O, no
//! telemetry reads. Opening or cancelling a confirmation creates nothing;
//! actions that never reach the executor create nothing.
//!
//! The latest notification doubles as the concise result banner, so there
//! is a single result history, not two.

use std::collections::VecDeque;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::confirmation::{Notice, NoticeKind};
use super::theme::Theme;

/// Exact history bound: oldest evicted first once full.
pub const NOTIFICATION_CAPACITY: usize = 16;

/// Bounded FIFO of result notices, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationCenter {
    entries: VecDeque<Notice>,
}

impl NotificationCenter {
    /// Empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one notice, evicting exactly the oldest entry when full.
    /// The history never exceeds [`NOTIFICATION_CAPACITY`].
    pub fn push(&mut self, notice: Notice) {
        if self.entries.len() == NOTIFICATION_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(notice);
    }

    /// Number of stored notifications.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no notifications are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Newest notification, if any. Also rendered as the result banner.
    pub fn latest(&self) -> Option<&Notice> {
        self.entries.back()
    }

    /// Stored notifications, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &Notice> {
        self.entries.iter()
    }

    /// Drops all stored notifications, keeping the bound.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// One overlay row: `N. Success — message`, newest first.
pub(crate) fn notification_lines(center: &NotificationCenter) -> Vec<String> {
    if center.is_empty() {
        return vec!["No notifications yet".to_owned()];
    }
    let newest_first: Vec<&Notice> = center.iter().collect();
    newest_first
        .iter()
        .rev()
        .enumerate()
        .map(|(index, notice)| {
            let kind = match notice.kind() {
                NoticeKind::Success => "Success",
                NoticeKind::Failure => "Failure",
            };
            format!("{}. {kind} — {}", index + 1, notice.message())
        })
        .collect()
}

/// Centered overlay geometry with saturating math so tiny and zero areas
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

/// Renders the read-only notification history above the underlying screen.
/// Newest first; empty state is honest. Safe for tiny and zero areas.
pub(crate) fn render_notifications(
    frame: &mut Frame,
    area: Rect,
    center: &NotificationCenter,
    theme: &Theme,
) {
    let rows = notification_lines(center);
    let overlay = overlay_area(area, rows.len());
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            " Notifications ".to_owned(),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);
    let text: Vec<Line<'static>> = rows.into_iter().map(Line::from).collect();
    frame.render_widget(Paragraph::new(Text::from(text)), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(message: &str) -> Notice {
        Notice::success(message.to_owned())
    }

    #[test]
    fn empty_by_default() {
        let center = NotificationCenter::new();
        assert!(center.is_empty());
        assert_eq!(center.len(), 0);
        assert_eq!(center.latest(), None);
    }

    #[test]
    fn capacity_is_exactly_sixteen() {
        assert_eq!(NOTIFICATION_CAPACITY, 16);
        let mut center = NotificationCenter::new();
        for index in 0..16 {
            center.push(notice(&format!("n{index}")));
        }
        assert_eq!(center.len(), 16);
    }

    #[test]
    fn seventeenth_push_evicts_exactly_oldest() {
        let mut center = NotificationCenter::new();
        for index in 0..17 {
            center.push(notice(&format!("n{index}")));
        }
        assert_eq!(center.len(), 16);
        let messages: Vec<&str> = center.iter().map(|entry| entry.message()).collect();
        assert!(!messages.contains(&"n0"));
        assert_eq!(messages[0], "n1");
        assert_eq!(messages[15], "n16");
        assert_eq!(center.latest().unwrap().message(), "n16");
    }

    #[test]
    fn insertion_order_is_stable() {
        let mut center = NotificationCenter::new();
        for message in ["a", "b", "c"] {
            center.push(notice(message));
        }
        let messages: Vec<&str> = center.iter().map(|entry| entry.message()).collect();
        assert_eq!(messages, vec!["a", "b", "c"]);
    }

    #[test]
    fn clear_empties_history() {
        let mut center = NotificationCenter::new();
        center.push(notice("a"));
        center.push(Notice::failure("b".to_owned()));
        center.clear();
        assert!(center.is_empty());
        assert_eq!(center.latest(), None);
        center.push(notice("c"));
        assert_eq!(center.latest().unwrap().message(), "c");
    }

    #[test]
    fn overlay_lists_newest_first_with_kinds() {
        let mut center = NotificationCenter::new();
        center.push(notice("first"));
        center.push(Notice::failure("second".to_owned()));
        let lines = notification_lines(&center);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Failure"));
        assert!(lines[0].contains("second"));
        assert!(lines[1].contains("Success"));
        assert!(lines[1].contains("first"));
    }

    #[test]
    fn empty_overlay_is_honest() {
        assert_eq!(
            notification_lines(&NotificationCenter::new()),
            vec!["No notifications yet".to_owned()]
        );
    }
}
