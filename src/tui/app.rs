//! Interactive TUI orchestration: startup, loop, and shutdown.
//!
//! [`TuiApp`] owns navigation state, live telemetry, and startup
//! capabilities. [`prepare_tui`] composes the hardware layer once before
//! any terminal takeover; [`run_tui_loop`] refreshes and redraws without
//! ever owning the terminal. Only [`run_tui`] touches a real terminal.

use std::time::Duration;

use thiserror::Error;

use crate::app::{AppState, LiveHardware, ProfileSelection, Screen};
use crate::config::{AppConfig, AppConfigStore, ConfigError};
use crate::hardware::{
    Capabilities, CapabilityDetector, CapabilityDiscoveryError, EcBackend, LinuxSysfsReader,
    MsiEcBackend, SupportEvaluator, SupportMode, SysfsReader, SystemPaths,
};
use crate::monitoring::SnapshotHistory;
use crate::profiles::ProfilePlanner;

use super::confirmation::{PendingMutation, ProfilePending, ProfileSource};
use super::editing::{ControlState, is_interactive_screen};
use super::executor::{SafeTuiExecutor, TuiMutationExecutor};
use super::notifications::NotificationCenter;
use super::palette::{CommandPalette, PaletteCommand};
use super::profile_catalog::ProfileCatalog;
use super::theme::{Theme, ThemeName};
use super::{
    CrosstermEventSource, EventSource, TerminalSession, TuiEvent, render_screen_with_theme,
};

/// Failures that prevent the interactive TUI from running.
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

/// Interactive application state: navigation, live telemetry, the startup
/// capability set, and the prepared read-only profile catalog. Owns no
/// terminal or filesystem handles beyond the backend itself.
pub struct TuiApp<B, E = SafeTuiExecutor> {
    state: AppState,
    live: LiveHardware<B>,
    capabilities: Capabilities,
    profile_catalog: ProfileCatalog,
    profile_selection: ProfileSelection,
    controls: ControlState,
    executor: E,
    palette: CommandPalette,
    notifications: NotificationCenter,
    notifications_open: bool,
    theme_name: ThemeName,
    config: AppConfig,
    config_store: Option<AppConfigStore>,
}

impl<B, E> TuiApp<B, E>
where
    B: EcBackend,
    E: TuiMutationExecutor,
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

    /// Prepared read-only profile catalog: built-ins plus custom files
    /// discovered once before terminal takeover. Ticks never reread it.
    pub fn profile_catalog(&self) -> &ProfileCatalog {
        &self.profile_catalog
    }

    /// Selected profile row on the Profiles screen.
    pub fn profile_selection(&self) -> &ProfileSelection {
        &self.profile_selection
    }

    /// Control editing state: row selections, draft editor, and pending
    /// modal confirmation.
    pub fn controls(&self) -> &ControlState {
        &self.controls
    }

    /// Pending modal confirmation, if any.
    pub fn pending(&self) -> Option<&PendingMutation> {
        self.controls.pending()
    }

    /// Post-attempt result banner, if any.
    pub fn notice(&self) -> Option<&super::confirmation::Notice> {
        self.controls.notice()
    }

    /// Execution adapter (fake in tests).
    pub fn executor(&self) -> &E {
        &self.executor
    }

    /// Non-mutating command palette state.
    pub fn palette(&self) -> &CommandPalette {
        &self.palette
    }

    /// Bounded notification history (newest useful result last).
    pub fn notifications(&self) -> &NotificationCenter {
        &self.notifications
    }

    /// Whether the read-only notification history overlay is visible.
    pub fn notifications_open(&self) -> bool {
        self.notifications_open
    }

    /// Current named theme identity. Presentation only.
    pub fn theme_name(&self) -> ThemeName {
        self.theme_name
    }

    /// Current theme palette. Every overlay in one frame shares it.
    pub fn theme(&self) -> Theme {
        Theme::for_name(self.theme_name)
    }

    /// Loaded user configuration (defaults when unavailable/invalid).
    /// Renderers never read this directly; preparation copies the theme
    /// out and the event loop reads the interval and vim-keys setting.
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    /// Event-loop poll timeout derived from the configured interval.
    pub fn poll_timeout(&self) -> Duration {
        self.config.refresh_interval().as_duration()
    }

    /// Selectable profile rows: five built-ins plus custom entries in
    /// catalog order.
    pub fn profile_row_count(&self) -> usize {
        super::screens::profiles::profile_row_count(&self.profile_catalog)
    }

    /// Contextual action dispatch with confirmed execution.
    ///
    /// Modal input precedence matches visual layering (topmost owns input):
    /// 1. Help visible -> close Help first (`ToggleHelp`/`Cancel` close,
    ///    `Quit` still quits, everything else ignored). Hidden editor and
    ///    pending state underneath are preserved untouched, so a hidden
    ///    mutation UI can never consume `Activate` while Help is on top.
    ///    The first `Esc` closes Help only; a second `Esc` after Help
    ///    closes may cancel the now-visible confirmation or editor.
    /// 2. Pending confirmation -> `Activate` executes exactly once,
    ///    `Cancel` discards, everything else (including `ToggleHelp`)
    ///    ignored. Pending owns input even over test-constructed
    ///    notification/palette overlap states.
    /// 3. Notifications overlay -> `Cancel` or `TogglePalette` closes it,
    ///    everything else ignored. No mutation, no navigation.
    /// 4. Command palette -> row moves, `Activate` runs the selected
    ///    non-mutating command, `Cancel`/`TogglePalette` closes it,
    ///    everything else ignored.
    /// 5. Editor open -> edit/accept/cancel.
    /// 6. Normal screen interaction.
    ///
    /// Browsing (no editor, no pending):
    /// - `MoveUp`/`MoveDown` drive profile rows on Profiles, control rows
    ///   on interactive screens, else screen navigation. While editing they
    ///   are ignored to preserve the draft.
    /// - `MoveLeft`/`MoveRight` adjust the draft while editing, else screen
    ///   navigation. `Tab`/`Shift+Tab` always navigate (clearing an editor).
    /// - `Activate` begins editing, accepts a draft into a command pending,
    ///   or opens a profile confirmation when its pure preview is
    ///   applicable. Opening creates zero executor calls.
    /// - `Cancel` discards editor then pending, else hides help.
    ///
    /// After any attempt that reaches the executor, live state refreshes
    /// once before redraw, on success or error. Opening, cancelling, or
    /// READ-ONLY rejections never refresh and never call the executor. The
    /// stored preview is presentation only; execution re-evaluates fresh
    /// through the safe APIs.
    pub fn handle_action(&mut self, action: crate::app::AppAction) {
        use crate::app::AppAction as A;
        // Help is the topmost overlay: it owns input while visible.
        if self.state.help_visible() {
            match action {
                A::ToggleHelp | A::Cancel | A::Quit => self.state.apply(action),
                _ => {}
            }
            return;
        }
        // Modal confirmation owns input above notifications and palette:
        // a hardware/profile confirmation must never sit beneath a
        // lower-priority utility overlay, even in test-constructed overlap.
        if self.controls.has_pending() {
            match action {
                A::Activate => self.execute_pending(),
                A::Cancel => {
                    self.controls.cancel();
                }
                _ => {}
            }
            return;
        }
        // Notifications overlay owns input while visible: only closing
        // actions apply. P closes it deterministically (never the palette).
        if self.notifications_open {
            match action {
                A::Cancel | A::TogglePalette => self.notifications_open = false,
                _ => {}
            }
            return;
        }
        // Command palette owns input while visible: row moves, activation
        // of one non-mutating command, or close. Hidden control/profile
        // state underneath can never consume actions.
        if self.palette.is_open() {
            match action {
                A::MoveUp => self.palette.move_up(),
                A::MoveDown => self.palette.move_down(),
                A::Activate => self.activate_palette(),
                A::Cancel | A::TogglePalette => self.palette.close(),
                _ => {}
            }
            return;
        }
        let screen = self.state.current_screen();
        match action {
            A::MoveUp if self.controls.is_editing() => {}
            A::MoveDown if self.controls.is_editing() => {}
            A::MoveUp if screen == Screen::Profiles => {
                self.profile_selection.move_up(self.profile_row_count());
            }
            A::MoveDown if screen == Screen::Profiles => {
                self.profile_selection.move_down(self.profile_row_count());
            }
            A::MoveUp if is_interactive_screen(screen) => {
                self.controls.move_up(screen);
            }
            A::MoveDown if is_interactive_screen(screen) => {
                self.controls.move_down(screen);
            }
            A::MoveLeft if self.controls.is_editing() => {
                self.controls.adjust(&self.capabilities, -1);
            }
            A::MoveRight if self.controls.is_editing() => {
                self.controls.adjust(&self.capabilities, 1);
            }
            A::Activate if self.controls.is_editing() => {
                let mode = self.live.mode().clone();
                self.controls.clear_notice();
                self.controls.confirm(&mode, &self.capabilities);
            }
            A::Activate if screen == Screen::Profiles => {
                self.controls.clear_notice();
                self.open_profile_confirmation();
            }
            A::Activate if is_interactive_screen(screen) => {
                let mode = self.live.mode().clone();
                self.controls.clear_notice();
                self.controls.begin_edit(
                    screen,
                    self.live.current_snapshot(),
                    &self.capabilities,
                    &mode,
                );
            }
            A::Activate => {}
            // The palette opens only from normal browsing: no help (gated
            // above), no pending (gated above), no editor. Otherwise P does
            // nothing so it can never bypass mutation modal state.
            A::TogglePalette => {
                if !self.controls.is_editing() {
                    self.palette.open();
                }
            }
            A::Cancel => {
                if !self.controls.cancel() {
                    self.state.apply(action);
                }
            }
            A::NextScreen | A::PreviousScreen | A::GoTo(_) => {
                self.state.apply(action);
                self.controls.on_screen_change();
            }
            A::MoveLeft | A::MoveRight => {
                self.state.apply(action);
                self.controls.on_screen_change();
            }
            _ => self.state.apply(action),
        }
    }

    /// Runs the selected palette command. All commands are non-mutating:
    /// screen jumps mirror digit navigation, Notifications opens the
    /// read-only history overlay, Clear empties history (and the banner),
    /// Help opens Help, Quit requests quit. Zero executor calls.
    fn activate_palette(&mut self) {
        match self.palette.selected() {
            PaletteCommand::Notifications => {
                self.palette.close();
                self.notifications_open = true;
            }
            PaletteCommand::ClearNotifications => {
                self.notifications.clear();
                self.controls.clear_notice();
                self.palette.close();
            }
            PaletteCommand::Help => {
                self.palette.close();
                self.state.apply(crate::app::AppAction::ShowHelp);
            }
            PaletteCommand::Quit => {
                self.palette.close();
                self.state.apply(crate::app::AppAction::Quit);
            }
            command if command.theme().is_some() => {
                let name = command.theme().expect("theme row carries an identity");
                self.palette.close();
                self.switch_theme(name);
            }
            command => {
                if let Some(screen) = command.screen() {
                    self.palette.close();
                    self.state.apply(crate::app::AppAction::GoTo(screen));
                    self.controls.on_screen_change();
                }
            }
        }
    }

    /// Switches the session theme and persists it: the runtime theme
    /// changes immediately, the in-memory config follows, and an atomic
    /// config save is attempted. No backend refresh, no executor call.
    /// Records exactly one notice: success on save, or a truthful
    /// session-only notice when persistence fails (the visible theme is
    /// never rolled back). Selecting the active theme again is harmless.
    fn switch_theme(&mut self, name: ThemeName) {
        self.theme_name = name;
        self.config.set_theme(name);
        let notice = match self.config_store.as_ref() {
            Some(store) => match store.save(&self.config) {
                Ok(()) => super::confirmation::Notice::success(format!(
                    "Theme changed to {}",
                    name.display_name()
                )),
                Err(error) => super::confirmation::Notice::failure(format!(
                    "Theme changed to {} for this session; config save failed: {error}",
                    name.display_name()
                )),
            },
            None => super::confirmation::Notice::success(format!(
                "Theme changed to {}",
                name.display_name()
            )),
        };
        self.notifications.push(notice.clone());
        self.controls.set_notice(notice);
    }

    /// Opens a profile confirmation only when the pure preview deems the
    /// retained profile applicable. Unavailable built-ins, invalid customs,
    /// and READ-ONLY rejections create nothing and call nothing.
    fn open_profile_confirmation(&mut self) {
        use super::screens::profiles::ProfileRow;
        use super::screens::profiles::profile_row;
        let index = self.profile_selection.index();
        let mode = self.live.mode().clone();
        let capabilities = self.capabilities.clone();
        let Some(row) = profile_row(index, &self.profile_catalog) else {
            return;
        };
        match row {
            ProfileRow::Builtin(preset) => {
                let Ok(profile) = preset.resolve(&capabilities) else {
                    return;
                };
                if !ProfilePlanner::preview(&profile, &mode, &capabilities).is_applicable() {
                    return;
                }
                self.controls.set_profile_pending(ProfilePending::new(
                    profile,
                    preset.slug().to_owned(),
                    ProfileSource::Builtin,
                ));
            }
            ProfileRow::Custom(position) => {
                let Some(entry) = self.profile_catalog.customs().get(position) else {
                    return;
                };
                let Some(profile) = entry.profile() else {
                    return;
                };
                if !ProfilePlanner::preview(profile, &mode, &capabilities).is_applicable() {
                    return;
                }
                self.controls.set_profile_pending(ProfilePending::new(
                    profile.clone(),
                    entry.slug().as_str().to_owned(),
                    ProfileSource::Custom,
                ));
            }
        }
    }

    /// Executes the pending modal exactly once, clears it before any repeat
    /// key can re-run it, refreshes live state once, and records a Display
    /// notice. At most one executor call per confirm.
    fn execute_pending(&mut self) {
        let Some(pending) = self.controls.pending().cloned() else {
            return;
        };
        // Clear before refresh/draw so key repeat cannot re-execute.
        self.controls.cancel();
        match pending {
            PendingMutation::Command(command) => {
                let result = self.executor.execute_command(&command);
                self.live.refresh();
                let notice = match result {
                    Ok(()) => super::confirmation::Notice::success(format!(
                        "Applied {}",
                        crate::tui::controls::command_text(&command)
                    )),
                    Err(error) => {
                        super::confirmation::Notice::failure(format!("Action failed: {error}"))
                    }
                };
                self.notifications.push(notice.clone());
                self.controls.set_notice(notice);
            }
            PendingMutation::Profile(request) => {
                let result = self.executor.apply_profile(request.profile());
                self.live.refresh();
                let notice = match result {
                    Ok(summary) => super::confirmation::Notice::success(format!(
                        "Applied profile {} ({} changed, {} unchanged)",
                        summary.name, summary.applied, summary.unchanged
                    )),
                    Err(error) => {
                        super::confirmation::Notice::failure(format!("Action failed: {error}"))
                    }
                };
                self.notifications.push(notice.clone());
                self.controls.set_notice(notice);
            }
        }
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
///
/// Custom profiles come from the platform default store; any store,
/// listing, or load failure degrades to a nonfatal catalog state instead
/// of aborting startup, and built-ins always remain visible.
pub fn prepare_tui<R>(paths: SystemPaths, reader: R) -> Result<TuiApp<MsiEcBackend<R>>, TuiError>
where
    R: SysfsReader + Clone,
{
    let profile_catalog = match crate::profiles::ProfileStore::user_default() {
        Ok(store) => ProfileCatalog::from_store(&store),
        Err(_) => ProfileCatalog::unavailable(),
    };
    prepare_tui_with_catalog(paths, reader, profile_catalog)
}

/// Injectable preparation path for tests: identical hardware composition
/// with a caller-supplied read-only catalog, so tests never touch the
/// developer's real `~/.config/mec/profiles/`.
pub fn prepare_tui_with_profile_store<R>(
    paths: SystemPaths,
    reader: R,
    store: crate::profiles::ProfileStore,
) -> Result<TuiApp<MsiEcBackend<R>>, TuiError>
where
    R: SysfsReader + Clone,
{
    prepare_tui_with_catalog(paths, reader, ProfileCatalog::from_store(&store))
}

fn prepare_tui_with_catalog<R>(
    paths: SystemPaths,
    reader: R,
    profile_catalog: ProfileCatalog,
) -> Result<TuiApp<MsiEcBackend<R>>, TuiError>
where
    R: SysfsReader + Clone,
{
    let store = AppConfigStore::user_default().ok();
    let loaded = match store.as_ref() {
        Some(store) => store.load(),
        None => Err(ConfigError::ConfigDirectoryUnavailable),
    };
    prepare_tui_full(paths, reader, profile_catalog, loaded, store)
}

/// Full preparation with an explicit config outcome plus an optional
/// persistence store. Production passes the user-default load result;
/// tests inject documents or failures deterministically without touching
/// the real home directory.
///
/// A failed load never aborts monitoring: defaults apply and one startup
/// notice records the safe Display error (path-free by construction).
fn prepare_tui_full<R>(
    paths: SystemPaths,
    reader: R,
    profile_catalog: ProfileCatalog,
    loaded: Result<AppConfig, ConfigError>,
    config_store: Option<AppConfigStore>,
) -> Result<TuiApp<MsiEcBackend<R>>, TuiError>
where
    R: SysfsReader + Clone,
{
    // Same root for reads and the production executor by construction.
    let executor = SafeTuiExecutor::new(paths.clone());
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
    let mut app = TuiApp {
        state: AppState::default(),
        live: LiveHardware::new(device, mode, backend, SnapshotHistory::default()),
        capabilities,
        profile_catalog,
        profile_selection: ProfileSelection::default(),
        controls: ControlState::default(),
        executor,
        palette: CommandPalette::default(),
        notifications: NotificationCenter::new(),
        notifications_open: false,
        theme_name: ThemeName::MsiDark,
        config: AppConfig::default(),
        config_store: None,
    };
    apply_loaded_config(&mut app, loaded, config_store);
    Ok(app)
}

/// Applies a loaded config outcome to a prepared app: valid configs choose
/// the initial theme, failures fall back to defaults with one startup
/// notice. Shared by production preparation and config-injection tests.
fn apply_loaded_config<B, E>(
    app: &mut TuiApp<B, E>,
    loaded: Result<AppConfig, ConfigError>,
    config_store: Option<AppConfigStore>,
) where
    B: EcBackend,
    E: TuiMutationExecutor,
{
    match loaded {
        Ok(config) => {
            app.theme_name = config.theme();
            app.config = config;
            app.config_store = config_store;
        }
        Err(error) => {
            app.config_store = config_store;
            let notice = super::confirmation::Notice::failure(format!(
                "Config load failed (using defaults): {error}"
            ));
            app.notifications.push(notice.clone());
            app.controls.set_notice(notice);
        }
    }
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
pub fn run_tui_loop<B, X, Ev, D>(
    app: &mut TuiApp<B, X>,
    events: &mut Ev,
    timeout: Duration,
    mut draw: D,
) -> std::io::Result<()>
where
    B: EcBackend,
    X: TuiMutationExecutor,
    Ev: EventSource,
    D: FnMut(&mut TuiApp<B, X>) -> std::io::Result<()>,
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
fn step_tui_loop<B, X, Ev, D>(
    app: &mut TuiApp<B, X>,
    events: &mut Ev,
    timeout: Duration,
    draw: &mut D,
) -> std::io::Result<()>
where
    B: EcBackend,
    X: TuiMutationExecutor,
    Ev: EventSource,
    D: FnMut(&mut TuiApp<B, X>) -> std::io::Result<()>,
{
    match events.next_event(timeout)? {
        TuiEvent::Action(action) => {
            app.handle_action(action);
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
    // The configured interval drives the loop timeout and the configured
    // vim-keys setting drives input mapping; CLI monitor semantics are
    // untouched.
    let timeout = app.poll_timeout();
    let mut events = CrosstermEventSource::with_vim_keys(app.config().vim_keys());
    let runtime = run_tui_loop(&mut app, &mut events, timeout, |app| {
        session
            .terminal_mut()
            .draw(|frame| {
                render_screen_with_theme(
                    frame,
                    frame.area(),
                    app.state(),
                    app.live(),
                    app.capabilities(),
                    app.profile_catalog(),
                    app.profile_selection(),
                    app.controls(),
                    app.palette(),
                    app.notifications(),
                    app.notifications_open(),
                    &app.theme(),
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

    use crate::app::{AppAction, AppState, LiveHardware, ProfileSelection, Screen};
    use crate::hardware::{
        BackendError, Capabilities, DeviceInfo, EcBackend, HardwareSnapshot, ReadOnlyReason,
        SupportMode, SystemPaths, TemperatureCelsius,
    };
    use crate::monitoring::{PollInterval, SnapshotHistory};

    use super::{
        TuiApp, prepare_tui, prepare_tui_with_profile_store, resolve_outcome, run_tui_loop,
        should_launch_tui,
    };
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
    fn package_version_is_v1_0_1() {
        assert_eq!(env!("CARGO_PKG_VERSION"), "1.0.1");
    }

    #[test]
    fn injected_empty_store_preparation_succeeds() {
        let (_dir, store) = catalog_store();
        let app = prepare_tui_with_profile_store(
            fixture_root("gf63"),
            crate::hardware::LinuxSysfsReader,
            store,
        )
        .expect("empty store must not abort startup");
        assert!(app.profile_catalog().customs().is_empty());
        assert!(app.profile_catalog().customs_available());
        assert_eq!(app.live().mode(), &SupportMode::Ready);
        assert_eq!(app.live().current_snapshot(), None);
    }

    #[test]
    fn injected_valid_customs_appear_in_catalog() {
        let (_dir, store) = catalog_store();
        write_profile(
            &store,
            "work.toml",
            b"name = \"Startup Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        );
        let app = prepare_tui_with_profile_store(
            fixture_root("gf63"),
            crate::hardware::LinuxSysfsReader,
            store,
        )
        .expect("valid customs must not abort startup");
        assert_eq!(app.profile_catalog().customs().len(), 1);
        let entry = &app.profile_catalog().customs()[0];
        assert!(entry.is_valid());
        assert_eq!(entry.name().unwrap().as_str(), "Startup Work");
        assert_eq!(entry.profile().unwrap().name().as_str(), "Startup Work");
    }

    #[test]
    fn malformed_custom_does_not_abort_startup() {
        let (_dir, store) = catalog_store();
        write_profile(&store, "bad.toml", b"name = [unclosed\n");
        write_profile(
            &store,
            "work.toml",
            b"name = \"Startup Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        );
        let app = prepare_tui_with_profile_store(
            fixture_root("gf63"),
            crate::hardware::LinuxSysfsReader,
            store,
        )
        .expect("malformed custom must not abort startup");
        assert_eq!(app.profile_catalog().customs().len(), 2);
        assert!(!app.profile_catalog().customs()[0].is_valid());
        assert!(app.profile_catalog().customs()[1].is_valid());
    }

    #[test]
    fn missing_custom_directory_does_not_abort_startup() {
        let dir = tempfile::tempdir().expect("missing-dir TempDir constructs");
        let store = crate::profiles::ProfileStore::new(dir.path().join("profiles"));
        let app = prepare_tui_with_profile_store(
            fixture_root("gf63"),
            crate::hardware::LinuxSysfsReader,
            store,
        )
        .expect("missing directory must not abort startup");
        assert!(app.profile_catalog().customs().is_empty());
        assert!(app.profile_catalog().customs_available());
        assert!(!dir.path().join("profiles").exists());
    }

    #[test]
    fn list_level_failure_does_not_abort_startup() {
        // A regular file where the directory should be fails listing
        // deterministically without chmod games.
        let dir = tempfile::tempdir().expect("unreadable TempDir constructs");
        let profiles = dir.path().join("profiles");
        std::fs::write(&profiles, b"not a directory\n").expect("blocker writes");
        let store = crate::profiles::ProfileStore::new(&profiles);
        let app = prepare_tui_with_profile_store(
            fixture_root("gf63"),
            crate::hardware::LinuxSysfsReader,
            store,
        )
        .expect("list failure must not abort startup");
        assert!(!app.profile_catalog().customs_available());
        assert!(app.profile_catalog().customs().is_empty());
        assert_eq!(app.live().mode(), &SupportMode::Ready);
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
    ) -> TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor> {
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
            profile_catalog: crate::tui::ProfileCatalog::empty(),
            profile_selection: ProfileSelection::default(),
            controls: crate::tui::editing::ControlState::default(),
            executor: crate::tui::executor::FakeTuiExecutor::new(),
            palette: crate::tui::palette::CommandPalette::default(),
            notifications: crate::tui::notifications::NotificationCenter::new(),
            notifications_open: false,
            theme_name: crate::tui::theme::ThemeName::MsiDark,
            config: crate::config::AppConfig::default(),
            config_store: None,
        }
    }

    fn catalog_store() -> (tempfile::TempDir, crate::profiles::ProfileStore) {
        let dir = tempfile::tempdir().expect("catalog TempDir constructs");
        let store = crate::profiles::ProfileStore::new(dir.path().join("profiles"));
        (dir, store)
    }

    fn write_profile(store: &crate::profiles::ProfileStore, name: &str, contents: &[u8]) {
        std::fs::create_dir_all(store.directory()).expect("catalog dir constructs");
        std::fs::write(store.directory().join(name), contents).expect("profile writes");
    }

    #[allow(clippy::type_complexity)]
    fn run_loop(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
        events: Vec<TuiEvent>,
    ) -> (
        TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor>,
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

    // ---- Task 3: contextual profile selection dispatch ----

    fn selection_app_on_profiles() -> TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor> {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        assert_eq!(app.state().current_screen(), Screen::Profiles);
        app
    }

    #[test]
    fn initial_selection_is_first_builtin() {
        let app = loop_app(vec![Ok(temperature(60))], Rc::new(RefCell::new(Vec::new())));
        assert_eq!(app.profile_selection().index(), 0);
        assert!(app.profile_row_count() >= 5);
    }

    #[test]
    fn move_down_on_profiles_advances_selection_not_screen() {
        let mut app = selection_app_on_profiles();
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.state().current_screen(), Screen::Profiles);
        assert_eq!(app.profile_selection().index(), 1);
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.profile_selection().index(), 2);
    }

    #[test]
    fn move_up_on_profiles_wraps_to_last_row() {
        let mut app = selection_app_on_profiles();
        let count = app.profile_row_count();
        app.handle_action(AppAction::MoveUp);
        assert_eq!(app.state().current_screen(), Screen::Profiles);
        assert_eq!(app.profile_selection().index(), count - 1);
    }

    #[test]
    fn move_down_traverses_into_custom_rows() {
        let (dir, store) = catalog_store();
        write_profile(
            &store,
            "work.toml",
            b"name = \"App Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        );
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        app.profile_catalog = crate::tui::ProfileCatalog::from_store(&store);
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        assert_eq!(app.profile_row_count(), 6);
        for _ in 0..5 {
            app.handle_action(AppAction::MoveDown);
        }
        assert_eq!(app.profile_selection().index(), 5);
        let _ = dir;
    }

    #[test]
    fn invalid_custom_remains_selectable_through_dispatch() {
        let (dir, store) = catalog_store();
        write_profile(&store, "bad.toml", b"name = [unclosed\n");
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        app.profile_catalog = crate::tui::ProfileCatalog::from_store(&store);
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        for _ in 0..5 {
            app.handle_action(AppAction::MoveDown);
        }
        assert_eq!(app.profile_selection().index(), 5);
        assert!(!app.profile_catalog().customs()[0].is_valid());
        let _ = dir;
    }

    #[test]
    fn move_keys_fall_back_to_screen_navigation_off_profiles() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(temperature(60))], Rc::clone(&log));
        // Dashboard has no control rows: vertical moves fall back to screens.
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
        app.handle_action(AppAction::MoveUp);
        assert_eq!(app.state().current_screen(), Screen::Diagnostics);
        // Diagnostics also has no rows.
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
        // Interactive screens consume vertical moves as row navigation.
        app.handle_action(AppAction::GoTo(Screen::Performance));
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.state().current_screen(), Screen::Performance);
        assert_eq!(app.controls().selected_index(Screen::Performance), 1);
    }

    #[test]
    fn digit_navigation_still_jumps_directly() {
        let mut app = selection_app_on_profiles();
        app.handle_action(AppAction::GoTo(Screen::Fans));
        assert_eq!(app.state().current_screen(), Screen::Fans);
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        assert_eq!(app.state().current_screen(), Screen::Profiles);
        // Selection survives screen excursions.
        assert_eq!(app.profile_selection().index(), 0);
    }

    // ---- Task 4: control editing dispatch (no writes) ----

    fn healthy_control_app(
        screen: Screen,
    ) -> TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor> {
        use crate::tui::screens::support::healthy_snapshot;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(healthy_snapshot()),
                Ok(healthy_snapshot()),
                Ok(healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.refresh();
        app.handle_action(AppAction::GoTo(screen));
        app
    }

    fn read_only_control_app(
        screen: Screen,
    ) -> TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor> {
        use crate::hardware::{ReadOnlyReason, SupportMode};
        use crate::tui::screens::support::healthy_snapshot;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(healthy_snapshot()),
                Ok(healthy_snapshot()),
                Ok(healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        // Rebuild live state in READ-ONLY mode with the same healthy snapshot.
        app.live = LiveHardware::new(
            app.live.device().clone(),
            SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable),
            LoopBackend {
                script: RefCell::new(
                    vec![
                        Ok(healthy_snapshot()),
                        Ok(healthy_snapshot()),
                        Ok(healthy_snapshot()),
                    ]
                    .into(),
                ),
                log: Rc::clone(&log),
            },
            SnapshotHistory::default(),
        );
        app.refresh();
        app.handle_action(AppAction::GoTo(screen));
        app
    }

    #[test]
    fn control_row_navigation_for_each_interactive_screen() {
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
        ] {
            let mut app = healthy_control_app(screen);
            let before = app.controls().selected_index(screen);
            app.handle_action(AppAction::MoveDown);
            assert_eq!(app.state().current_screen(), screen);
            let rows = crate::tui::editing::control_rows(screen).len();
            assert_eq!(app.controls().selected_index(screen), (before + 1) % rows);
            app.handle_action(AppAction::MoveUp);
            assert_eq!(app.controls().selected_index(screen), before);
        }
    }

    #[test]
    fn dashboard_vertical_moves_still_navigate_screens() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.state().current_screen(), Screen::Performance);
    }

    #[test]
    fn enter_begins_edit_and_accepts_pending_only() {
        let mut app = healthy_control_app(Screen::Fans);
        assert!(!app.controls().is_editing());
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_some());
    }

    #[test]
    fn pending_command_is_typed_fan_mode() {
        use crate::tui::confirmation::PendingMutation;
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        let pending = app.controls().pending().expect("pending stored");
        assert!(matches!(pending, PendingMutation::Command(_)));
    }

    #[test]
    fn esc_discards_editor_then_pending() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        app.handle_action(AppAction::Cancel);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.handle_action(AppAction::Cancel);
        assert!(app.controls().pending().is_none());
    }

    #[test]
    fn left_right_adjust_draft_while_editing_otherwise_navigate() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        let before = app.controls().editor().unwrap().draft().clone();
        app.handle_action(AppAction::MoveRight);
        assert_eq!(app.state().current_screen(), Screen::Fans);
        let after = app.controls().editor().unwrap().draft().clone();
        assert_ne!(before, after);
        app.handle_action(AppAction::MoveLeft);
        assert_eq!(app.controls().editor().unwrap().draft(), &before);
        app.handle_action(AppAction::Cancel);
        app.handle_action(AppAction::MoveRight);
        assert_eq!(app.state().current_screen(), Screen::Battery);
    }

    #[test]
    fn tab_always_navigates_even_while_editing() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        app.handle_action(AppAction::NextScreen);
        assert_eq!(app.state().current_screen(), Screen::Battery);
        assert!(!app.controls().is_editing());
    }

    #[test]
    fn read_only_never_enters_edit_mode() {
        let mut app = read_only_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
    }

    #[test]
    fn unsupported_capability_blocks_edit() {
        let mut app = healthy_control_app(Screen::Fans);
        app.capabilities = crate::hardware::Capabilities::default();
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
    }

    #[test]
    fn fn_win_info_rows_never_edit() {
        let mut app = healthy_control_app(Screen::Devices);
        // Devices rows: Webcam(0), WebcamBlock(1), Backlight(2), Fn(3), Win(4).
        for _ in 0..3 {
            app.handle_action(AppAction::MoveDown);
        }
        assert_eq!(
            app.controls().selected(Screen::Devices),
            Some(crate::tui::editing::ControlId::FnKeyInfo)
        );
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
        app.handle_action(AppAction::MoveDown);
        assert_eq!(
            app.controls().selected(Screen::Devices),
            Some(crate::tui::editing::ControlId::WinKeyInfo)
        );
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
    }

    #[test]
    fn enter_on_profiles_opens_confirmation_without_execution() {
        let mut app = healthy_control_app(Screen::Profiles);
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
        // Balanced with full capabilities previews applicable: confirmation
        // opens, but the executor is not called until a second Enter.
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
    }

    #[test]
    fn edit_confirm_causes_zero_backend_refresh_calls() {
        // Editing validates purely: no live refresh occurs through dispatch.
        // LoopBackend counts refreshes via draws in run_loop; here dispatch
        // alone must not pop extra snapshots. A second refresh would exhaust
        // the single-script backend, so reaching pending proves zero reads.
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
    }

    // ---- Task 5: confirmed execution through fake executor ----

    #[test]
    fn opening_command_confirmation_makes_zero_calls() {
        let mut app = healthy_control_app(Screen::Fans);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        assert!(app.notice().is_none());
    }

    #[test]
    fn cancelling_command_confirmation_makes_zero_calls() {
        let mut app = healthy_control_app(Screen::Fans);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Cancel);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        assert!(app.notice().is_none());
    }

    #[test]
    fn confirming_command_executes_exactly_once_with_exact_command() {
        use crate::hardware::{FanMode, HardwareCommand};
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        // Adjust auto -> silent for an exact typed expectation.
        app.handle_action(AppAction::MoveRight);
        app.handle_action(AppAction::Activate);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        let expected = HardwareCommand::SetFanMode(FanMode::try_from("silent").unwrap());
        assert_eq!(app.executor.received_commands(), &[expected]);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.live().history().len(), history_before + 1);
        let notice = app.notice().expect("success notice");
        assert!(notice.message().contains("Applied Fan Mode: silent"));
        assert!(!notice.message().contains("RPM"));
    }

    #[test]
    fn key_repeat_cannot_execute_twice_without_new_pending() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        // Repeat Enter with no pending does nothing further.
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
    }

    #[test]
    fn read_only_command_makes_zero_calls() {
        let mut app = read_only_control_app(Screen::Fans);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn unsupported_command_makes_zero_calls() {
        let mut app = healthy_control_app(Screen::Fans);
        app.capabilities = crate::hardware::Capabilities::default();
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn command_failure_notice_uses_display_error() {
        use crate::hardware::{CommandValidationError, FanMode, HardwareCommand};
        use crate::safety::CommandExecutionError;
        use crate::tui::executor::FakeTuiExecutor;
        // Build an app whose fake fails with a typed boundary error.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.executor = FakeTuiExecutor::with_command_error(CommandExecutionError::Validation(
            CommandValidationError::FanModeNotAdvertised(FanMode::try_from("silent").unwrap()),
        ));
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Fans));
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        assert_eq!(app.live().history().len(), history_before + 1);
        let notice = app.notice().expect("failure notice");
        assert!(notice.message().contains("Action failed:"));
        assert!(notice.message().contains("fan mode not advertised"));
        assert!(!notice.message().contains("Boundary"));
        let _ = HardwareCommand::SetFanMode(FanMode::try_from("auto").unwrap());
    }

    #[test]
    fn profile_builtin_can_open_confirmation_without_calls() {
        let mut app = healthy_control_app(Screen::Profiles);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn profile_valid_custom_opens_without_reopening_file() {
        let (dir, store) = catalog_store();
        write_profile(
            &store,
            "work.toml",
            b"name = \"Custom Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        );
        let catalog = crate::tui::ProfileCatalog::from_store(&store);
        // Delete source files: retained catalog must open alone.
        std::fs::remove_dir_all(store.directory()).expect("source removed");
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.profile_catalog = catalog;
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        for _ in 0..5 {
            app.handle_action(AppAction::MoveDown);
        }
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        let _ = dir;
    }

    #[test]
    fn profile_invalid_custom_cannot_open() {
        let (dir, store) = catalog_store();
        write_profile(&store, "bad.toml", b"name = [unclosed\n");
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![Ok(crate::tui::screens::support::healthy_snapshot())],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.profile_catalog = crate::tui::ProfileCatalog::from_store(&store);
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        for _ in 0..5 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.profile_calls(), 0);
        let _ = dir;
    }

    #[test]
    fn profile_unavailable_builtin_cannot_open() {
        let mut app = healthy_control_app(Screen::Profiles);
        app.capabilities = crate::hardware::Capabilities::default();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.profile_calls(), 0);
    }

    #[test]
    fn profile_read_only_cannot_execute() {
        let mut app = read_only_control_app(Screen::Profiles);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn cancelling_profile_confirmation_makes_zero_calls() {
        let mut app = healthy_control_app(Screen::Profiles);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.handle_action(AppAction::Cancel);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        assert!(app.notice().is_none());
    }

    #[test]
    fn confirming_profile_executes_once_with_exact_retained_profile() {
        let (dir, store) = catalog_store();
        write_profile(
            &store,
            "work.toml",
            b"name = \"Exact Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        );
        let expected = store.load(&"work".parse().unwrap()).expect("profile loads");
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.profile_catalog = crate::tui::ProfileCatalog::from_store(&store);
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        for _ in 0..5 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.profile_calls(), 1);
        assert_eq!(app.executor.received_profiles(), &[expected]);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.live().history().len(), history_before + 1);
        let notice = app.notice().expect("profile success notice");
        assert!(notice.message().contains("Applied profile"));
        assert!(notice.message().contains("changed"));
        let _ = dir;
    }

    #[test]
    fn profile_success_notice_shows_report_counts() {
        use crate::tui::executor::{FakeTuiExecutor, ProfileApplySummary};
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.executor = FakeTuiExecutor::with_profile_summary(ProfileApplySummary {
            name: "Gaming".to_owned(),
            applied: 2,
            unchanged: 1,
        });
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        let notice = app.notice().expect("counts notice");
        assert!(notice.message().contains("Gaming"));
        assert!(notice.message().contains("2 changed"));
        assert!(notice.message().contains("1 unchanged"));
    }

    #[test]
    fn profile_failure_notice_uses_display_error() {
        use crate::hardware::CommandValidationError;
        use crate::safety::ProfileApplyError;
        use crate::tui::executor::FakeTuiExecutor;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.executor =
            FakeTuiExecutor::with_profile_error(ProfileApplyError::PreviewRejected(vec![
                CommandValidationError::ReadOnly,
            ]));
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        app.handle_action(AppAction::Activate);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.profile_calls(), 1);
        assert_eq!(app.live().history().len(), history_before + 1);
        let notice = app.notice().expect("failure notice");
        assert!(notice.message().contains("Action failed:"));
        assert!(!notice.message().contains("PreviewRejected("));
    }

    #[test]
    fn modal_pending_blocks_navigation_until_resolved() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.handle_action(AppAction::GoTo(Screen::Battery));
        assert_eq!(app.state().current_screen(), Screen::Fans);
        app.handle_action(AppAction::NextScreen);
        assert_eq!(app.state().current_screen(), Screen::Fans);
        app.handle_action(AppAction::Cancel);
        app.handle_action(AppAction::GoTo(Screen::Battery));
        assert_eq!(app.state().current_screen(), Screen::Battery);
    }
    // ---- Task 6: command palette + notifications ----

    #[test]
    fn p_opens_palette_from_browsing() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        assert!(app.palette().is_open());
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert!(app.notifications().is_empty());
    }

    #[test]
    fn palette_cannot_open_above_help() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::ShowHelp);
        app.handle_action(AppAction::TogglePalette);
        assert!(!app.palette().is_open());
        assert!(app.state().help_visible());
    }

    #[test]
    fn palette_cannot_open_above_pending() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.handle_action(AppAction::TogglePalette);
        assert!(!app.palette().is_open());
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.command_calls(), 0);
    }

    #[test]
    fn palette_cannot_open_while_editing() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        app.handle_action(AppAction::TogglePalette);
        assert!(!app.palette().is_open());
        assert!(app.controls().is_editing());
    }

    #[test]
    fn palette_rows_wrap_deterministically() {
        use crate::tui::palette::PaletteCommand;
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..PaletteCommand::ALL.len() {
            app.handle_action(AppAction::MoveDown);
        }
        assert_eq!(app.palette().selected_index(), 0);
        app.handle_action(AppAction::MoveUp);
        assert_eq!(app.palette().selected(), PaletteCommand::ThemeLight);
        app.handle_action(AppAction::MoveUp);
        assert_eq!(app.palette().selected(), PaletteCommand::ThemeTerminal);
    }

    #[test]
    fn palette_screen_rows_navigate_exactly() {
        let cases = [
            (0, Screen::Dashboard),
            (1, Screen::Performance),
            (2, Screen::Fans),
            (3, Screen::Battery),
            (4, Screen::Devices),
            (5, Screen::Profiles),
            (6, Screen::Diagnostics),
        ];
        for (steps, screen) in cases {
            let mut app = healthy_control_app(Screen::Dashboard);
            app.handle_action(AppAction::TogglePalette);
            for _ in 0..steps {
                app.handle_action(AppAction::MoveDown);
            }
            app.handle_action(AppAction::Activate);
            assert_eq!(app.state().current_screen(), screen);
            assert!(!app.palette().is_open());
            assert!(!app.controls().is_editing());
            assert!(app.controls().pending().is_none());
            assert_eq!(app.executor.command_calls(), 0);
            assert_eq!(app.executor.profile_calls(), 0);
        }
    }

    #[test]
    fn palette_notifications_opens_overlay() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..7 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert!(!app.palette().is_open());
        assert!(app.notifications_open());
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
    }

    #[test]
    fn palette_clear_empties_history_and_banner() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.notifications().len(), 1);
        assert!(app.notice().is_some());
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..8 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert!(!app.palette().is_open());
        assert!(app.notifications().is_empty());
        assert!(app.notice().is_none());
    }

    #[test]
    fn palette_help_opens_help() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..9 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert!(!app.palette().is_open());
        assert!(app.state().help_visible());
    }

    #[test]
    fn palette_quit_requests_quit() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..10 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert!(!app.palette().is_open());
        assert!(app.state().should_quit());
    }

    #[test]
    fn esc_and_p_close_palette() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        app.handle_action(AppAction::Cancel);
        assert!(!app.palette().is_open());
        app.handle_action(AppAction::TogglePalette);
        assert!(app.palette().is_open());
        app.handle_action(AppAction::TogglePalette);
        assert!(!app.palette().is_open());
    }

    #[test]
    fn palette_blocks_hidden_state_changes() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::TogglePalette);
        app.handle_action(AppAction::GoTo(Screen::Battery));
        assert_eq!(app.state().current_screen(), Screen::Fans);
        assert!(app.palette().is_open());
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.palette().selected_index(), 1);
        assert_eq!(app.controls().selected_index(Screen::Fans), 0);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.state().current_screen(), Screen::Performance);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
    }

    #[test]
    fn command_success_adds_one_notification_mirroring_banner() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.notifications().len(), 0);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("one notification");
        assert!(latest.message().contains("Applied Fan Mode:"));
        assert_eq!(
            latest.message(),
            app.notice().expect("banner mirrors latest").message()
        );
    }

    #[test]
    fn command_failure_adds_one_notification() {
        use crate::hardware::{CommandValidationError, FanMode};
        use crate::safety::CommandExecutionError;
        use crate::tui::executor::FakeTuiExecutor;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.executor = FakeTuiExecutor::with_command_error(CommandExecutionError::Validation(
            CommandValidationError::FanModeNotAdvertised(FanMode::try_from("silent").unwrap()),
        ));
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Fans));
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("one notification");
        assert!(latest.message().contains("Action failed:"));
        assert!(latest.message().contains("fan mode not advertised"));
    }

    #[test]
    fn profile_success_adds_one_notification() {
        let mut app = healthy_control_app(Screen::Profiles);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.notifications().len(), 0);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.profile_calls(), 1);
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("one notification");
        assert!(latest.message().contains("Applied profile"));
    }

    #[test]
    fn profile_failure_adds_one_notification() {
        use crate::hardware::CommandValidationError;
        use crate::safety::ProfileApplyError;
        use crate::tui::executor::FakeTuiExecutor;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(
            vec![
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
                Ok(crate::tui::screens::support::healthy_snapshot()),
            ],
            Rc::clone(&log),
        );
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.executor =
            FakeTuiExecutor::with_profile_error(ProfileApplyError::PreviewRejected(vec![
                CommandValidationError::ReadOnly,
            ]));
        app.refresh();
        app.handle_action(AppAction::GoTo(Screen::Profiles));
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.profile_calls(), 1);
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("one notification");
        assert!(latest.message().contains("Action failed:"));
    }

    #[test]
    fn open_cancel_adds_zero_notifications() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.notifications().is_empty());
        app.handle_action(AppAction::Cancel);
        assert!(app.notifications().is_empty());
        let mut profiles = healthy_control_app(Screen::Profiles);
        profiles.handle_action(AppAction::Activate);
        assert!(profiles.notifications().is_empty());
        profiles.handle_action(AppAction::Cancel);
        assert!(profiles.notifications().is_empty());
    }

    #[test]
    fn notifications_overlay_gates_input_deterministically() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..7 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert!(app.notifications_open());
        app.handle_action(AppAction::GoTo(Screen::Fans));
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
        assert!(app.notifications_open());
        assert_eq!(app.executor.command_calls(), 0);
        app.handle_action(AppAction::TogglePalette);
        assert!(!app.notifications_open());
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..7 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Cancel);
        assert!(!app.notifications_open());
    }

    #[test]
    fn pending_owns_cancel_above_notifications_overlay() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        // Test-constructed overlap: overlay beneath a confirmation.
        app.notifications_open = true;
        // Pending owns Cancel: first Esc discards the confirmation and
        // keeps the notification overlay open.
        app.handle_action(AppAction::Cancel);
        assert!(app.controls().pending().is_none());
        assert!(app.notifications_open);
        assert_eq!(app.executor.command_calls(), 0);
        // Second Esc closes the now-topmost notifications overlay.
        app.handle_action(AppAction::Cancel);
        assert!(!app.notifications_open);
        assert_eq!(app.executor.command_calls(), 0);
    }

    #[test]
    fn pending_command_above_palette_activate_executes_pending() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        // Normal input blocks opening the palette above pending, so
        // construct the stale overlap directly.
        app.palette.open();
        let screen_before = app.state().current_screen();
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        assert_eq!(app.executor.profile_calls(), 0);
        assert!(app.controls().pending().is_none());
        // The palette command did not run: no navigation, palette untouched.
        assert_eq!(app.state().current_screen(), screen_before);
        assert!(app.palette().is_open());
        assert_eq!(app.notifications().len(), 1);
    }

    #[test]
    fn pending_profile_above_palette_activate_executes_pending() {
        let mut app = healthy_control_app(Screen::Profiles);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.palette.open();
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.profile_calls(), 1);
        assert_eq!(app.executor.command_calls(), 0);
        assert!(app.controls().pending().is_none());
        assert!(app.palette().is_open());
        assert_eq!(app.notifications().len(), 1);
    }

    #[test]
    fn pending_above_notifications_activate_executes_pending() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.notifications_open = true;
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        assert!(app.controls().pending().is_none());
        assert!(app.notifications_open);
    }

    #[test]
    fn pending_above_notifications_and_palette_owns_input() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.notifications_open = true;
        app.palette.open();
        let selected_before = app.palette().selected_index();
        // Navigation/row moves never reach lower overlays while pending.
        app.handle_action(AppAction::MoveDown);
        assert_eq!(app.palette().selected_index(), selected_before);
        assert!(app.controls().pending().is_some());
        app.handle_action(AppAction::Activate);
        assert_eq!(app.executor.command_calls(), 1);
        assert!(app.controls().pending().is_none());
        assert!(app.notifications_open);
        assert!(app.palette().is_open());
    }

    #[test]
    fn help_above_pending_notifications_palette_activate_executes_nothing() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        app.notifications_open = true;
        app.palette.open();
        // Pending blocks Help via normal input, so construct the full
        // overlap directly: Help on top of everything.
        app.state_mut().apply(AppAction::ShowHelp);
        assert!(app.state().help_visible());
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        assert!(app.state().help_visible());
        assert!(app.notifications_open);
        assert!(app.palette().is_open());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        // First Esc closes Help only; lower layers stay intact.
        app.handle_action(AppAction::Cancel);
        assert!(!app.state().help_visible());
        assert!(app.controls().pending().is_some());
        assert!(app.notifications_open);
        assert!(app.palette().is_open());
    }

    // ---- Review correction: Help owns input while visible ----

    fn help_visible_control_app(
        screen: Screen,
    ) -> TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor> {
        let mut app = healthy_control_app(screen);
        app.handle_action(AppAction::ShowHelp);
        assert!(app.state().help_visible());
        app
    }

    #[test]
    fn help_visible_activate_starts_nothing_and_calls_nothing() {
        let mut app = help_visible_control_app(Screen::Fans);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        assert!(app.state().help_visible());
    }

    #[test]
    fn help_above_editor_activate_preserves_editor_and_calls_nothing() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        let draft_before = app.controls().editor().unwrap().draft().clone();
        app.handle_action(AppAction::ShowHelp);
        assert!(app.state().help_visible());
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        assert_eq!(app.controls().editor().unwrap().draft(), &draft_before);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn help_above_editor_adjust_keys_leave_draft_unchanged() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        let draft_before = app.controls().editor().unwrap().draft().clone();
        app.handle_action(AppAction::ShowHelp);
        app.handle_action(AppAction::MoveLeft);
        app.handle_action(AppAction::MoveRight);
        assert!(app.controls().is_editing());
        assert_eq!(app.controls().editor().unwrap().draft(), &draft_before);
        assert_eq!(app.executor.command_calls(), 0);
    }

    #[test]
    fn help_above_pending_command_activate_executes_nothing() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        // Pending blocks Help via normal input, so construct the overlap
        // directly: Help on top of an existing confirmation.
        app.state_mut().apply(AppAction::ShowHelp);
        assert!(app.state().help_visible());
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn help_above_pending_profile_activate_executes_nothing() {
        let mut app = healthy_control_app(Screen::Profiles);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        // Pending blocks Help via normal input, so construct the overlap
        // directly: Help on top of an existing confirmation.
        app.state_mut().apply(AppAction::ShowHelp);
        assert!(app.state().help_visible());
        let history_before = app.live().history().len();
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
    }

    #[test]
    fn first_esc_closes_help_but_keeps_pending_then_second_esc_cancels() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        // Pending blocks Help via normal input, so construct the overlap
        // directly: Help on top of an existing confirmation.
        app.state_mut().apply(AppAction::ShowHelp);
        assert!(app.state().help_visible());
        app.handle_action(AppAction::Cancel);
        assert!(!app.state().help_visible());
        assert!(app.controls().pending().is_some());
        assert_eq!(app.executor.command_calls(), 0);
        app.handle_action(AppAction::Cancel);
        assert!(app.controls().pending().is_none());
        assert_eq!(app.executor.command_calls(), 0);
    }

    #[test]
    fn first_esc_closes_help_but_keeps_editor_then_second_esc_cancels() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().is_editing());
        app.handle_action(AppAction::ShowHelp);
        app.handle_action(AppAction::Cancel);
        assert!(!app.state().help_visible());
        assert!(app.controls().is_editing());
        app.handle_action(AppAction::Cancel);
        assert!(!app.controls().is_editing());
        assert_eq!(app.executor.command_calls(), 0);
    }

    #[test]
    fn help_visible_blocks_digit_navigation() {
        let mut app = help_visible_control_app(Screen::Dashboard);
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
            Screen::Profiles,
            Screen::Diagnostics,
            Screen::Dashboard,
        ] {
            app.handle_action(AppAction::GoTo(screen));
            assert_eq!(app.state().current_screen(), Screen::Dashboard);
        }
        assert!(app.state().help_visible());
    }

    #[test]
    fn help_visible_blocks_tab_navigation() {
        let mut app = help_visible_control_app(Screen::Dashboard);
        app.handle_action(AppAction::NextScreen);
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
        app.handle_action(AppAction::PreviousScreen);
        assert_eq!(app.state().current_screen(), Screen::Dashboard);
        assert!(app.state().help_visible());
    }

    #[test]
    fn help_visible_blocks_row_and_candidate_moves() {
        let mut app = healthy_control_app(Screen::Performance);
        let selected_before = app.controls().selected_index(Screen::Performance);
        app.handle_action(AppAction::ShowHelp);
        app.handle_action(AppAction::MoveUp);
        app.handle_action(AppAction::MoveDown);
        app.handle_action(AppAction::MoveLeft);
        app.handle_action(AppAction::MoveRight);
        assert_eq!(app.state().current_screen(), Screen::Performance);
        assert_eq!(
            app.controls().selected_index(Screen::Performance),
            selected_before
        );
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        // Profile selection is frozen too.
        let mut profiles = healthy_control_app(Screen::Profiles);
        let profile_before = profiles.profile_selection().index();
        profiles.handle_action(AppAction::ShowHelp);
        profiles.handle_action(AppAction::MoveUp);
        profiles.handle_action(AppAction::MoveDown);
        assert_eq!(profiles.profile_selection().index(), profile_before);
    }

    #[test]
    fn toggle_help_closes_help() {
        let mut app = help_visible_control_app(Screen::Dashboard);
        app.handle_action(AppAction::ToggleHelp);
        assert!(!app.state().help_visible());
    }

    #[test]
    fn quit_still_requests_quit_while_help_visible() {
        let mut app = help_visible_control_app(Screen::Dashboard);
        app.handle_action(AppAction::Quit);
        assert!(app.state().should_quit());
    }

    // ---- Task 8: named theme switching ----

    #[test]
    fn session_boots_msi_dark_deterministically() {
        let app = healthy_control_app(Screen::Dashboard);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::MsiDark);
        let prepared =
            prepare_tui(fixture_root("gf63"), crate::hardware::LinuxSysfsReader).expect("prepares");
        assert_eq!(prepared.theme_name(), crate::tui::theme::ThemeName::MsiDark);
    }

    #[test]
    fn selecting_each_theme_switches_palette_closed_with_one_notification() {
        use crate::tui::theme::ThemeName;
        let cases = [
            (11, ThemeName::MsiDark, "MSI Dark"),
            (12, ThemeName::Terminal, "Terminal"),
            (13, ThemeName::Light, "Light"),
        ];
        for (steps, name, display) in cases {
            let mut app = healthy_control_app(Screen::Dashboard);
            let history_before = app.live().history().len();
            app.handle_action(AppAction::TogglePalette);
            for _ in 0..steps {
                app.handle_action(AppAction::MoveDown);
            }
            app.handle_action(AppAction::Activate);
            assert_eq!(app.theme_name(), name, "{display}");
            assert_eq!(app.theme(), crate::tui::theme::Theme::for_name(name));
            assert!(!app.palette().is_open());
            assert_eq!(app.executor.command_calls(), 0);
            assert_eq!(app.executor.profile_calls(), 0);
            assert_eq!(app.live().history().len(), history_before);
            assert_eq!(app.notifications().len(), 1);
            let latest = app.notifications().latest().expect("theme notification");
            assert!(
                latest
                    .message()
                    .contains(&format!("Theme changed to {display}"))
            );
        }
    }

    #[test]
    fn reselecting_active_theme_is_harmless() {
        let mut app = healthy_control_app(Screen::Dashboard);
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..11 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        // Selection persists across opens: reopening lands back on the
        // theme row, so Activate reselects it directly.
        assert_eq!(app.palette().selected_index(), 11);
        app.handle_action(AppAction::TogglePalette);
        app.handle_action(AppAction::Activate);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::MsiDark);
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.notifications().len(), 2);
    }

    #[test]
    fn pending_confirmation_blocks_theme_switching() {
        let mut app = healthy_control_app(Screen::Fans);
        app.handle_action(AppAction::Activate);
        app.handle_action(AppAction::Activate);
        assert!(app.controls().pending().is_some());
        let before = app.theme_name();
        app.handle_action(AppAction::TogglePalette);
        assert!(!app.palette().is_open());
        assert_eq!(app.theme_name(), before);
        assert!(app.notifications().is_empty());
    }

    // ---- Task 9: persistent configuration ----

    fn prepared_with_config(
        loaded: Result<crate::config::AppConfig, crate::config::ConfigError>,
        store: Option<crate::config::AppConfigStore>,
    ) -> TuiApp<
        crate::hardware::MsiEcBackend<crate::hardware::LinuxSysfsReader>,
        crate::tui::executor::SafeTuiExecutor,
    > {
        super::prepare_tui_full(
            fixture_root("gf63"),
            crate::hardware::LinuxSysfsReader,
            crate::tui::ProfileCatalog::empty(),
            loaded,
            store,
        )
        .expect("fixture preparation succeeds")
    }

    #[test]
    fn valid_config_chooses_initial_light() {
        let config = crate::config::parse_config_text("theme = \"light\"\n").expect("light parses");
        let app = prepared_with_config(Ok(config), None);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::Light);
        assert_eq!(app.config().theme(), crate::tui::theme::ThemeName::Light);
        assert!(app.notifications().is_empty());
    }

    #[test]
    fn valid_config_chooses_initial_terminal() {
        let config =
            crate::config::parse_config_text("theme = \"terminal\"\n").expect("terminal parses");
        let app = prepared_with_config(Ok(config), None);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::Terminal);
    }

    #[test]
    fn default_config_chooses_msi_dark() {
        let app = prepared_with_config(Ok(crate::config::AppConfig::default()), None);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::MsiDark);
        assert_eq!(
            app.poll_timeout(),
            crate::monitoring::PollInterval::default().as_duration()
        );
    }

    #[test]
    fn configured_refresh_interval_reaches_poll_timeout() {
        use std::time::Duration;
        let config =
            crate::config::parse_config_text("refresh_interval_ms = 2000\n").expect("2000 parses");
        let app = prepared_with_config(Ok(config), None);
        // Production run_tui drives the event loop with this timeout.
        assert_eq!(app.poll_timeout(), Duration::from_secs(2));
        let config =
            crate::config::parse_config_text("refresh_interval_ms = 500\n").expect("500 parses");
        let app = prepared_with_config(Ok(config), None);
        assert_eq!(app.poll_timeout(), Duration::from_millis(500));
    }

    #[test]
    fn configured_vim_setting_reaches_event_source() {
        use crate::tui::CrosstermEventSource;
        let config = crate::config::parse_config_text("vim_keys = false\n").expect("parses");
        let app = prepared_with_config(Ok(config), None);
        assert!(!app.config().vim_keys());
        // Production run_tui builds the source from this flag.
        let source = CrosstermEventSource::with_vim_keys(app.config().vim_keys());
        assert!(!source.vim_keys());
    }

    #[test]
    fn palette_theme_change_persists_in_temp_store() {
        use crate::config::{AppConfigStore, parse_config_text};
        let dir = tempfile::tempdir().expect("config TempDir constructs");
        let store = AppConfigStore::new(dir.path().join("mec").join("config.toml"));
        // Seed refresh/vim settings that must survive the theme save.
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(
            store.path(),
            b"refresh_interval_ms = 2000\ntheme = \"msi-dark\"\nvim_keys = false\n",
        )
        .unwrap();
        let config = store.load().unwrap();
        let mut app = healthy_control_app(Screen::Dashboard);
        app.config = config;
        app.config_store = Some(store);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::MsiDark);
        let history_before = app.live().history().len();
        // Drive the palette to "Theme: Light" (index 13) and activate.
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..13 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::Light);
        assert_eq!(app.config().theme(), crate::tui::theme::ThemeName::Light);
        assert!(!app.palette().is_open());
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("theme notice");
        assert!(latest.message().contains("Theme changed to Light"));
        // The file holds the full normalized current config.
        let text = std::fs::read_to_string(app.config_store.as_ref().unwrap().path()).unwrap();
        assert_eq!(
            text,
            "refresh_interval_ms = 2000\ntheme = \"light\"\nvim_keys = false\n"
        );
        let reloaded = parse_config_text(&text).unwrap();
        assert_eq!(reloaded, app.config().clone());
    }

    #[test]
    fn persistence_failure_keeps_runtime_theme_with_truthful_notice() {
        use crate::config::AppConfigStore;
        let dir = tempfile::tempdir().expect("config TempDir constructs");
        // A regular file where the directory should be: saves fail.
        let blocker = dir.path().join("mec");
        std::fs::write(&blocker, b"not a directory\n").unwrap();
        let store = AppConfigStore::new(blocker.join("config.toml"));
        let mut app = healthy_control_app(Screen::Dashboard);
        app.config_store = Some(store);
        let history_before = app.live().history().len();
        app.handle_action(AppAction::TogglePalette);
        for _ in 0..13 {
            app.handle_action(AppAction::MoveDown);
        }
        app.handle_action(AppAction::Activate);
        // Runtime theme changed despite the failed save.
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::Light);
        assert_eq!(app.executor.command_calls(), 0);
        assert_eq!(app.executor.profile_calls(), 0);
        assert_eq!(app.live().history().len(), history_before);
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("truthful notice");
        assert!(
            latest
                .message()
                .contains("Theme changed to Light for this session")
        );
        assert!(latest.message().contains("config save failed"));
    }

    #[test]
    fn invalid_config_falls_back_to_defaults_nonfatally() {
        let app = prepared_with_config(
            Err(crate::config::ConfigError::Schema("bad = [\n".to_owned())),
            None,
        );
        assert_eq!(app.theme_name(), crate::tui::theme::ThemeName::MsiDark);
        assert_eq!(app.config(), &crate::config::AppConfig::default());
        // One startup notice, path-free, and monitoring still launched.
        assert_eq!(app.notifications().len(), 1);
        let latest = app.notifications().latest().expect("startup notice");
        assert!(latest.message().contains("using defaults"));
        assert!(!latest.message().contains("home"));
        assert!(app.live().device().product_name.contains("GF63"));
    }
}
