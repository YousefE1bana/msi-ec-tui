//! Terminal event model and Crossterm event source.
//!
//! [`TuiEvent`] represents runtime occurrences for the event loop.
//! [`CrosstermEventSource`] polls the real terminal; conversion stays pure
//! so timeout and resize semantics are unit-testable without a terminal.

use std::time::Duration;

use crossterm::event::{Event, poll, read};

use crate::app::AppAction;

use super::input::action_for_key_with_options;

/// Runtime occurrence delivered to the event loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiEvent {
    /// A key mapped to an application intent.
    Action(AppAction),
    /// The poll timeout elapsed with no terminal input.
    Tick,
    /// The terminal was resized.
    Resize { width: u16, height: u16 },
    /// An event the TUI does not act on.
    Ignored,
}

/// Converts a polled terminal outcome into a [`TuiEvent`].
///
/// `None` means the poll timed out and becomes [`TuiEvent::Tick`].
/// Keeping this pure lets timeout and resize behavior stay covered by
/// unit tests without touching a real terminal. Vim navigation stays
/// enabled; use [`event_to_tui_event_with_options`] for configured input.
pub fn event_to_tui_event(polled: Option<Event>) -> TuiEvent {
    event_to_tui_event_with_options(polled, true)
}

/// Pure polled-outcome conversion honoring the configured vim-keys setting.
pub fn event_to_tui_event_with_options(polled: Option<Event>, vim_keys: bool) -> TuiEvent {
    match polled {
        None => TuiEvent::Tick,
        Some(Event::Key(key)) => match action_for_key_with_options(key, vim_keys) {
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

/// Production [`EventSource`] backed by Crossterm. Owns the immutable
/// vim-keys input option so `h`/`j`/`k`/`l` honor `vim_keys = false`;
/// arrow keys always work. Defaults to vim keys on.
#[derive(Debug, Clone, Copy)]
pub struct CrosstermEventSource {
    vim_keys: bool,
}

impl Default for CrosstermEventSource {
    fn default() -> Self {
        Self { vim_keys: true }
    }
}

impl CrosstermEventSource {
    /// Builds a source with the configured vim-keys setting.
    pub fn with_vim_keys(vim_keys: bool) -> Self {
        Self { vim_keys }
    }

    /// The configured vim-keys setting.
    pub fn vim_keys(&self) -> bool {
        self.vim_keys
    }
}

impl EventSource for CrosstermEventSource {
    fn next_event(&mut self, timeout: Duration) -> std::io::Result<TuiEvent> {
        let polled = if poll(timeout)? { Some(read()?) } else { None };
        Ok(event_to_tui_event_with_options(polled, self.vim_keys))
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::Event;

    use crate::app::{AppAction, Screen};

    use super::{TuiEvent, event_to_tui_event, event_to_tui_event_with_options};

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

    #[test]
    fn vim_keys_off_turns_hjkl_into_ignored() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

        for key in ['h', 'j', 'k', 'l'] {
            let press = KeyEvent {
                code: KeyCode::Char(key),
                modifiers: KeyModifiers::empty(),
                kind: KeyEventKind::Press,
                state: KeyEventState::empty(),
            };
            assert_eq!(
                event_to_tui_event_with_options(Some(Event::Key(press)), false),
                TuiEvent::Ignored,
                "{key} must be ignored when vim keys are off",
            );
        }
    }

    #[test]
    fn vim_keys_off_keeps_arrows() {
        use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

        let press = KeyEvent {
            code: KeyCode::Up,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        };
        assert_eq!(
            event_to_tui_event_with_options(Some(Event::Key(press)), false),
            TuiEvent::Action(AppAction::MoveUp)
        );
    }

    #[test]
    fn event_source_defaults_to_vim_keys_on() {
        use super::CrosstermEventSource;
        assert!(CrosstermEventSource::default().vim_keys());
        assert!(CrosstermEventSource::with_vim_keys(true).vim_keys());
        assert!(!CrosstermEventSource::with_vim_keys(false).vim_keys());
    }
}
