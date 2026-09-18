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
    run_event_loop_with_ticks(state, events, timeout, || {})
}

/// Drives `state` until it requests quit, sampling immediately once before
/// the first event and again on every [`TuiEvent::Tick`].
///
/// Actions apply to state; resizes and ignored events never tick. Source
/// errors propagate without an extra tick and never become quits.
pub fn run_event_loop_with_ticks<E, F>(
    state: &mut AppState,
    events: &mut E,
    timeout: Duration,
    mut on_tick: F,
) -> std::io::Result<()>
where
    E: EventSource,
    F: FnMut(),
{
    on_tick();
    while !state.should_quit() {
        match events.next_event(timeout)? {
            TuiEvent::Action(action) => state.apply(action),
            TuiEvent::Tick => on_tick(),
            TuiEvent::Resize { .. } | TuiEvent::Ignored => {}
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

    use super::{EventSource, TuiEvent, run_event_loop, run_event_loop_with_ticks};

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

    fn run_with_ticks(script: Vec<TuiEvent>) -> (AppState, usize, usize) {
        let mut state = AppState::default();
        let mut source = ScriptedSource::events(script);
        let mut ticks = 0;
        run_event_loop_with_ticks(&mut state, &mut source, TIMEOUT, || ticks += 1)
            .expect("scripted tick loop succeeds");
        (state, source.consumed, ticks)
    }

    #[test]
    fn initial_tick_runs_before_first_event() {
        let (_, consumed, ticks) = run_with_ticks(vec![TuiEvent::Action(AppAction::Quit)]);
        assert_eq!(ticks, 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn immediate_quit_still_receives_exactly_one_initial_tick() {
        let (state, _, ticks) = run_with_ticks(vec![TuiEvent::Action(AppAction::Quit)]);
        assert_eq!(ticks, 1);
        assert!(state.should_quit());
    }

    #[test]
    fn tick_event_invokes_one_additional_callback() {
        let (_, _, ticks) = run_with_ticks(vec![TuiEvent::Tick, TuiEvent::Action(AppAction::Quit)]);
        assert_eq!(ticks, 2);
    }

    #[test]
    fn two_tick_events_invoke_two_additional_callbacks() {
        let (_, _, ticks) = run_with_ticks(vec![
            TuiEvent::Tick,
            TuiEvent::Tick,
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(ticks, 3);
    }

    #[test]
    fn action_does_not_invoke_extra_tick() {
        let (state, _, ticks) = run_with_ticks(vec![
            TuiEvent::Action(AppAction::NextScreen),
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(ticks, 1);
        assert_eq!(state.current_screen(), Screen::Performance);
    }

    #[test]
    fn resize_does_not_invoke_extra_tick() {
        let (_, _, ticks) = run_with_ticks(vec![
            TuiEvent::Resize {
                width: 120,
                height: 40,
            },
            TuiEvent::Action(AppAction::Quit),
        ]);
        assert_eq!(ticks, 1);
    }

    #[test]
    fn ignored_does_not_invoke_extra_tick() {
        let (_, _, ticks) =
            run_with_ticks(vec![TuiEvent::Ignored, TuiEvent::Action(AppAction::Quit)]);
        assert_eq!(ticks, 1);
    }

    #[test]
    fn quit_stops_further_events_and_ticks() {
        let (state, consumed, ticks) =
            run_with_ticks(vec![TuiEvent::Action(AppAction::Quit), TuiEvent::Tick]);
        assert_eq!(consumed, 1);
        assert_eq!(ticks, 1);
        assert_eq!(state.current_screen(), Screen::Dashboard);
    }

    #[test]
    fn tick_loop_error_propagates_after_initial_tick_only() {
        let mut state = AppState::default();
        let mut source = ScriptedSource::failing("terminal gone");
        let mut ticks = 0;
        let error =
            run_event_loop_with_ticks(&mut state, &mut source, TIMEOUT, || ticks += 1).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
        assert_eq!(ticks, 1);
        assert!(!state.should_quit());
    }

    #[test]
    fn telemetry_recovery_flows_through_tick_loop() {
        use std::cell::RefCell;

        use crate::app::LiveHardware;
        use crate::hardware::{
            BackendError, Capabilities, DeviceInfo, EcBackend, HardwareSnapshot, SupportMode,
            TemperatureCelsius,
        };
        use crate::monitoring::SnapshotHistory;

        struct TelemetryBackend {
            script: RefCell<VecDeque<Result<HardwareSnapshot, BackendError>>>,
        }

        impl EcBackend for TelemetryBackend {
            fn detect_device(&self) -> Result<DeviceInfo, BackendError> {
                panic!("tick refresh must not detect device identity");
            }

            fn capabilities(&self) -> Result<Capabilities, BackendError> {
                panic!("tick refresh must not discover capabilities");
            }

            fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
                self.script
                    .borrow_mut()
                    .pop_front()
                    .expect("script exhausted")
            }
        }

        fn reading(celsius: u8) -> HardwareSnapshot {
            HardwareSnapshot {
                cpu_temperature: TemperatureCelsius::try_from(celsius).ok(),
                ..Default::default()
            }
        }

        let mut live = LiveHardware::new(
            DeviceInfo {
                manufacturer: "MSI".to_owned(),
                product_name: "Tick Recovery Fixture".to_owned(),
                board_name: None,
                bios_version: None,
                ec_firmware_version: None,
            },
            SupportMode::Ready,
            TelemetryBackend {
                script: RefCell::new(
                    vec![
                        Ok(reading(60)),
                        Err(BackendError::Unavailable),
                        Ok(reading(61)),
                    ]
                    .into(),
                ),
            },
            SnapshotHistory::default(),
        );
        let mut state = AppState::default();
        let mut source = ScriptedSource::events(vec![
            TuiEvent::Tick,
            TuiEvent::Tick,
            TuiEvent::Action(AppAction::Quit),
        ]);
        run_event_loop_with_ticks(&mut state, &mut source, TIMEOUT, || live.refresh())
            .expect("telemetry tick loop succeeds");

        assert!(state.should_quit());
        assert!(!live.is_degraded());
        assert_eq!(live.history().len(), 2);
        let temperatures: Vec<u8> = live
            .history()
            .iter()
            .map(|snapshot| snapshot.cpu_temperature.unwrap().get())
            .collect();
        assert_eq!(temperatures, vec![60, 61]);
        assert_eq!(
            live.current_snapshot().unwrap().cpu_temperature,
            reading(61).cpu_temperature
        );
    }
}
