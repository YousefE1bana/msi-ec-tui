//! Safe Crossterm/Ratatui terminal session.
//!
//! [`TerminalSession::enter`] enables raw mode, enters the alternate
//! screen, enables mouse capture, and hides the cursor before constructing
//! the Ratatui terminal. Partial-initialization failures clean up before
//! returning the original error so the user's terminal is never left in
//! raw mode or mouse-capture mode. [`Drop`] restores best-effort without
//! panicking; normal flow calls [`TerminalSession::restore`] explicitly so
//! errors stay reportable.

use std::io::Stdout;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Tracks whether terminal restoration already ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Lifecycle {
    /// The terminal session owns raw mode and the alternate screen.
    #[default]
    Active,
    /// Restoration already ran; further restores are no-ops.
    Restored,
}

impl Lifecycle {
    /// Moves to restored on first call and reports whether restoration
    /// work should run now.
    fn restore(&mut self) -> bool {
        match self {
            Lifecycle::Active => {
                *self = Lifecycle::Restored;
                true
            }
            Lifecycle::Restored => false,
        }
    }
}

/// Setup progress reached before [`TerminalSession::enter`] failed.
///
/// Keeps alternate-screen entry, mouse-capture enable, and cursor hiding
/// as distinct stages so each failure cleans up exactly what it reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnterStage {
    /// Raw mode enabled; alternate screen not yet entered.
    Raw,
    /// Alternate screen entered; mouse capture not yet enabled.
    Alternate,
    /// Mouse capture enabled; cursor not yet hidden.
    MouseCapture,
    /// Cursor hidden; terminal construction pending.
    CursorHidden,
}

/// One best-effort restoration operation for partial `enter` cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnterCleanup {
    ShowCursor,
    DisableMouseCapture,
    LeaveAlternateScreen,
    DisableRawMode,
}

/// Cleanup required when `enter` fails after reaching `completed`.
///
/// Pure so the staged contract stays testable without touching a real
/// terminal. Cleanup failures never replace the original setup error.
fn cleanup_after_enter_failure(completed: EnterStage) -> &'static [EnterCleanup] {
    match completed {
        EnterStage::Raw => &[EnterCleanup::DisableRawMode],
        EnterStage::Alternate => &[
            EnterCleanup::LeaveAlternateScreen,
            EnterCleanup::DisableRawMode,
        ],
        EnterStage::MouseCapture | EnterStage::CursorHidden => &[
            EnterCleanup::ShowCursor,
            EnterCleanup::DisableMouseCapture,
            EnterCleanup::LeaveAlternateScreen,
            EnterCleanup::DisableRawMode,
        ],
    }
}

/// Runs the staged partial-`enter` cleanup best-effort.
fn cleanup_partial_enter(completed: EnterStage) {
    let mut stdout = std::io::stdout();
    for step in cleanup_after_enter_failure(completed) {
        match step {
            EnterCleanup::ShowCursor => {
                let _ = execute!(stdout, Show);
            }
            EnterCleanup::DisableMouseCapture => {
                let _ = execute!(stdout, DisableMouseCapture);
            }
            EnterCleanup::LeaveAlternateScreen => {
                let _ = execute!(stdout, LeaveAlternateScreen);
            }
            EnterCleanup::DisableRawMode => {
                let _ = disable_raw_mode();
            }
        }
    }
}

/// Owns the Crossterm/Ratatui terminal from entry to restoration.
pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    lifecycle: Lifecycle,
}

impl TerminalSession {
    /// Enters raw mode, the alternate screen, mouse capture, and a hidden
    /// cursor, then constructs the Ratatui terminal. Each stage cleans up
    /// on failure and returns the original error.
    pub fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(std::io::stdout(), EnterAlternateScreen) {
            cleanup_partial_enter(EnterStage::Raw);
            return Err(error);
        }
        if let Err(error) = execute!(std::io::stdout(), EnableMouseCapture) {
            cleanup_partial_enter(EnterStage::Alternate);
            return Err(error);
        }
        if let Err(error) = execute!(std::io::stdout(), Hide) {
            cleanup_partial_enter(EnterStage::MouseCapture);
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
            Ok(terminal) => Ok(Self {
                terminal,
                lifecycle: Lifecycle::default(),
            }),
            Err(error) => {
                cleanup_partial_enter(EnterStage::CursorHidden);
                Err(error)
            }
        }
    }

    /// Borrows the Ratatui terminal for rendering.
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    /// Shows the cursor, disables mouse capture, leaves the alternate
    /// screen, and disables raw mode. Safe to call more than once; later
    /// calls return `Ok(())` without repeating transitions.
    pub fn restore(&mut self) -> std::io::Result<()> {
        if !self.lifecycle.restore() {
            return Ok(());
        }
        let mut stdout = std::io::stdout();
        let mut failure: Option<std::io::Error> = None;
        for step in [
            execute!(stdout, Show),
            execute!(stdout, DisableMouseCapture),
            execute!(stdout, LeaveAlternateScreen),
            disable_raw_mode(),
        ] {
            if let Err(error) = step {
                failure = failure.or(Some(error));
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for TerminalSession {
    /// Best-effort restoration safety net for early-return and unwinding
    /// paths. Never panics; explicit [`TerminalSession::restore`] remains
    /// the reporting path for normal flow.
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::{EnterCleanup, EnterStage, Lifecycle, cleanup_after_enter_failure};

    #[test]
    fn first_restore_transition_runs_once() {
        let mut lifecycle = Lifecycle::default();
        assert!(lifecycle.restore());
    }

    #[test]
    fn second_restore_is_a_noop() {
        let mut lifecycle = Lifecycle::default();
        assert!(lifecycle.restore());
        assert!(!lifecycle.restore());
    }

    #[test]
    fn alternate_entry_failure_cleans_only_raw_mode() {
        assert_eq!(
            cleanup_after_enter_failure(EnterStage::Raw),
            &[EnterCleanup::DisableRawMode]
        );
    }

    #[test]
    fn mouse_capture_failure_leaves_screen_and_raw() {
        // Mouse enablement fails after the alternate screen: cursor work
        // never ran, so only screen + raw mode clean up.
        assert_eq!(
            cleanup_after_enter_failure(EnterStage::Alternate),
            &[
                EnterCleanup::LeaveAlternateScreen,
                EnterCleanup::DisableRawMode,
            ]
        );
    }

    #[test]
    fn hide_failure_cleans_cursor_mouse_screen_and_raw() {
        assert_eq!(
            cleanup_after_enter_failure(EnterStage::MouseCapture),
            &[
                EnterCleanup::ShowCursor,
                EnterCleanup::DisableMouseCapture,
                EnterCleanup::LeaveAlternateScreen,
                EnterCleanup::DisableRawMode,
            ]
        );
    }

    #[test]
    fn terminal_construction_failure_runs_full_cleanup() {
        assert_eq!(
            cleanup_after_enter_failure(EnterStage::CursorHidden),
            &[
                EnterCleanup::ShowCursor,
                EnterCleanup::DisableMouseCapture,
                EnterCleanup::LeaveAlternateScreen,
                EnterCleanup::DisableRawMode,
            ]
        );
    }

    #[test]
    fn enter_stages_stay_distinct() {
        assert_ne!(EnterStage::Raw, EnterStage::Alternate);
        assert_ne!(EnterStage::Alternate, EnterStage::MouseCapture);
        assert_ne!(EnterStage::MouseCapture, EnterStage::CursorHidden);
        assert!(
            !cleanup_after_enter_failure(EnterStage::Raw)
                .contains(&EnterCleanup::LeaveAlternateScreen),
            "raw-only failure must not leave the alternate screen"
        );
        assert!(
            !cleanup_after_enter_failure(EnterStage::Alternate)
                .contains(&EnterCleanup::DisableMouseCapture),
            "mouse never engaged, so nothing disables it"
        );
        assert!(
            cleanup_after_enter_failure(EnterStage::MouseCapture)
                .contains(&EnterCleanup::DisableMouseCapture),
            "cursor-hide failure must release mouse capture"
        );
    }
}
