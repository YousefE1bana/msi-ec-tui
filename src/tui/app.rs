//! Interactive read-only TUI orchestration: startup, loop, and shutdown.
//!
//! [`TuiApp`] owns navigation state, live telemetry, and startup
//! capabilities. [`prepare_tui`] composes the hardware layer once before
//! any terminal takeover; [`run_tui_loop`] refreshes and redraws without
//! ever owning the terminal. Only [`run_tui`] touches a real terminal.

use std::time::Duration;

use thiserror::Error;

use crate::app::{AppState, LiveHardware};
use crate::hardware::{
    Capabilities, CapabilityDetector, CapabilityDiscoveryError, EcBackend, LinuxSysfsReader,
    MsiEcBackend, SupportEvaluator, SupportMode, SysfsReader, SystemPaths,
};
use crate::monitoring::{PollInterval, SnapshotHistory};

use super::{CrosstermEventSource, EventSource, TerminalSession, TuiEvent, render_screen};

/// Failures that prevent the read-only TUI from running.
#[derive(Debug, Error)]
pub enum TuiError {
    /// Device identity could not be established; nothing is invented.
    #[error("TUI device unavailable: {0}")]
    Device(#[from] crate::hardware::BackendError),
    /// Capability discovery failed while the support verdict claims READY,
    /// an internally contradictory state that must not launch degraded.
    #[error("TUI capability discovery failed: {0}")]
    Capabilities(#[from] CapabilityDiscoveryError),
    /// The terminal could not be entered.
    #[error("TUI terminal unavailable: {0}")]
    Terminal(std::io::Error),
    /// The event/draw loop failed after a successful terminal entry.
    #[error("TUI runtime failed: {0}")]
    Runtime(std::io::Error),
    /// The terminal could not be restored after a successful run.
    #[error("TUI terminal restore failed: {0}")]
    Restore(std::io::Error),
    /// Both the runtime and the terminal restoration failed; the runtime
    /// error stays primary while restore context is retained.
    #[error("TUI runtime failed: {runtime}; terminal restore also failed: {restore}")]
    RuntimeAndRestore {
        runtime: std::io::Error,
        restore: std::io::Error,
    },
}

/// Interactive application state: navigation, live telemetry, and the
/// startup capability set. Owns no terminal or filesystem handles beyond
/// the backend itself.
pub struct TuiApp<B> {
    state: AppState,
    live: LiveHardware<B>,
    capabilities: Capabilities,
}

impl<B> TuiApp<B>
where
    B: EcBackend,
{
    /// Navigation state driving screen dispatch and the help overlay.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// Mutable navigation state for the event loop.
    pub fn state_mut(&mut self) -> &mut AppState {
        &mut self.state
    }

    /// Live read-only telemetry.
    pub fn live(&self) -> &LiveHardware<B> {
        &self.live
    }

    /// Mutable live telemetry for tick refreshes.
    pub fn live_mut(&mut self) -> &mut LiveHardware<B> {
        &mut self.live
    }

    /// Capability set discovered once at startup.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Samples once; failures degrade instead of terminating.
    pub fn refresh(&mut self) {
        self.live.refresh();
    }
}

/// Composes the hardware layer once: support verdict, device identity,
/// startup capabilities, then side-effect-free live state. No snapshot is
/// taken here; the runtime loop owns the first refresh.
///
/// A failed capability discovery under READ-ONLY falls back to a
/// conservative empty set so monitoring can still launch degraded. The
/// same failure under READY is contradictory and aborts startup.
pub fn prepare_tui<R>(paths: SystemPaths, reader: R) -> Result<TuiApp<MsiEcBackend<R>>, TuiError>
where
    R: SysfsReader + Clone,
{
    let mode = SupportEvaluator::new(paths.clone(), reader.clone()).evaluate();
    let backend = MsiEcBackend::new(paths.clone(), reader.clone());
    let device = backend.detect_device()?;
    let capabilities = match CapabilityDetector::new(paths, reader).discover() {
        Ok(capabilities) => capabilities,
        Err(error) => {
            if matches!(mode, SupportMode::ReadOnly(_)) {
                Capabilities::default()
            } else {
                return Err(TuiError::Capabilities(error));
            }
        }
    };
    Ok(TuiApp {
        state: AppState::default(),
        live: LiveHardware::new(device, mode, backend, SnapshotHistory::default()),
        capabilities,
    })
}

/// Launches the TUI only when both standard streams are terminals, so
/// pipes, CI captures, and redirected output keep the historic banner.
pub fn should_launch_tui(stdin_terminal: bool, stdout_terminal: bool) -> bool {
    stdin_terminal && stdout_terminal
}

/// Drives `app` until quit: one refresh plus one draw up front, then per
/// event exactly one refresh+draw on ticks, one draw on actions and
/// resizes, and nothing on ignored events. Refresh failures degrade;
/// event and draw errors propagate without extra work.
pub fn run_tui_loop<B, E, D>(
    app: &mut TuiApp<B>,
    events: &mut E,
    timeout: Duration,
    mut draw: D,
) -> std::io::Result<()>
where
    B: EcBackend,
    E: EventSource,
    D: FnMut(&mut TuiApp<B>) -> std::io::Result<()>,
{
    app.refresh();
    draw(app)?;
    while !app.state().should_quit() {
        step_tui_loop(app, events, timeout, &mut draw)?;
    }
    Ok(())
}

/// Applies one runtime event: actions redraw unless quitting, ticks refresh
/// and redraw, resizes redraw, ignored events rest. Errors propagate with
/// no extra refresh or redraw.
fn step_tui_loop<B, E, D>(
    app: &mut TuiApp<B>,
    events: &mut E,
    timeout: Duration,
    draw: &mut D,
) -> std::io::Result<()>
where
    B: EcBackend,
    E: EventSource,
    D: FnMut(&mut TuiApp<B>) -> std::io::Result<()>,
{
    match events.next_event(timeout)? {
        TuiEvent::Action(action) => {
            app.state_mut().apply(action);
            if !app.state().should_quit() {
                draw(app)?;
            }
        }
        TuiEvent::Tick => {
            app.refresh();
            draw(app)?;
        }
        TuiEvent::Resize { .. } => {
            draw(app)?;
        }
        TuiEvent::Ignored => {}
    }
    Ok(())
}

/// Picks the reported outcome: a runtime failure stays primary even when
/// restoration also fails; otherwise either failure surfaces alone.
fn resolve_outcome(
    runtime: Result<(), std::io::Error>,
    restore: Result<(), std::io::Error>,
) -> Result<(), TuiError> {
    match (runtime, restore) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(runtime), Ok(())) => Err(TuiError::Runtime(runtime)),
        (Ok(()), Err(restore)) => Err(TuiError::Restore(restore)),
        (Err(runtime), Err(restore)) => Err(TuiError::RuntimeAndRestore { runtime, restore }),
    }
}

/// Production entry point: prepares hardware before terminal takeover,
/// runs the 1-second loop, and always attempts terminal restoration.
/// Returns typed errors; only `main` decides process exit codes.
pub fn run_tui(paths: SystemPaths) -> Result<(), TuiError> {
    let mut app = prepare_tui(paths, LinuxSysfsReader)?;
    let mut session = TerminalSession::enter().map_err(TuiError::Terminal)?;
    let mut events = CrosstermEventSource;
    let timeout = PollInterval::default().as_duration();
    let runtime = run_tui_loop(&mut app, &mut events, timeout, |app| {
        session
            .terminal_mut()
            .draw(|frame| {
                render_screen(
                    frame,
                    frame.area(),
                    app.state(),
                    app.live(),
                    app.capabilities(),
                );
            })
            .map(|_| ())
    });
    let restore = session.restore();
    resolve_outcome(runtime, restore)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::Duration;

    use crate::app::{AppAction, AppState, LiveHardware, Screen};
    use crate::hardware::{
        BackendError, Capabilities, DeviceInfo, EcBackend, HardwareSnapshot, ReadOnlyReason,
        SupportMode, SystemPaths, TemperatureCelsius,
    };
    use crate::monitoring::{PollInterval, SnapshotHistory};

    use super::{TuiApp, prepare_tui, resolve_outcome, run_tui_loop, should_launch_tui};
    use crate::tui::{EventSource, TuiEvent};

    const TIMEOUT: Duration = Duration::from_millis(10);

    fn fixture_root(name: &str) -> SystemPaths {
        SystemPaths::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(name),
        )
    }

    #[test]
    fn gf63_preparation_succeeds() {
        let app = prepare_tui(fixture_root("gf63"), crate::hardware::LinuxSysfsReader);
        assert!(app.is_ok());
    }

    #[test]
    fn gf63_startup_mode_is_ready() {
        let app = prepare_tui(fixture_root("gf63"), crate::hardware::LinuxSysfsReader).unwrap();
        assert_eq!(app.live().mode(), &SupportMode::Ready);
    }

    #[test]
    fn gf63_startup_device_is_gf63() {
        let app = prepare_tui(fixture_root("gf63"), crate::hardware::LinuxSysfsReader).unwrap();
        assert_eq!(app.live().device().product_name, "GF63 Thin 11UC");
    }

    #[test]
    fn gf63_startup_capabilities_cover_hardware() {
        let app = prepare_tui(fixture_root("gf63"), crate::hardware::LinuxSysfsReader).unwrap();
        assert!(app.capabilities().cpu_temperature);
        assert!(!app.capabilities().fan_modes.is_empty());
        assert!(!app.capabilities().shift_modes.is_empty());
    }

    #[test]
    fn construction_samples_nothing() {
        let app = prepare_tui(fixture_root("gf63"), crate::hardware::LinuxSysfsReader).unwrap();
        assert_eq!(app.live().current_snapshot(), None);
        assert!(!app.live().is_degraded());
    }

    #[test]
    fn partial_device_preparation_succeeds() {
        assert!(
            prepare_tui(
                fixture_root("partial-device"),
                crate::hardware::LinuxSysfsReader
            )
            .is_ok()
        );
    }

    #[test]
    fn unknown_device_preparation_succeeds() {
        assert!(
            prepare_tui(
                fixture_root("unknown-device"),
                crate::hardware::LinuxSysfsReader
            )
            .is_ok()
        );
    }

    #[test]
    fn broken_sysfs_preparation_succeeds() {
        assert!(
            prepare_tui(
                fixture_root("broken-sysfs"),
                crate::hardware::LinuxSysfsReader
            )
            .is_ok()
        );
    }

    #[test]
    fn broken_sysfs_mode_is_read_only_inconsistent() {
        let app = prepare_tui(
            fixture_root("broken-sysfs"),
            crate::hardware::LinuxSysfsReader,
        )
        .unwrap();
        assert_eq!(
            app.live().mode(),
            &SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface)
        );
    }

    #[test]
    fn broken_sysfs_capabilities_stay_conservative() {
        let app = prepare_tui(
            fixture_root("broken-sysfs"),
            crate::hardware::LinuxSysfsReader,
        )
        .unwrap();
        assert_eq!(
            app.capabilities(),
            &crate::hardware::Capabilities::default()
        );
    }

    #[test]
    fn launch_decision_requires_both_terminals() {
        assert!(should_launch_tui(true, true));
        assert!(!should_launch_tui(true, false));
        assert!(!should_launch_tui(false, true));
        assert!(!should_launch_tui(false, false));
    }

    #[test]
    fn package_version_is_v05() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.5.0");
    }

    struct LoopBackend {
        script: RefCell<VecDeque<Result<HardwareSnapshot, BackendError>>>,
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    impl EcBackend for LoopBackend {
        fn detect_device(&self) -> Result<DeviceInfo, BackendError> {
            panic!("loop refresh must not detect device identity");
        }

        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            panic!("loop refresh must not discover capabilities");
        }

        fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
            self.log.borrow_mut().push("refresh");
            self.script
                .borrow_mut()
                .pop_front()
                .expect("script exhausted")
        }
    }

    struct LoopSource {
        script: VecDeque<Result<TuiEvent, String>>,
        events: Rc<Cell<usize>>,
        timeouts: Rc<RefCell<Vec<Duration>>>,
        log: Rc<RefCell<Vec<&'static str>>>,
    }

    impl LoopSource {
        fn events(events: Vec<TuiEvent>, log: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                script: events.into_iter().map(Ok).collect(),
                events: Rc::new(Cell::new(0)),
                timeouts: Rc::new(RefCell::new(Vec::new())),
                log,
            }
        }

        fn failing(log: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                script: VecDeque::from([Err("event source gone".to_owned())]),
                events: Rc::new(Cell::new(0)),
                timeouts: Rc::new(RefCell::new(Vec::new())),
                log,
            }
        }
    }

    impl EventSource for LoopSource {
        fn next_event(&mut self, timeout: Duration) -> std::io::Result<TuiEvent> {
            self.events.set(self.events.get() + 1);
            self.timeouts.borrow_mut().push(timeout);
            self.log.borrow_mut().push("event");
            match self.script.pop_front().expect("script exhausted") {
                Ok(event) => Ok(event),
                Err(message) => Err(std::io::Error::other(message)),
            }
        }
    }

    struct LoopHarness {
        draws: Rc<Cell<usize>>,
        degraded_at_draw: Rc<RefCell<Vec<bool>>>,
        current_at_draw: Rc<RefCell<Vec<Option<u8>>>>,
    }

    fn temperature(celsius: u8) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(celsius).ok(),
            ..Default::default()
        }
    }

    fn loop_app(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
        log: Rc<RefCell<Vec<&'static str>>>,
    ) -> TuiApp<LoopBackend> {
        TuiApp {
            state: AppState::default(),
            live: LiveHardware::new(
                DeviceInfo {
                    manufacturer: "MSI".to_owned(),
                    product_name: "Loop Fixture".to_owned(),
                    board_name: None,
                    bios_version: None,
                    ec_firmware_version: None,
                },
                SupportMode::Ready,
                LoopBackend {
                    script: RefCell::new(script.into()),
                    log,
                },
                SnapshotHistory::default(),
            ),
            capabilities: Capabilities::default(),
        }
    }

    fn run_loop(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
        events: Vec<TuiEvent>,
    ) -> (
        TuiApp<LoopBackend>,
        Rc<RefCell<Vec<&'static str>>>,
        LoopHarness,
        usize,
    ) {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(script, Rc::clone(&log));
        let mut source = LoopSource::events(events, Rc::clone(&log));
        let draws = Rc::new(Cell::new(0));
        let degraded_at_draw = Rc::new(RefCell::new(Vec::new()));
        let current_at_draw = Rc::new(RefCell::new(Vec::new()));
        let harness = LoopHarness {
            draws: Rc::clone(&draws),
            degraded_at_draw: Rc::clone(&degraded_at_draw),
            current_at_draw: Rc::clone(&current_at_draw),
        };
        run_tui_loop(&mut app, &mut source, TIMEOUT, |app| {
            draws.set(draws.get() + 1);
            log.borrow_mut().push("draw");
            degraded_at_draw.borrow_mut().push(app.live().is_degraded());
            current_at_draw.borrow_mut().push(
                app.live()
                    .current_snapshot()
                    .and_then(|snapshot| snapshot.cpu_temperature)
                    .map(|reading| reading.get()),
            );
            Ok(())
        })
        .expect("scripted loop succeeds");
        let consumed = source.events.get();
        (app, log, harness, consumed)
    }

    #[test]
    fn initial_refresh_precedes_first_draw() {
        let (_, log, _, _) = run_loop(
            vec![Ok(temperature(60))],
            vec![TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(log.borrow()[0..2], ["refresh", "draw"]);
    }

    #[test]
    fn initial_draw_precedes_first_event() {
        let (_, log, _, _) = run_loop(
            vec![Ok(temperature(60))],
            vec![TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(log.borrow()[0..3], ["refresh", "draw", "event"]);
    }

    #[test]
    fn immediate_quit_refreshes_and_draws_once() {
        let (_, _, harness, consumed) = run_loop(
            vec![Ok(temperature(60))],
            vec![TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(harness.draws.get(), 1);
        assert_eq!(consumed, 1);
    }

    #[test]
    fn navigation_actions_redraw_without_refresh() {
        for action in [
            AppAction::NextScreen,
            AppAction::PreviousScreen,
            AppAction::GoTo(Screen::Fans),
            AppAction::ToggleHelp,
        ] {
            let (app, log, harness, _) = run_loop(
                vec![Ok(temperature(60))],
                vec![TuiEvent::Action(action), TuiEvent::Action(AppAction::Quit)],
            );
            assert_eq!(harness.draws.get(), 2);
            assert_eq!(
                log.borrow()
                    .iter()
                    .filter(|step| **step == "refresh")
                    .count(),
                1
            );
            let _ = app;
        }
    }

    #[test]
    fn tick_refreshes_and_redraws_once() {
        let (_, _, harness, _) = run_loop(
            vec![Ok(temperature(60)), Ok(temperature(61))],
            vec![TuiEvent::Tick, TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(harness.draws.get(), 2);
    }

    #[test]
    fn two_ticks_refresh_twice_more() {
        let (app, _, harness, _) = run_loop(
            vec![
                Ok(temperature(60)),
                Ok(temperature(61)),
                Ok(temperature(62)),
            ],
            vec![
                TuiEvent::Tick,
                TuiEvent::Tick,
                TuiEvent::Action(AppAction::Quit),
            ],
        );
        assert_eq!(harness.draws.get(), 3);
        assert_eq!(app.live().history().len(), 3);
    }

    #[test]
    fn resize_redraws_without_refresh() {
        let (_, log, harness, _) = run_loop(
            vec![Ok(temperature(60))],
            vec![
                TuiEvent::Resize {
                    width: 80,
                    height: 24,
                },
                TuiEvent::Action(AppAction::Quit),
            ],
        );
        assert_eq!(harness.draws.get(), 2);
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|step| **step == "refresh")
                .count(),
            1
        );
    }

    #[test]
    fn ignored_event_redraws_and_refreshes_nothing() {
        let (_, log, harness, _) = run_loop(
            vec![Ok(temperature(60))],
            vec![TuiEvent::Ignored, TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(harness.draws.get(), 1);
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|step| **step == "refresh")
                .count(),
            1
        );
    }

    #[test]
    fn quit_requests_no_further_events() {
        let (_, _, _, consumed) = run_loop(
            vec![Ok(temperature(60))],
            vec![
                TuiEvent::Action(AppAction::Quit),
                TuiEvent::Action(AppAction::NextScreen),
            ],
        );
        assert_eq!(consumed, 1);
    }

    #[test]
    fn quit_causes_no_final_redraw() {
        let (_, _, harness, _) = run_loop(
            vec![Ok(temperature(60))],
            vec![TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(harness.draws.get(), 1);
    }

    #[test]
    fn event_source_error_propagates_without_extra_work() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        let mut source = LoopSource::failing(Rc::clone(&log));
        let mut draws = 0;
        let error = run_tui_loop(&mut app, &mut source, TIMEOUT, |_| {
            draws += 1;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
        assert_eq!(draws, 1);
        assert_eq!(
            log.borrow()
                .iter()
                .filter(|step| **step == "refresh")
                .count(),
            1
        );
        assert!(!app.state().should_quit());
    }

    #[test]
    fn draw_error_propagates_immediately() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        let mut source = LoopSource::events(
            vec![TuiEvent::Tick, TuiEvent::Action(AppAction::Quit)],
            Rc::clone(&log),
        );
        let error = run_tui_loop(&mut app, &mut source, TIMEOUT, |_| {
            Err(std::io::Error::other("draw blown"))
        })
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
        assert_eq!(source.events.get(), 0);
    }

    #[test]
    fn refresh_failure_degrades_without_exiting() {
        let (app, _, harness, _) = run_loop(
            vec![Err(BackendError::Unavailable)],
            vec![TuiEvent::Action(AppAction::Quit)],
        );
        assert!(app.live().is_degraded());
        assert_eq!(harness.degraded_at_draw.borrow().as_slice(), [true]);
    }

    #[test]
    fn later_tick_recovers_to_live() {
        let (app, _, harness, _) = run_loop(
            vec![Err(BackendError::Unavailable), Ok(temperature(61))],
            vec![TuiEvent::Tick, TuiEvent::Action(AppAction::Quit)],
        );
        assert!(!app.live().is_degraded());
        assert_eq!(harness.degraded_at_draw.borrow().as_slice(), [true, false]);
        assert_eq!(
            app.live()
                .current_snapshot()
                .unwrap()
                .cpu_temperature
                .unwrap()
                .get(),
            61
        );
    }

    #[test]
    fn failed_refresh_hides_stale_current_sample() {
        let (app, _, harness, _) = run_loop(
            vec![Ok(temperature(60)), Err(BackendError::Unavailable)],
            vec![TuiEvent::Tick, TuiEvent::Action(AppAction::Quit)],
        );
        assert_eq!(app.live().history().len(), 1);
        assert_eq!(
            harness.current_at_draw.borrow().as_slice(),
            [Some(60), None]
        );
    }

    #[test]
    fn loop_uses_configured_timeout() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        let mut source =
            LoopSource::events(vec![TuiEvent::Action(AppAction::Quit)], Rc::clone(&log));
        let timeout = PollInterval::default().as_duration();
        run_tui_loop(&mut app, &mut source, timeout, |_| Ok(())).unwrap();
        assert_eq!(source.timeouts.borrow().as_slice(), [timeout]);
    }

    fn outcome_error(kind: ErrorKind, message: &str) -> std::io::Error {
        std::io::Error::new(kind, message.to_owned())
    }

    #[test]
    fn clean_run_resolves_clean() {
        assert!(resolve_outcome(Ok(()), Ok(())).is_ok());
    }

    #[test]
    fn runtime_error_survives_clean_restore() {
        let error = resolve_outcome(Err(outcome_error(ErrorKind::Other, "runtime boom")), Ok(()))
            .unwrap_err();
        assert!(error.to_string().contains("runtime boom"));
    }

    #[test]
    fn restore_error_surfaces_alone() {
        let error = resolve_outcome(Ok(()), Err(outcome_error(ErrorKind::Other, "restore boom")))
            .unwrap_err();
        assert!(error.to_string().contains("restore boom"));
    }

    #[test]
    fn runtime_error_stays_primary_over_restore_error() {
        let error = resolve_outcome(
            Err(outcome_error(ErrorKind::Other, "runtime boom")),
            Err(outcome_error(ErrorKind::Other, "restore boom")),
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("runtime boom"));
        assert!(text.contains("restore boom"));
    }
}
