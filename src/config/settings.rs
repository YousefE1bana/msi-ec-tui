//! Typed TUI user configuration: domain model plus strict TOML parsing.
//!
//! [`AppConfig`] is the single normalized model: refresh interval as a
//! validated [`PollInterval`], theme as a [`ThemeName`], vim keys as a
//! boolean. Raw TOML values never leave this module. The normalized schema
//! is exactly:
//!
//! ```toml
//! refresh_interval_ms = 1000
//! theme = "msi-dark"
//! vim_keys = true
//! ```
//!
//! Config is data only: no environment expansion, no shell syntax, no
//! command substitution, no path interpretation. Unknown fields (including
//! the design-example `mouse` key, which MEC does not implement) are
//! rejected rather than silently accepted.

use serde::Deserialize;

use crate::monitoring::PollInterval;
use crate::tui::theme::ThemeName;

use super::ConfigError;

/// Maximum accepted config file size: 64 KiB.
pub const MAX_CONFIG_SIZE: usize = 64 * 1024;

/// Typed TUI settings. Missing-file defaults: 1s polling, MSI Dark,
/// vim keys on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    refresh_interval: PollInterval,
    theme: ThemeName,
    vim_keys: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            refresh_interval: PollInterval::Sec1,
            theme: ThemeName::MsiDark,
            vim_keys: true,
        }
    }
}

impl AppConfig {
    /// Validated poll interval driving the TUI event-loop timeout.
    pub fn refresh_interval(&self) -> PollInterval {
        self.refresh_interval
    }

    /// Canonical millisecond count for the configured interval.
    pub fn refresh_interval_ms(&self) -> u64 {
        match self.refresh_interval {
            PollInterval::Ms500 => 500,
            PollInterval::Sec1 => 1000,
            PollInterval::Sec2 => 2000,
            PollInterval::Sec5 => 5000,
        }
    }

    /// Initial TUI theme identity.
    pub fn theme(&self) -> ThemeName {
        self.theme
    }

    /// Whether h/j/k/l navigate (arrow keys always work).
    pub fn vim_keys(&self) -> bool {
        self.vim_keys
    }

    /// Updates the theme in memory. Callers persist via the store.
    pub fn set_theme(&mut self, theme: ThemeName) {
        self.theme = theme;
    }

    /// Serializes the full normalized document: every supported key,
    /// canonical slugs, stable key order, trailing newline.
    pub fn to_toml_string(&self) -> String {
        format!(
            "refresh_interval_ms = {}\ntheme = \"{}\"\nvim_keys = {}\n",
            self.refresh_interval_ms(),
            self.theme.config_name(),
            self.vim_keys,
        )
    }
}

fn default_refresh_ms() -> u64 {
    1000
}

fn default_theme_slug() -> String {
    "msi-dark".to_owned()
}

fn default_vim_keys() -> bool {
    true
}

/// Raw TOML representation. Unknown fields are rejected outright, so a
/// `mouse` key (or any typo) fails loudly instead of pretending support.
/// Each key falls back to its default when absent; wrong types fail.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default = "default_refresh_ms")]
    refresh_interval_ms: u64,
    #[serde(default = "default_theme_slug")]
    theme: String,
    #[serde(default = "default_vim_keys")]
    vim_keys: bool,
}

/// Parses one config document with strict validation. Never panics, never
/// touches the filesystem, never executes anything resembling shell.
pub fn parse_config_text(text: &str) -> Result<AppConfig, ConfigError> {
    if text.len() > MAX_CONFIG_SIZE {
        return Err(ConfigError::TooLarge {
            max: MAX_CONFIG_SIZE,
        });
    }
    if text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(ConfigError::ControlCharacters);
    }
    let raw: RawConfig =
        toml::from_str(text).map_err(|error| ConfigError::Schema(error.to_string()))?;
    let refresh_interval = PollInterval::from_millis(raw.refresh_interval_ms)
        .map_err(|_| ConfigError::UnsupportedInterval(raw.refresh_interval_ms))?;
    let theme = ThemeName::from_config_name(&raw.theme)
        .ok_or_else(|| ConfigError::InvalidTheme(raw.theme.clone()))?;
    Ok(AppConfig {
        refresh_interval,
        theme,
        vim_keys: raw.vim_keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_exact() {
        let config = AppConfig::default();
        assert_eq!(config.refresh_interval(), PollInterval::Sec1);
        assert_eq!(config.refresh_interval_ms(), 1000);
        assert_eq!(config.theme(), ThemeName::MsiDark);
        assert!(config.vim_keys());
    }

    #[test]
    fn supported_intervals_map_through_poll_interval() {
        for (millis, interval) in [
            (500, PollInterval::Ms500),
            (1000, PollInterval::Sec1),
            (2000, PollInterval::Sec2),
            (5000, PollInterval::Sec5),
        ] {
            let text = format!("refresh_interval_ms = {millis}\n");
            let config = parse_config_text(&text).unwrap();
            assert_eq!(config.refresh_interval(), interval);
            assert_eq!(config.refresh_interval_ms(), millis);
        }
    }

    #[test]
    fn other_intervals_rejected() {
        for millis in [0, 1, 100, 999, 1500, 2500, 3000, 10000, 60000] {
            let text = format!("refresh_interval_ms = {millis}\n");
            assert!(
                matches!(
                    parse_config_text(&text),
                    Err(ConfigError::UnsupportedInterval(_))
                ),
                "{millis}"
            );
        }
    }

    #[test]
    fn theme_slugs_parse() {
        for (slug, theme) in [
            ("msi-dark", ThemeName::MsiDark),
            ("terminal", ThemeName::Terminal),
            ("light", ThemeName::Light),
        ] {
            let config = parse_config_text(&format!("theme = \"{slug}\"\n")).unwrap();
            assert_eq!(config.theme(), theme);
        }
    }

    #[test]
    fn default_alias_maps_to_msi_dark() {
        let config = parse_config_text("theme = \"default\"\n").unwrap();
        assert_eq!(config.theme(), ThemeName::MsiDark);
    }

    #[test]
    fn invalid_theme_rejected() {
        for slug in ["MSI Dark", "dark", "LIGHT", "solarized", "", "msi_dark"] {
            let text = format!("theme = \"{slug}\"\n");
            assert!(
                matches!(parse_config_text(&text), Err(ConfigError::InvalidTheme(_))),
                "{slug:?}"
            );
        }
    }

    #[test]
    fn unknown_key_rejected() {
        let config =
            "refresh_interval_ms = 1000\ntheme = \"msi-dark\"\nvim_keys = true\npoll = true\n";
        assert!(matches!(
            parse_config_text(config),
            Err(ConfigError::Schema(_))
        ));
    }

    #[test]
    fn mouse_key_rejected_rather_than_silently_accepted() {
        // The design example shows `mouse = true`, but MEC implements no
        // mouse control: claiming support would be dishonest.
        let config =
            "refresh_interval_ms = 1000\ntheme = \"msi-dark\"\nvim_keys = true\nmouse = true\n";
        assert!(matches!(
            parse_config_text(config),
            Err(ConfigError::Schema(_))
        ));
    }

    #[test]
    fn wrong_types_rejected() {
        for text in [
            "refresh_interval_ms = \"1000\"\n",
            "refresh_interval_ms = 1.5\n",
            "theme = 42\n",
            "vim_keys = \"yes\"\n",
            "vim_keys = 1\n",
        ] {
            assert!(parse_config_text(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn shell_looking_strings_are_never_executed() {
        let text = "theme = \"$(rm -rf ~)\"\n";
        assert!(matches!(
            parse_config_text(text),
            Err(ConfigError::InvalidTheme(_))
        ));
        let text = "refresh_interval_ms = 1000\ntheme = \"`id`\"\nvim_keys = true\n";
        assert!(parse_config_text(text).is_err());
    }

    #[test]
    fn malformed_toml_rejected() {
        for text in ["refresh_interval_ms = [unclosed\n", "===\n", "theme"] {
            assert!(
                matches!(parse_config_text(text), Err(ConfigError::Schema(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn control_characters_rejected() {
        assert!(matches!(
            parse_config_text("theme = \"msi-dark\"\n\x07\n"),
            Err(ConfigError::ControlCharacters)
        ));
    }

    #[test]
    fn oversized_document_rejected() {
        let mut text = String::from("refresh_interval_ms = 1000\n# ");
        while text.len() <= MAX_CONFIG_SIZE {
            text.push('x');
        }
        assert!(matches!(
            parse_config_text(&text),
            Err(ConfigError::TooLarge { .. })
        ));
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let config = parse_config_text("").unwrap();
        assert_eq!(config, AppConfig::default());
        let config = parse_config_text("vim_keys = false\n").unwrap();
        assert_eq!(config.refresh_interval(), PollInterval::Sec1);
        assert_eq!(config.theme(), ThemeName::MsiDark);
        assert!(!config.vim_keys());
    }

    #[test]
    fn normalized_serialization_round_trips_exactly() {
        let mut config = AppConfig::default();
        config.set_theme(ThemeName::Light);
        let text = config.to_toml_string();
        assert_eq!(
            text,
            "refresh_interval_ms = 1000\ntheme = \"light\"\nvim_keys = true\n"
        );
        assert_eq!(parse_config_text(&text).unwrap(), config);
    }

    #[test]
    fn error_display_is_stable_and_path_free() {
        assert_eq!(
            ConfigError::TooLarge {
                max: MAX_CONFIG_SIZE
            }
            .to_string(),
            format!("config exceeds size limit of {MAX_CONFIG_SIZE} bytes")
        );
        // Rejected values are never echoed: notifications stay path-free.
        assert_eq!(
            ConfigError::InvalidTheme("light\n/home/x".to_owned()).to_string(),
            "invalid config theme value"
        );
        assert_eq!(
            ConfigError::UnsupportedInterval(7).to_string(),
            "unsupported config refresh interval: 7 ms"
        );
    }
}
