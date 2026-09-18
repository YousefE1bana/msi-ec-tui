//! Default semantic palette for the read-only TUI.
//!
//! Exactly one palette: semantic roles with terminal-safe colors. Named
//! product themes belong to a later plan.

use ratatui::style::Color;

/// Semantic color roles consumed by screens, chrome, and overlays.
///
/// Roles name meaning, never widgets: READY/LIVE resolve to `success`,
/// READ-ONLY to `warning`, DEGRADED to `danger`, waiting states to `muted`,
/// and the active navigation entry to `primary` plus bold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Default terminal background.
    pub background: Color,
    /// Default terminal foreground.
    pub foreground: Color,
    /// Active navigation entry, screen titles, help chrome.
    pub primary: Color,
    /// Secondary informational labels.
    pub secondary: Color,
    /// Healthy states: READY, LIVE, Supported.
    pub success: Color,
    /// Degraded-but-usable states: READ-ONLY.
    pub warning: Color,
    /// Failed states: DEGRADED telemetry.
    pub danger: Color,
    /// Inactive navigation, waiting states, unavailable capabilities.
    pub muted: Color,
    /// Panel and shell borders.
    pub border: Color,
}

impl Default for Theme {
    /// One conservative ANSI-style palette shared by every screen.
    fn default() -> Self {
        Self {
            background: Color::Reset,
            foreground: Color::Reset,
            primary: Color::Cyan,
            secondary: Color::Blue,
            success: Color::Green,
            warning: Color::Yellow,
            danger: Color::Red,
            muted: Color::Gray,
            border: Color::DarkGray,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn exposes_all_nine_semantic_roles() {
        let theme = Theme::default();
        let _ = theme.background;
        let _ = theme.foreground;
        let _ = theme.primary;
        let _ = theme.secondary;
        let _ = theme.success;
        let _ = theme.warning;
        let _ = theme.danger;
        let _ = theme.muted;
        let _ = theme.border;
    }

    #[test]
    fn default_is_deterministic() {
        assert_eq!(Theme::default(), Theme::default());
    }

    #[test]
    fn status_roles_are_distinct() {
        let theme = Theme::default();
        assert_ne!(theme.success, theme.warning);
        assert_ne!(theme.warning, theme.danger);
        assert_ne!(theme.success, theme.danger);
    }
}
