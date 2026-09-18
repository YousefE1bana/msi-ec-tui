//! Safe Crossterm/Ratatui terminal session.
//!
//! [`TerminalSession::enter`] enables raw mode, enters the alternate
//! screen, and hides the cursor before constructing the Ratatui terminal.
//! Partial-initialization failures clean up before returning the original
//! error so the user's terminal is never left in raw mode. [`Drop`]
//! restores best-effort without panicking; normal flow calls
//! [`TerminalSession::restore`] explicitly so errors stay reportable.

use std::io::Stdout;

use crossterm::cursor::{Hide, Show};
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

/// Owns the Crossterm/Ratatui terminal from entry to restoration.
pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    lifecycle: Lifecycle,
}

impl TerminalSession {
    /// Enters raw mode, the alternate screen, and a hidden cursor, then
    /// constructs the Ratatui terminal. Cleans up best-effort and returns
    /// the original error when a later step fails.
    pub fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(std::io::stdout(), EnterAlternateScreen, Hide) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(std::io::stdout())) {
            Ok(terminal) => Ok(Self {
                terminal,
                lifecycle: Lifecycle::default(),
            }),
            Err(error) => {
                let _ = execute!(std::io::stdout(), Show, LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }

    /// Borrows the Ratatui terminal for rendering.
    pub fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    /// Shows the cursor, leaves the alternate screen, and disables raw
    /// mode. Safe to call more than once; later calls return `Ok(())`
    /// without repeating transitions.
    pub fn restore(&mut self) -> std::io::Result<()> {
        if !self.lifecycle.restore() {
            return Ok(());
        }
        let mut stdout = std::io::stdout();
        let mut failure: Option<std::io::Error> = None;
        for step in [
            execute!(stdout, Show),
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
    use super::Lifecycle;

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
}
