//! Application-level actions: terminal-independent intents.
//!
//! The state layer never sees terminal events. Later tasks map Crossterm
//! key events onto these actions.

use super::state::Screen;

/// Intent applied to [`super::AppState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// Request application exit.
    Quit,
    /// Advance to the next screen in canonical order.
    NextScreen,
    /// Move to the previous screen in canonical order.
    PreviousScreen,
    /// Move to the previous row on row-driven screens; falls back to
    /// previous-screen navigation elsewhere.
    MoveUp,
    /// Move to the next row on row-driven screens; falls back to
    /// next-screen navigation elsewhere.
    MoveDown,
    /// Jump directly to a screen.
    GoTo(Screen),
    /// Invert help overlay visibility.
    ToggleHelp,
    /// Show the help overlay.
    ShowHelp,
    /// Hide the help overlay.
    HideHelp,
}
