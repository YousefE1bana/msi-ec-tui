//! Read-only custom-profile storage for `~/.config/mec/profiles/`.
//!
//! Profiles remain strict TOML data: [`ProfileStore`] discovers filenames
//! and loads file contents only through [`Profile::parse_toml`]. There is
//! no save, delete, apply, shell, environment, or recursive-reference
//! handling. Built-in preset slugs are reserved and never exposed as
//! custom profiles.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use thiserror::Error;

use super::{BuiltinPreset, Profile, ProfileParseError};

/// Maximum accepted profile file size: 64 KiB.
pub const MAX_PROFILE_SIZE: usize = 64 * 1024;

/// A canonical custom-profile filename slug: `work` maps to `work.toml`.
///
/// Rules: 1..=64 ASCII bytes; lowercase letters, digits, hyphen only;
/// first and last characters alphanumeric; no whitespace, uppercase,
/// dots, slashes, or `..` (all rejected by the character rules).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CustomProfileSlug(String);

/// Why a raw string is not a valid custom-profile slug.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CustomProfileSlugError {
    /// The slug is empty.
    #[error("custom profile slug must not be empty")]
    Empty,
    /// The slug exceeds 64 bytes.
    #[error("custom profile slug must not exceed 64 bytes")]
    TooLong,
    /// The slug contains a forbidden character or placement.
    #[error("invalid custom profile slug: {0}")]
    Invalid(String),
}

/// Why custom-profile discovery or loading failed.
#[derive(Debug, Error)]
pub enum ProfileStorageError {
    /// No platform configuration directory is available.
    #[error("configuration directory unavailable")]
    ConfigDirectoryUnavailable,
    /// A raw slug string violates the canonical slug rules.
    #[error("{0}")]
    InvalidSlug(#[from] CustomProfileSlugError),
    /// The slug is reserved by a built-in preset.
    #[error("custom profile slug is reserved by a built-in preset: {0}")]
    ReservedSlug(String),
    /// No such `<slug>.toml` exists beneath the store directory.
    #[error("custom profile not found: {0}")]
    NotFound(String),
    /// The target exists but is not a regular file.
    #[error("custom profile is not a regular file: {0}")]
    NotRegularFile(String),
    /// The target is a symbolic link.
    #[error("custom profile symlink rejected: {0}")]
    SymlinkRejected(String),
    /// The file exceeds the 64 KiB limit.
    #[error("custom profile exceeds size limit of {max} bytes")]
    TooLarge {
        /// The enforced limit in bytes.
        max: usize,
    },
    /// An underlying I/O failure.
    #[error("custom profile storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// The file is not valid UTF-8.
    #[error("custom profile is not valid UTF-8")]
    InvalidUtf8,
    /// The file contents violate the strict profile schema.
    #[error("{0}")]
    Parse(#[from] ProfileParseError),
}

/// Read-only discovery and loading of custom profiles in one directory.
#[derive(Debug, Clone)]
pub struct ProfileStore {
    directory: PathBuf,
}

fn validate_slug_syntax(value: &str) -> Result<(), CustomProfileSlugError> {
    if value.is_empty() {
        return Err(CustomProfileSlugError::Empty);
    }
    if value.len() > 64 {
        return Err(CustomProfileSlugError::TooLong);
    }
    let bytes = value.as_bytes();
    let is_alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !is_alnum(bytes[0]) || !is_alnum(bytes[bytes.len() - 1]) {
        return Err(CustomProfileSlugError::Invalid(value.to_owned()));
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    {
        return Err(CustomProfileSlugError::Invalid(value.to_owned()));
    }
    Ok(())
}

/// Whether a slug string is reserved by a built-in preset.
fn is_reserved_slug(value: &str) -> bool {
    BuiltinPreset::from_slug(value).is_ok()
}

impl CustomProfileSlug {
    /// The slug text, usable as `<slug>.toml` stem.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this slug is reserved by a built-in preset.
    pub fn is_reserved(&self) -> bool {
        is_reserved_slug(&self.0)
    }
}

impl TryFrom<&str> for CustomProfileSlug {
    type Error = CustomProfileSlugError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        validate_slug_syntax(value)?;
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for CustomProfileSlug {
    type Error = CustomProfileSlugError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_slug_syntax(&value)?;
        Ok(Self(value))
    }
}

impl FromStr for CustomProfileSlug {
    type Err = CustomProfileSlugError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}

impl fmt::Display for CustomProfileSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ProfileStore {
    /// Uses `directory` as the profiles root. Performs no I/O.
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// Resolves `~/.config/mec/profiles/` through the platform
    /// configuration-directory abstraction. Performs no directory
    /// creation.
    pub fn user_default() -> Result<Self, ProfileStorageError> {
        let base =
            directories::BaseDirs::new().ok_or(ProfileStorageError::ConfigDirectoryUnavailable)?;
        Ok(Self::new(base.config_dir().join("mec").join("profiles")))
    }

    /// The configured profiles directory.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Discovers direct-child regular `*.toml` files with valid,
    /// non-reserved slugs, in lexical slug order. A missing directory
    /// yields an empty list. File contents are never parsed here, so a
    /// malformed TOML file is still listed under its valid slug.
    pub fn list(&self) -> Result<Vec<CustomProfileSlug>, ProfileStorageError> {
        let entries = match std::fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(ProfileStorageError::Io(error)),
        };
        let mut slugs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(ProfileStorageError::Io)?;
            let file_type = entry.file_type().map_err(ProfileStorageError::Io)?;
            if file_type.is_symlink() || !file_type.is_file() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name.starts_with('#') || name.ends_with('~') {
                continue;
            }
            let Some(stem) = name.strip_suffix(".toml") else {
                continue;
            };
            let Ok(slug) = CustomProfileSlug::try_from(stem) else {
                continue;
            };
            if slug.is_reserved() {
                continue;
            }
            slugs.push(slug);
        }
        slugs.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        Ok(slugs)
    }

    /// Loads `<slug>.toml` beneath the store directory and parses it via
    /// [`Profile::parse_toml`]. Rejects reserved slugs, missing targets,
    /// symlinks, non-regular files, oversized files, non-UTF-8 bytes, and
    /// schema violations with typed errors. Performs no writes.
    pub fn load(&self, slug: &CustomProfileSlug) -> Result<Profile, ProfileStorageError> {
        if slug.is_reserved() {
            return Err(ProfileStorageError::ReservedSlug(slug.as_str().to_owned()));
        }
        let target = self.directory.join(format!("{}.toml", slug.as_str()));
        match std::fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(ProfileStorageError::SymlinkRejected(
                        slug.as_str().to_owned(),
                    ));
                }
                if !metadata.file_type().is_file() {
                    return Err(ProfileStorageError::NotRegularFile(
                        slug.as_str().to_owned(),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProfileStorageError::NotFound(slug.as_str().to_owned()));
            }
            Err(error) => return Err(ProfileStorageError::Io(error)),
        }
        let file = match File::open(&target) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ProfileStorageError::NotFound(slug.as_str().to_owned()));
            }
            Err(error) => return Err(ProfileStorageError::Io(error)),
        };
        let mut bytes = Vec::new();
        file.take((MAX_PROFILE_SIZE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(ProfileStorageError::Io)?;
        if bytes.len() > MAX_PROFILE_SIZE {
            return Err(ProfileStorageError::TooLarge {
                max: MAX_PROFILE_SIZE,
            });
        }
        let text = String::from_utf8(bytes).map_err(|_| ProfileStorageError::InvalidUtf8)?;
        Profile::parse_toml(&text).map_err(ProfileStorageError::Parse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    fn store_in_temp() -> (tempfile::TempDir, ProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(dir.path().join("profiles"));
        (dir, store)
    }

    fn write(store: &ProfileStore, name: &str, contents: &[u8]) {
        fs::create_dir_all(store.directory()).unwrap();
        fs::write(store.directory().join(name), contents).unwrap();
    }

    fn slug(value: &str) -> CustomProfileSlug {
        CustomProfileSlug::try_from(value).unwrap()
    }

    #[test]
    fn valid_basic_slug() {
        assert_eq!(slug("work").as_str(), "work");
    }

    #[test]
    fn valid_hyphen_and_digits() {
        for value in ["quiet-night", "gaming-2", "profile1", "a", "0", "a1-2b"] {
            assert_eq!(slug(value).as_str(), value);
        }
    }

    #[test]
    fn empty_slug_rejected() {
        assert_eq!(
            CustomProfileSlug::try_from(""),
            Err(CustomProfileSlugError::Empty)
        );
    }

    #[test]
    fn uppercase_slug_rejected() {
        assert!(CustomProfileSlug::try_from("Work").is_err());
        assert!(CustomProfileSlug::try_from("WORK").is_err());
    }

    #[test]
    fn whitespace_slug_rejected() {
        for value in [" work", "work ", "qu iet", "work\n"] {
            assert!(CustomProfileSlug::try_from(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn traversal_rejected() {
        for value in ["../work", "..", "work/../x"] {
            assert!(CustomProfileSlug::try_from(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn dot_rejected() {
        for value in ["work.toml", "work.stuff", ".work"] {
            assert!(CustomProfileSlug::try_from(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn slash_and_backslash_rejected() {
        for value in ["work/profile", "work\\profile"] {
            assert!(CustomProfileSlug::try_from(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn leading_trailing_hyphen_rejected() {
        for value in ["-work", "work-", "-"] {
            assert!(CustomProfileSlug::try_from(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn underscore_rejected() {
        assert!(CustomProfileSlug::try_from("a_b").is_err());
    }

    #[test]
    fn over_64_bytes_rejected() {
        let long = "a".repeat(65);
        assert_eq!(
            CustomProfileSlug::try_from(long.as_str()),
            Err(CustomProfileSlugError::TooLong)
        );
        assert!(CustomProfileSlug::try_from("a".repeat(64).as_str()).is_ok());
    }

    #[test]
    fn nul_rejected() {
        assert!(CustomProfileSlug::try_from("wo\0rk").is_err());
    }

    #[test]
    fn builtin_slugs_reserved_by_store() {
        for value in [
            "balanced",
            "silent",
            "gaming",
            "battery-saver",
            "maximum-cooling",
        ] {
            assert!(slug(value).is_reserved(), "{value}");
        }
        assert!(!slug("work").is_reserved());
    }

    #[test]
    fn slug_display_and_fromstr() {
        assert_eq!(slug("work").to_string(), "work");
        assert_eq!("work".parse::<CustomProfileSlug>().unwrap(), slug("work"));
    }

    #[test]
    fn missing_directory_lists_empty() {
        let (_temp, store) = store_in_temp();
        assert_eq!(store.list().unwrap(), Vec::new());
    }

    #[test]
    fn list_sorted_results() {
        let (_temp, store) = store_in_temp();
        for name in ["work.toml", "alpha.toml", "quiet-night.toml"] {
            write(
                &store,
                name,
                b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
            );
        }
        let names: Vec<String> = store
            .list()
            .unwrap()
            .iter()
            .map(|slug| slug.to_string())
            .collect();
        assert_eq!(names, vec!["alpha", "quiet-night", "work"]);
    }

    #[test]
    fn list_only_toml_regular_files() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "work.toml",
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        write(&store, "notes.txt", b"hello");
        write(&store, "data.json", b"{}");
        write(&store, "upper.TOML", b"name = \"X\"\n");
        let names: Vec<String> = store
            .list()
            .unwrap()
            .iter()
            .map(|slug| slug.to_string())
            .collect();
        assert_eq!(names, vec!["work"]);
    }

    #[test]
    fn list_ignores_directories() {
        let (_temp, store) = store_in_temp();
        fs::create_dir_all(store.directory().join("nested.toml")).unwrap();
        write(
            &store,
            "work.toml",
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        let names: Vec<String> = store
            .list()
            .unwrap()
            .iter()
            .map(|slug| slug.to_string())
            .collect();
        assert_eq!(names, vec!["work"]);
    }

    #[test]
    fn list_ignores_symlinks() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "real.toml",
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        symlink(
            store.directory().join("real.toml"),
            store.directory().join("linked.toml"),
        )
        .unwrap();
        let names: Vec<String> = store
            .list()
            .unwrap()
            .iter()
            .map(|slug| slug.to_string())
            .collect();
        assert_eq!(names, vec!["real"]);
    }

    #[test]
    fn list_ignores_invalid_and_hidden_and_reserved() {
        let (_temp, store) = store_in_temp();
        for name in [
            ".hidden.toml",
            "Work.toml",
            "work-.toml",
            "gaming.toml",
            "balanced.toml",
            "a_b.toml",
        ] {
            write(
                &store,
                name,
                b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
            );
        }
        write(
            &store,
            "work.toml",
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        let names: Vec<String> = store
            .list()
            .unwrap()
            .iter()
            .map(|slug| slug.to_string())
            .collect();
        assert_eq!(names, vec!["work"]);
    }

    #[test]
    fn malformed_toml_still_listed() {
        let (_temp, store) = store_in_temp();
        write(&store, "broken.toml", b"name = [unclosed\n");
        let names: Vec<String> = store
            .list()
            .unwrap()
            .iter()
            .map(|slug| slug.to_string())
            .collect();
        assert_eq!(names, vec!["broken"]);
        assert!(matches!(
            store.load(&slug("broken")),
            Err(ProfileStorageError::Parse(_))
        ));
    }

    #[test]
    fn valid_custom_profile_loads_exact_fields() {
        let (_temp, store) = store_in_temp();
        let text = "name = \"Work\"\n\n[performance]\nfan_mode = \"silent\"\n\n[battery]\ncharge_end_threshold = 80\n\n[device]\nkeyboard_backlight = 2\n";
        write(&store, "work.toml", text.as_bytes());
        let profile = store.load(&slug("work")).unwrap();
        assert_eq!(profile.name().as_str(), "Work");
        assert_eq!(
            profile
                .performance()
                .fan_mode()
                .map(crate::hardware::FanMode::as_str),
            Some("silent")
        );
        assert_eq!(profile.battery().threshold().unwrap().end_percent(), 80);
        assert_eq!(profile.device().keyboard_backlight(), Some(2));
    }

    #[test]
    fn missing_file_typed_error() {
        let (_temp, store) = store_in_temp();
        fs::create_dir_all(store.directory()).unwrap();
        assert!(matches!(
            store.load(&slug("work")),
            Err(ProfileStorageError::NotFound(_))
        ));
    }

    #[test]
    fn reserved_builtin_load_rejected() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "gaming.toml",
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        assert!(matches!(
            store.load(&slug("gaming")),
            Err(ProfileStorageError::ReservedSlug(_))
        ));
    }

    #[test]
    fn symlink_load_rejected() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "real.toml",
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        symlink(
            store.directory().join("real.toml"),
            store.directory().join("linked.toml"),
        )
        .unwrap();
        // `linked` is syntactically valid but a symlink: rejected.
        assert!(matches!(
            store.load(&slug("linked")),
            Err(ProfileStorageError::SymlinkRejected(_))
        ));
        // Target bytes unchanged.
        assert_eq!(
            fs::read(store.directory().join("real.toml")).unwrap(),
            b"name = \"X\"\n\n[device]\nkeyboard_backlight = 1\n"
        );
    }

    #[test]
    fn directory_target_rejected() {
        let (_temp, store) = store_in_temp();
        fs::create_dir_all(store.directory().join("work.toml")).unwrap();
        assert!(matches!(
            store.load(&slug("work")),
            Err(ProfileStorageError::NotRegularFile(_))
        ));
    }

    #[test]
    fn malformed_toml_preserves_parse_error() {
        let (_temp, store) = store_in_temp();
        write(&store, "broken.toml", b"name = [unclosed\n");
        assert!(matches!(
            store.load(&slug("broken")),
            Err(ProfileStorageError::Parse(ProfileParseError::Toml(_)))
        ));
    }

    #[test]
    fn unknown_toml_fields_rejected() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "work.toml",
            b"name = \"X\"\nunknown = true\n\n[device]\nkeyboard_backlight = 1\n",
        );
        assert!(matches!(
            store.load(&slug("work")),
            Err(ProfileStorageError::Parse(_))
        ));
    }

    #[test]
    fn oversized_profile_rejected() {
        let (_temp, store) = store_in_temp();
        let mut text = String::from("name = \"Big\"\n\n[device]\nkeyboard_backlight = 1\n# ");
        while text.len() <= MAX_PROFILE_SIZE {
            text.push('x');
        }
        assert!(text.len() > MAX_PROFILE_SIZE);
        write(&store, "big.toml", text.as_bytes());
        assert!(matches!(
            store.load(&slug("big")),
            Err(ProfileStorageError::TooLarge {
                max: MAX_PROFILE_SIZE
            })
        ));
    }

    #[test]
    fn exactly_max_size_loads_when_valid() {
        let (_temp, store) = store_in_temp();
        let prefix = "name = \"Big\"\n\n[device]\nkeyboard_backlight = 1\n# ";
        let padding = MAX_PROFILE_SIZE - prefix.len();
        let mut text = String::from(prefix);
        text.push_str(&"x".repeat(padding));
        assert_eq!(text.len(), MAX_PROFILE_SIZE);
        write(&store, "big.toml", text.as_bytes());
        // Comment-only padding keeps the document valid at exactly the limit.
        assert_eq!(store.load(&slug("big")).unwrap().name().as_str(), "Big");
    }

    #[test]
    fn shell_looking_text_stays_inert() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "evil.toml",
            b"name = \"$(rm -rf ~)\"\n\n[performance]\nfan_mode = \"$(id)\"\n",
        );
        let profile = store.load(&slug("evil")).unwrap();
        assert_eq!(profile.name().as_str(), "$(rm -rf ~)");
    }

    #[test]
    fn invalid_utf8_rejected() {
        let (_temp, store) = store_in_temp();
        write(&store, "binary.toml", &[0xff, 0xfe, 0x00]);
        assert!(matches!(
            store.load(&slug("binary")),
            Err(ProfileStorageError::InvalidUtf8)
        ));
    }

    #[test]
    fn list_and_load_never_mutate_files() {
        let (_temp, store) = store_in_temp();
        let text = b"name = \"Work\"\n\n[device]\nkeyboard_backlight = 1\n";
        write(&store, "work.toml", text);
        let before = fs::read(store.directory().join("work.toml")).unwrap();
        let _ = store.list().unwrap();
        let _ = store.load(&slug("work")).unwrap();
        assert_eq!(
            fs::read(store.directory().join("work.toml")).unwrap(),
            before
        );
    }

    #[test]
    fn repeated_load_deterministic() {
        let (_temp, store) = store_in_temp();
        write(
            &store,
            "work.toml",
            b"name = \"Work\"\n\n[device]\nkeyboard_backlight = 1\n",
        );
        assert_eq!(
            store.load(&slug("work")).unwrap(),
            store.load(&slug("work")).unwrap()
        );
    }

    #[test]
    fn error_display_stable() {
        assert_eq!(
            ProfileStorageError::ReservedSlug("gaming".to_owned()).to_string(),
            "custom profile slug is reserved by a built-in preset: gaming"
        );
        assert_eq!(
            ProfileStorageError::TooLarge {
                max: MAX_PROFILE_SIZE
            }
            .to_string(),
            format!("custom profile exceeds size limit of {MAX_PROFILE_SIZE} bytes")
        );
    }
}
