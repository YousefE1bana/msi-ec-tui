//! Deterministic event loop over [`AppState`].
//!
//! The loop applies actions, ignores ticks/resizes in Task 2, and
//! propagates event-source errors without converting them into quits.

use std::time::Duration;

use crate::app::AppState;

use super::event::{EventSource, TuiEvent};

/// Drives `state` until it requests quit.
///
/// Ticks, resizes, and ignored events leave state unchanged. Hardware
/// polling arrives in a later task. Source errors propagate to the caller.
pub fn run_event_loop<E: EventSource>(
    state: &mut AppState,
    events: &mut E,
    timeout: Duration,
) -> std::io::Result<()> {
    while !state.should_quit() {
        match events.next_event(timeout)? {
            TuiEvent::Action(action) => state.apply(action),
            TuiEvent::Tick | TuiEvent::Resize { .. } | TuiEvent::Ignored => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::ErrorKind;
    use std::time::Duration;

    use crate::app::{AppAction, AppState, Screen};

    use super::{EventSource, TuiEvent, run_event_loop};

    const TIMEOUT: Duration = Duration::from_millis(10);

    struct ScriptedSource {
        script: VecDeque<Result<TuiEvent, String>>,
        consumed: usize,
    }

    impl ScriptedSource {
        fn events(events: Vec<TuiEvent>) -> Self {
            Self {
                script: events.into_iter().map(Ok).collect(),
                consumed: 0,
            }
        }

        fn failing(message: &str) -> Self {
            Self {
                script: VecDeque::from([Err(message.to_owned())]),
                consumed: 0,
            }
        }
    }

    impl EventSource for ScriptedSource {
        fn next_event(&mut self, _timeout: Duration) -> std::io::Result<TuiEvent> {
            self.consumed += 1;
            match self.script.pop_front().expect("script exhausted") {
                Ok(event) => Ok(event),
                Err(message) => Err(std::io::Error::other(message)),
            }
        }
    }

    fn run(script: Vec<TuiEvent>) -> (AppState, usize) {
        let mut state = AppState::default();
        let mut source = ScriptedSource::events(script);
        run_event_loop(&mut state, &mut source, TIMEOUT).expect("scripted loop succeeds");
        (state, source.consumed)
    }

    #[test]
    fn next_screen_then_quit_lands_on_performance() {
        let (state, _) = run(vec![
            TuiEvent::Action(AppAction::NextScreen),
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(state.current_screen(), Screen::Performance);
        assert!(state.should_quit());
    }

    #[test]
    fn previous_screen_then_quit_wraps_to_diagnostics() {
        let (state, _) = run(vec![
            TuiEvent::Action(AppAction::PreviousScreen),
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(state.current_screen(), Screen::Diagnostics);
    }

    #[test]
    fn goto_reaches_exact_screen() {
        let (state, _) = run(vec![
            TuiEvent::Action(AppAction::GoTo(Screen::Battery)),
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(state.current_screen(), Screen::Battery);
    }

    #[test]
    fn toggle_help_updates_state() {
        let (state, _) = run(vec![
            TuiEvent::Action(AppAction::ToggleHelp),
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert!(state.help_visible());
    }

    #[test]
    fn tick_does_not_mutate_state() {
        let (state, _) = run(vec![TuiEvent::Tick, TuiEvent::Action(AppAction::Quit)]);
        assert_eq!(state.current_screen(), Screen::Dashboard);
        assert!(!state.help_visible());
    }

    #[test]
    fn resize_does_not_mutate_state() {
        let (state, _) = run(vec![
            TuiEvent::Resize {
                width: 120,
                height: 40,
            },
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(state.current_screen(), Screen::Dashboard);
        assert!(!state.help_visible());
    }

    #[test]
    fn ignored_does_not_mutate_state() {
        let (state, _) = run(vec![TuiEvent::Ignored, TuiEvent::Action(AppAction::Quit)]);
        assert_eq!(state.current_screen(), Screen::Dashboard);
        assert!(!state.help_visible());
    }

    #[test]
    fn event_source_error_propagates_without_quit() {
        let mut state = AppState::default();
        let mut source = ScriptedSource::failing("terminal gone");
        let error = run_event_loop(&mut state, &mut source, TIMEOUT).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
        assert!(!state.should_quit());
    }

    #[test]
    fn quit_stops_requesting_further_events() {
        let (state, consumed) = run(vec![
            TuiEvent::Action(AppAction::Quit),
            TuiEvent::Action(AppAction::NextScreen),
        ]);
        assert_eq!(consumed, 1);
        assert_eq!(state.current_screen(), Screen::Dashboard);
    }

    #[test]
    fn already_quit_state_requests_no_events() {
        let mut state = AppState::default();
        state.apply(AppAction::Quit);
        let mut source = ScriptedSource::events(vec![TuiEvent::Action(AppAction::NextScreen)]);
        run_event_loop(&mut state, &mut source, TIMEOUT).expect("pre-quit loop succeeds");
        assert_eq!(source.consumed, 0);
        assert_eq!(state.current_screen(), Screen::Dashboard);
    }
}
