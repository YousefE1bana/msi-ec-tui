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
    /// Move to the previous candidate while editing; falls back to
    /// previous-screen navigation when not editing.
    MoveLeft,
    /// Move to the next candidate while editing; falls back to
    /// next-screen navigation when not editing.
    MoveRight,
    /// Begin editing the selected control, or accept the draft into a
    /// pending (data-only, never executed here) command while editing.
    Activate,
    /// Cancel the editor or pending draft; falls back to hiding help.
    Cancel,
    /// Jump directly to a screen.
    GoTo(Screen),
    /// Invert help overlay visibility.
    ToggleHelp,
    /// Show the help overlay.
    ShowHelp,
    /// Hide the help overlay.
    HideHelp,
    /// Toggle the non-mutating command palette overlay.
    TogglePalette,
}
