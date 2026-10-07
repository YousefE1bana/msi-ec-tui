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
    /// Begin editing, review a draft/profile, or explicitly confirm an
    /// existing pending mutation. TuiApp resolves the current context;
    /// keyboard Enter and the visible Review/Apply buttons share this intent.
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
    /// Focus a card/panel by index on the current screen. Presentation
    /// only: updates the focused-card highlight, never touches hardware
    /// or selection. Produced by mouse clicks; keyboard has no binding.
    FocusCard(usize),
    /// Select an interactive control row by index on the current screen.
    /// Enters the same selection state as keyboard row movement without
    /// starting an edit. Produced by mouse clicks; dropped while an
    /// editor, confirmation, or overlay owns input.
    SelectControlRow(usize),
    /// Select a visible control value and enter the existing editor.
    /// Never confirms or executes; ignored while an editor/modal owns input.
    EditControlRow(usize),
    /// Select a profile row by index on the Profiles screen. Enters the
    /// same selection state as keyboard row movement without staging or
    /// applying anything. Produced by mouse clicks.
    SelectProfileRow(usize),
    /// Jump the command palette selection to a row and activate it
    /// through the existing palette path (navigation, overlays, themes,
    /// quit only: never a hardware mutation). Produced by mouse clicks
    /// while the palette is open.
    ActivatePaletteRow(usize),
}
