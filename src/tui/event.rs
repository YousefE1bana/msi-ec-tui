//! Terminal event model and Crossterm event source.
//!
//! [`TuiEvent`] represents runtime occurrences for the event loop.
//! [`CrosstermEventSource`] polls the real terminal; conversion stays pure
//! so timeout and resize semantics are unit-testable without a terminal.

use std::time::Duration;

use crossterm::event::{Event, poll, read};

use crate::app::AppAction;

use super::input::action_for_key;

/// Runtime occurrence delivered to the event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiEvent {
    /// A key mapped to an application intent.
    Action(AppAction),
    /// The poll timeout elapsed with no terminal input.
    Tick,
    /// The terminal was resized.
    Resize { width: u16, height: u16 },
    /// An event the read-only TUI does not act on.
    Ignored,
}

/// Converts a polled terminal outcome into a [`TuiEvent`].
///
/// `None` means the poll timed out and becomes [`TuiEvent::Tick`].
/// Keeping this pure lets timeout and resize behavior stay covered by
/// unit tests without touching a real terminal.
pub fn event_to_tui_event(polled: Option<Event>) -> TuiEvent {
    match polled {
        None => TuiEvent::Tick,
        Some(Event::Key(key)) => match action_for_key(key) {
            Some(action) => TuiEvent::Action(action),
            None => TuiEvent::Ignored,
        },
        Some(Event::Resize(width, height)) => TuiEvent::Resize { width, height },
        Some(_) => TuiEvent::Ignored,
    }
}

/// Source of [`TuiEvent`]s for [`super::run_event_loop`].
pub trait EventSource {
    /// Waits up to `timeout` for the next runtime event.
    fn next_event(&mut self, timeout: Duration) -> std::io::Result<TuiEvent>;
}

/// Production [`EventSource`] backed by Crossterm.
#[derive(Debug, Default)]
pub struct CrosstermEventSource;

impl EventSource for CrosstermEventSource {
    fn next_event(&mut self, timeout: Duration) -> std::io::Result<TuiEvent> {
        let polled = if poll(timeout)? { Some(read()?) } else { None };
        Ok(event_to_tui_event(polled))
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::Event;

    use crate::app::{AppAction, Screen};

    use super::{TuiEvent, event_to_tui_event};

    #[test]
    fn poll_timeout_becomes_tick() {
        assert_eq!(event_to_tui_event(None), TuiEvent::Tick);
    }

    #[test]
    fn resize_dimensions_are_preserved() {
        assert_eq!(
            event_to_tui_event(Some(Event::Resize(80, 24))),
            TuiEvent::Resize {
                width: 80,
                height: 24,
            }
        );
    }

    #[test]
    fn quit_key_becomes_action() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

        let key = KeyEvent {
            code: KeyCode::Char('q'),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };
        assert_eq!(
            event_to_tui_event(Some(Event::Key(key))),
            TuiEvent::Action(AppAction::Quit)
        );
    }

    #[test]
    fn unmapped_key_becomes_ignored() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

        let key = KeyEvent {
            code: KeyCode::F(12),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };
        assert_eq!(event_to_tui_event(Some(Event::Key(key))), TuiEvent::Ignored);
    }

    #[test]
    fn digit_key_becomes_goto_action() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

        let key = KeyEvent {
            code: KeyCode::Char('3'),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };
        assert_eq!(
            event_to_tui_event(Some(Event::Key(key))),
            TuiEvent::Action(AppAction::GoTo(Screen::Fans))
        );
    }

    #[test]
    fn mouse_and_focus_events_become_ignored() {
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

        let click = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::empty(),
        });
        assert_eq!(event_to_tui_event(Some(click)), TuiEvent::Ignored);
        assert_eq!(
            event_to_tui_event(Some(Event::FocusGained)),
            TuiEvent::Ignored
        );
        assert_eq!(
            event_to_tui_event(Some(Event::FocusLost)),
            TuiEvent::Ignored
        );
    }
}
