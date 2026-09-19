//! Persistent TUI user configuration in `~/.config/mec/config.toml`.
//!
//! [`AppConfig`] is the typed domain model; [`AppConfigStore`] binds it to
//! one file path. Reading is side-effect-free (a missing file yields
//! defaults, creating nothing); saving is atomic (temp file in the same
//! directory plus rename). Config is data only: no environment expansion,
//! no shell, no evaluation.

mod settings;

pub use settings::{AppConfig, MAX_CONFIG_SIZE, parse_config_text};

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Why user configuration could not be loaded or saved. Display text is
/// deliberately path-free so startup/persistence notices never leak home
/// directory locations into the visible UI.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// No platform configuration directory is available.
    #[error("configuration directory unavailable")]
    ConfigDirectoryUnavailable,
    /// An underlying I/O failure (open, read, write, rename).
    #[error("config storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// The file exceeds the 64 KiB limit.
    #[error("config exceeds size limit of {max} bytes")]
    TooLarge {
        /// The enforced limit in bytes.
        max: usize,
    },
    /// The file is not valid UTF-8.
    #[error("config is not valid UTF-8")]
    InvalidUtf8,
    /// The target is a symbolic link.
    #[error("config symlink rejected")]
    SymlinkRejected,
    /// The target exists but is not a regular file.
    #[error("config is not a regular file")]
    NotRegularFile,
    /// The document contains control characters.
    #[error("config contains control characters")]
    ControlCharacters,
    /// Malformed TOML, unknown fields, or wrong value types.
    #[error("invalid config document: {0}")]
    Schema(String),
    /// A theme value outside the canonical slugs plus `default` alias.
    /// The rejected value is never echoed, keeping notices path-free.
    #[error("invalid config theme value")]
    InvalidTheme(String),
    /// A millisecond count outside the [`PollInterval`](crate::monitoring::PollInterval) contract.
    #[error("unsupported config refresh interval: {0} ms")]
    UnsupportedInterval(u64),
}

/// Narrow file binding for [`AppConfig`]: one path, load plus atomic save.
/// Performs no hardware access and never touches profiles.
#[derive(Debug, Clone)]
pub struct AppConfigStore {
    path: PathBuf,
}

impl AppConfigStore {
    /// Binds the store to `path` (…/config.toml). Performs no I/O.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Production binding: platform config dir / `mec` / `config.toml`.
    /// Performs no directory creation.
    pub fn user_default() -> Result<Self, ConfigError> {
        let base = directories::BaseDirs::new().ok_or(ConfigError::ConfigDirectoryUnavailable)?;
        Ok(Self::new(base.config_dir().join("mec").join("config.toml")))
    }

    /// The configured file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads and strictly validates the file. A missing file succeeds with
    /// defaults and creates nothing: no directory, no file. Existing files
    /// open with `O_NOFOLLOW` on the handle used for metadata and reads, so
    /// symlinks, non-regular files, oversized content, and invalid UTF-8
    /// fail with typed errors.
    pub fn load(&self) -> Result<AppConfig, ConfigError> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AppConfig::default());
            }
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
                return Err(ConfigError::SymlinkRejected);
            }
            Err(error) => return Err(ConfigError::Io(error)),
        };
        let metadata = file.metadata().map_err(ConfigError::Io)?;
        if !metadata.file_type().is_file() {
            return Err(ConfigError::NotRegularFile);
        }
        let mut bytes = Vec::new();
        file.take((MAX_CONFIG_SIZE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(ConfigError::Io)?;
        if bytes.len() > MAX_CONFIG_SIZE {
            return Err(ConfigError::TooLarge {
                max: MAX_CONFIG_SIZE,
            });
        }
        let text = String::from_utf8(bytes).map_err(|_| ConfigError::InvalidUtf8)?;
        parse_config_text(&text)
    }

    /// Serializes the full normalized config and replaces the file
    /// atomically: creates the parent directory if needed, writes a
    /// complete temp file beside the destination with user-only
    /// permissions, syncs, then renames over the target. The destination
    /// is never truncated in place; temp files are removed on failure.
    /// Never invokes shell commands.
    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigError> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }
        let contents = config.to_toml_string();
        let temp = self.temp_path();
        let write_result = self.write_temp(&temp, contents.as_bytes());
        if write_result.is_err() {
            let _ = std::fs::remove_file(&temp);
            return write_result;
        }
        match std::fs::rename(&temp, &self.path) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = std::fs::remove_file(&temp);
                Err(ConfigError::Io(error))
            }
        }
    }

    /// Temp file beside the destination: same directory (one filesystem,
    /// so rename is atomic), `create_new` so nothing is ever overwritten.
    fn temp_path(&self) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "config.toml".to_owned());
        self.path
            .with_file_name(format!(".{file_name}.tmp.{}", std::process::id()))
    }

    fn write_temp(&self, temp: &Path, contents: &[u8]) -> Result<(), ConfigError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(temp)
            .map_err(ConfigError::Io)?;
        file.write_all(contents).map_err(ConfigError::Io)?;
        file.flush().map_err(ConfigError::Io)?;
        file.sync_all().map_err(ConfigError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn store_in_temp() -> (tempfile::TempDir, AppConfigStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = AppConfigStore::new(dir.path().join("mec").join("config.toml"));
        (dir, store)
    }

    fn write_raw(store: &AppConfigStore, contents: &[u8]) {
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        std::fs::write(store.path(), contents).unwrap();
    }

    #[test]
    fn missing_config_loads_defaults() {
        let (_temp, store) = store_in_temp();
        assert_eq!(store.load().unwrap(), AppConfig::default());
    }

    #[test]
    fn missing_config_load_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppConfigStore::new(dir.path().join("mec").join("config.toml"));
        assert!(store.load().is_ok());
        assert!(!dir.path().join("mec").exists());
    }

    #[test]
    fn save_creates_normalized_document() {
        let (_temp, store) = store_in_temp();
        let mut config = AppConfig::default();
        config.set_theme(crate::tui::theme::ThemeName::Light);
        store.save(&config).unwrap();
        let text = std::fs::read_to_string(store.path()).unwrap();
        assert_eq!(
            text,
            "refresh_interval_ms = 1000\ntheme = \"light\"\nvim_keys = true\n"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn saved_file_is_user_restrictive() {
        use std::os::unix::fs::MetadataExt;
        let (_temp, store) = store_in_temp();
        store.save(&AppConfig::default()).unwrap();
        let mode = std::fs::metadata(store.path()).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn save_load_round_trip_exact() {
        let (_temp, store) = store_in_temp();
        let mut config = AppConfig::default();
        config.set_theme(crate::tui::theme::ThemeName::Terminal);
        store.save(&config).unwrap();
        assert_eq!(store.load().unwrap(), config);
    }

    #[test]
    fn save_preserves_refresh_and_vim_while_changing_theme() {
        let (_temp, store) = store_in_temp();
        let text = "refresh_interval_ms = 2000\ntheme = \"msi-dark\"\nvim_keys = false\n";
        write_raw(&store, text.as_bytes());
        let mut config = store.load().unwrap();
        assert_eq!(config.refresh_interval_ms(), 2000);
        assert!(!config.vim_keys());
        config.set_theme(crate::tui::theme::ThemeName::Light);
        store.save(&config).unwrap();
        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.refresh_interval_ms(), 2000);
        assert!(!reloaded.vim_keys());
        assert_eq!(reloaded.theme(), crate::tui::theme::ThemeName::Light);
    }

    #[test]
    fn temp_file_does_not_remain_after_success() {
        let (temp, store) = store_in_temp();
        store.save(&AppConfig::default()).unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(temp.path().join("mec"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains("config.toml.tmp")
            })
            .collect();
        assert!(leftovers.is_empty());
        // Only config.toml remains.
        let names: Vec<String> = std::fs::read_dir(temp.path().join("mec"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["config.toml"]);
    }

    #[test]
    fn symlink_config_rejected() {
        let (_temp, store) = store_in_temp();
        std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        let real = store.path().with_extension("real.toml");
        std::fs::write(&real, b"theme = \"light\"\n").unwrap();
        symlink(&real, store.path()).unwrap();
        assert!(matches!(store.load(), Err(ConfigError::SymlinkRejected)));
    }

    #[test]
    fn directory_target_rejected() {
        let (_temp, store) = store_in_temp();
        std::fs::create_dir_all(store.path()).unwrap();
        assert!(matches!(store.load(), Err(ConfigError::NotRegularFile)));
    }

    #[test]
    fn oversized_config_rejected() {
        let (_temp, store) = store_in_temp();
        let mut text = String::from("refresh_interval_ms = 1000\n# ");
        while text.len() <= MAX_CONFIG_SIZE {
            text.push('x');
        }
        write_raw(&store, text.as_bytes());
        assert!(matches!(store.load(), Err(ConfigError::TooLarge { .. })));
    }

    #[test]
    fn invalid_utf8_rejected() {
        let (_temp, store) = store_in_temp();
        write_raw(&store, &[0xff, 0xfe, 0x00]);
        assert!(matches!(store.load(), Err(ConfigError::InvalidUtf8)));
    }

    #[test]
    fn invalid_document_rejected() {
        let (_temp, store) = store_in_temp();
        write_raw(&store, b"theme = \"nope\"\n");
        assert!(matches!(store.load(), Err(ConfigError::InvalidTheme(_))));
        write_raw(&store, b"mouse = true\n");
        assert!(matches!(store.load(), Err(ConfigError::Schema(_))));
    }

    #[test]
    fn save_failure_when_parent_is_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("mec");
        std::fs::write(&blocker, b"not a directory\n").unwrap();
        let store = AppConfigStore::new(blocker.join("config.toml"));
        assert!(store.save(&AppConfig::default()).is_err());
    }
}
