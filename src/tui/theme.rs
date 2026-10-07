//! Semantic palettes for the TUI: one stable identity per named theme.
//!
//! Screens consume semantic roles only and never branch on identity.
//! [`Theme::default()`] maps exactly to [`ThemeName::Terminal`]. The
//! interactive session boots [`ThemeName::MsiDark`] unless the user
//! configuration selects another theme; that choice lives in TUI
//! preparation via [`crate::config::AppConfig`], not here.

use ratatui::style::{Color, Style};

/// Stable identity for the existing themes and two optional candidates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeName {
    /// Dark background with an MSI-inspired red primary accent.
    MsiDark,
    /// Terminal defaults with conservative ANSI accents.
    Terminal,
    /// Light background with a non-red primary accent.
    Light,
    /// Navy surfaces with ice-cyan focus. Optional candidate.
    Arctic,
    /// Graphite surfaces with violet focus. Optional candidate.
    Graphite,
}

impl ThemeName {
    /// All themes in stable order.
    pub const fn all() -> [ThemeName; 5] {
        [
            ThemeName::MsiDark,
            ThemeName::Terminal,
            ThemeName::Light,
            ThemeName::Arctic,
            ThemeName::Graphite,
        ]
    }

    /// Exact user-facing name.
    pub const fn display_name(self) -> &'static str {
        match self {
            ThemeName::MsiDark => "MSI Dark",
            ThemeName::Terminal => "Terminal",
            ThemeName::Light => "Light",
            ThemeName::Arctic => "Arctic Midnight",
            ThemeName::Graphite => "Graphite Violet",
        }
    }

    /// Canonical persistent config slug. Never a display string: the
    /// user-facing name and the on-disk value are separate concepts.
    pub const fn config_name(self) -> &'static str {
        match self {
            ThemeName::MsiDark => "msi-dark",
            ThemeName::Terminal => "terminal",
            ThemeName::Light => "light",
            ThemeName::Arctic => "arctic",
            ThemeName::Graphite => "graphite",
        }
    }

    /// Parses a config slug. Accepts the canonical values plus the
    /// `default` alias (design-spec compatibility), which maps to the
    /// interactive product default MSI Dark. Anything else is rejected.
    pub fn from_config_name(value: &str) -> Option<Self> {
        match value {
            "msi-dark" | "default" => Some(ThemeName::MsiDark),
            "terminal" => Some(ThemeName::Terminal),
            "light" => Some(ThemeName::Light),
            "arctic" => Some(ThemeName::Arctic),
            "graphite" => Some(ThemeName::Graphite),
            _ => None,
        }
    }
}

/// Semantic color roles consumed by screens, chrome, and overlays.
///
/// Roles name meaning, never widgets: READY/LIVE resolve to `success`,
/// READ-ONLY to `warning`, DEGRADED to `danger`, waiting states to `muted`,
/// and the active navigation entry to `primary` plus bold. Dashboard card
/// titles, telemetry headings, and focus accents resolve to `accent`;
/// card interiors resolve to `surface`; telemetry meters resolve to
/// `meter_fill` on `meter_track`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Default terminal background.
    pub background: Color,
    /// Card interior surface. Dark themes separate cards from the root;
    /// reset/background themes let borders define cards.
    pub surface: Color,
    /// Default terminal foreground.
    pub foreground: Color,
    /// Active navigation entry, screen titles, help chrome.
    pub primary: Color,
    /// Dashboard card titles, telemetry headings, focus accents.
    pub accent: Color,
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
    /// Telemetry meter fill (approved cool cyan, all themes).
    pub meter_fill: Color,
    /// Telemetry meter track (approved dark slate, all themes).
    pub meter_track: Color,
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
                surface: Color::Rgb(17, 19, 24),
                foreground: Color::White,
                primary: Color::Red,
                accent: Color::Rgb(94, 197, 222),
                secondary: Color::Magenta,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::LightRed,
                muted: Color::Gray,
                border: Color::DarkGray,
                meter_fill: Color::Rgb(79, 159, 173),
                meter_track: Color::Rgb(34, 50, 56),
            },
            ThemeName::Arctic => Self {
                background: Color::Rgb(8, 15, 25),
                surface: Color::Rgb(17, 29, 43),
                foreground: Color::Rgb(224, 235, 244),
                primary: Color::Rgb(117, 210, 234),
                accent: Color::Rgb(117, 210, 234),
                secondary: Color::Rgb(145, 170, 212),
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::LightRed,
                muted: Color::Rgb(144, 159, 177),
                border: Color::Rgb(63, 79, 97),
                meter_fill: Color::Rgb(79, 159, 173),
                meter_track: Color::Rgb(34, 50, 56),
            },
            ThemeName::Graphite => Self {
                background: Color::Rgb(13, 13, 17),
                surface: Color::Rgb(24, 23, 30),
                foreground: Color::Rgb(232, 228, 239),
                primary: Color::Rgb(183, 160, 240),
                accent: Color::Rgb(183, 160, 240),
                secondary: Color::Rgb(142, 166, 199),
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::LightRed,
                muted: Color::Rgb(155, 150, 165),
                border: Color::Rgb(73, 69, 83),
                meter_fill: Color::Rgb(79, 159, 173),
                meter_track: Color::Rgb(34, 50, 56),
            },
            ThemeName::Terminal => Self {
                background: Color::Reset,
                surface: Color::Reset,
                foreground: Color::Reset,
                primary: Color::Cyan,
                accent: Color::Cyan,
                secondary: Color::Blue,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::Red,
                muted: Color::Gray,
                border: Color::DarkGray,
                meter_fill: Color::Rgb(79, 159, 173),
                meter_track: Color::Rgb(34, 50, 56),
            },
            ThemeName::Light => Self {
                background: Color::White,
                surface: Color::White,
                foreground: Color::Black,
                primary: Color::Blue,
                accent: Color::Blue,
                secondary: Color::Magenta,
                success: Color::Green,
                warning: Color::Yellow,
                danger: Color::Red,
                muted: Color::DarkGray,
                border: Color::Gray,
                meter_fill: Color::Rgb(79, 159, 173),
                meter_track: Color::Rgb(34, 50, 56),
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
    fn dashboard_roles_cover_surface_accent_and_meters() {
        use ratatui::style::Color;
        for name in super::ThemeName::all() {
            let theme = Theme::for_name(name);
            // Meter colors are the approved locked pair in every theme.
            assert_eq!(theme.meter_fill, Color::Rgb(79, 159, 173), "{name:?}");
            assert_eq!(theme.meter_track, Color::Rgb(34, 50, 56), "{name:?}");
            let _ = theme.surface;
            let _ = theme.accent;
        }
        // The dark session separates cards from the root background.
        assert_ne!(
            Theme::for_name(super::ThemeName::MsiDark).surface,
            Theme::for_name(super::ThemeName::MsiDark).background
        );
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
    fn theme_names_preserve_existing_order_and_append_candidates() {
        use super::ThemeName;
        assert_eq!(ThemeName::all().len(), 5);
        let names: Vec<&str> = ThemeName::all()
            .iter()
            .map(|name| name.display_name())
            .collect();
        assert_eq!(
            names,
            vec![
                "MSI Dark",
                "Terminal",
                "Light",
                "Arctic Midnight",
                "Graphite Violet"
            ]
        );
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
    fn theme_config_names_are_stable_slugs() {
        use super::ThemeName;
        assert_eq!(ThemeName::MsiDark.config_name(), "msi-dark");
        assert_eq!(ThemeName::Terminal.config_name(), "terminal");
        assert_eq!(ThemeName::Light.config_name(), "light");
        assert_eq!(
            ThemeName::from_config_name("msi-dark"),
            Some(ThemeName::MsiDark)
        );
        assert_eq!(
            ThemeName::from_config_name("terminal"),
            Some(ThemeName::Terminal)
        );
        assert_eq!(ThemeName::from_config_name("light"), Some(ThemeName::Light));
        // `default` is the design-spec compatibility alias for MSI Dark.
        assert_eq!(
            ThemeName::from_config_name("default"),
            Some(ThemeName::MsiDark)
        );
        // Display strings are never valid config values.
        assert_eq!(ThemeName::from_config_name("MSI Dark"), None);
        assert_eq!(ThemeName::from_config_name("Terminal "), None);
        assert_eq!(ThemeName::from_config_name(""), None);
        assert_eq!(ThemeName::from_config_name("dark"), None);
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
