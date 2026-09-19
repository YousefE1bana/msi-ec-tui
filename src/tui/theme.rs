//! Semantic palettes for the TUI: one stable identity per named theme.
//!
//! Screens consume semantic roles only and never branch on identity.
//! [`Theme::default()`] maps exactly to [`ThemeName::Terminal`]. The
//! interactive session boots [`ThemeName::MsiDark`] until config
//! persistence lands; that choice lives in TUI preparation, not here.

use ratatui::style::{Color, Style};

/// Stable identity for the three approved named themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeName {
    /// Dark background with an MSI-inspired red primary accent.
    MsiDark,
    /// Terminal defaults with conservative ANSI accents.
    Terminal,
    /// Light background with a non-red primary accent.
    Light,
}

impl ThemeName {
    /// All themes in stable order.
    pub const fn all() -> [ThemeName; 3] {
        [ThemeName::MsiDark, ThemeName::Terminal, ThemeName::Light]
    }

    /// Exact user-facing name.
    pub const fn display_name(self) -> &'static str {
        match self {
            ThemeName::MsiDark => "MSI Dark",
            ThemeName::Terminal => "Terminal",
            ThemeName::Light => "Light",
        }
    }
}

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
    /// Conservative ANSI-style palette. Identical to the Terminal theme:
    /// `Theme::default()` maps exactly to [`ThemeName::Terminal`].
    fn default() -> Self {
        Self::for_name(ThemeName::Terminal)
    }
}

impl Theme {
    /// Pure palette lookup: deterministic per identity, no I/O.
    pub const fn for_name(name: ThemeName) -> Self {
        match name {
            ThemeName::MsiDark => Self {
                background: Color::Black,
                foreground: Color::White,
                primary: Color::Red,
                secondary: Color::Magenta,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::LightRed,
                muted: Color::Gray,
                border: Color::DarkGray,
            },
            ThemeName::Terminal => Self {
                background: Color::Reset,
                foreground: Color::Reset,
                primary: Color::Cyan,
                secondary: Color::Blue,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::Red,
                muted: Color::Gray,
                border: Color::DarkGray,
            },
            ThemeName::Light => Self {
                background: Color::White,
                foreground: Color::Black,
                primary: Color::Blue,
                secondary: Color::Magenta,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::Red,
                muted: Color::DarkGray,
                border: Color::Gray,
            },
        }
    }

    /// Base surface style for the full TUI frame and overlay interiors.
    ///
    /// Pure semantic helper: foreground resolves ordinary unstyled text to
    /// `theme.foreground` while background paints the surface. Semantic
    /// foreground roles (primary/success/warning/...) override the
    /// foreground on top of this and inherit the background from the
    /// surrounding widget style. No I/O, no branching on screens.
    pub fn base_style(&self) -> Style {
        Style::default().fg(self.foreground).bg(self.background)
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

    #[test]
    fn theme_name_has_exactly_three_stable_names() {
        use super::ThemeName;
        assert_eq!(ThemeName::all().len(), 3);
        let names: Vec<&str> = ThemeName::all()
            .iter()
            .map(|name| name.display_name())
            .collect();
        assert_eq!(names, vec!["MSI Dark", "Terminal", "Light"]);
    }

    #[test]
    fn every_theme_provides_all_nine_roles() {
        use super::ThemeName;
        for name in ThemeName::all() {
            let theme = Theme::for_name(name);
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
    }

    #[test]
    fn named_themes_are_deterministic() {
        use super::ThemeName;
        for name in ThemeName::all() {
            assert_eq!(Theme::for_name(name), Theme::for_name(name));
        }
    }

    #[test]
    fn default_maps_to_terminal() {
        use super::ThemeName;
        assert_eq!(Theme::default(), Theme::for_name(ThemeName::Terminal));
    }

    #[test]
    fn themes_differ_in_primary_background_foreground() {
        use super::ThemeName;
        let dark = Theme::for_name(ThemeName::MsiDark);
        let terminal = Theme::for_name(ThemeName::Terminal);
        let light = Theme::for_name(ThemeName::Light);
        assert_ne!(dark.primary, terminal.primary);
        assert_ne!(terminal.primary, light.primary);
        assert_ne!(dark.primary, light.primary);
        assert_ne!(dark.background, terminal.background);
        assert_ne!(terminal.background, light.background);
        assert_ne!(dark.foreground, terminal.foreground);
        assert_ne!(terminal.foreground, light.foreground);
    }

    #[test]
    fn status_roles_stay_distinguishable_in_every_theme() {
        use super::ThemeName;
        for name in ThemeName::all() {
            let theme = Theme::for_name(name);
            assert_ne!(theme.success, theme.warning, "{name:?}");
            assert_ne!(theme.warning, theme.danger, "{name:?}");
            assert_ne!(theme.success, theme.danger, "{name:?}");
        }
    }

    #[test]
    fn msi_dark_uses_red_primary_on_dark_background() {
        use super::{Theme, ThemeName};
        use ratatui::style::Color;
        let theme = Theme::for_name(ThemeName::MsiDark);
        assert_eq!(theme.primary, Color::Red);
        assert_eq!(theme.background, Color::Black);
    }

    #[test]
    fn base_style_carries_foreground_and_background() {
        use super::ThemeName;
        use ratatui::style::Color;
        for name in ThemeName::all() {
            let theme = Theme::for_name(name);
            let style = theme.base_style();
            assert_eq!(style.fg, Some(theme.foreground), "{name:?}");
            assert_eq!(style.bg, Some(theme.background), "{name:?}");
        }
        assert_eq!(Theme::default().base_style().bg, Some(Color::Reset));
    }
}
