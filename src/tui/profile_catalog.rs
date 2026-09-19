//! Read-only TUI profile catalog: built-ins plus discovered custom files.
//!
//! Pure presentation data prepared once at startup, before terminal
//! takeover. [`ProfileCatalog::from_store`] discovers custom profiles
//! through [`ProfileStore`] — the single filesystem owner — and parses
//! each listed file once via [`ProfileStore::load`]. The Ratatui renderer
//! receives this prepared data and performs no I/O itself.
//!
//! A `Valid` custom entry means only that safe storage load, TOML parsing,
//! and profile domain validation succeeded. It does NOT mean the current
//! laptop can apply the profile; capability preview/apply belong to later
//! PLAN-006 tasks.

use crate::profiles::{CustomProfileSlug, Profile, ProfileName, ProfileStore};

/// One custom profile row: immutable render-safe data. No filesystem
/// paths, no store handles, no mutation.
#[derive(Debug, Clone)]
pub struct CustomProfileEntry {
    slug: CustomProfileSlug,
    name: Option<ProfileName>,
    profile: Option<Profile>,
}

impl CustomProfileEntry {
    /// Canonical filename stem (`work` for `work.toml`).
    pub fn slug(&self) -> &CustomProfileSlug {
        &self.slug
    }

    /// Parsed display name; absent when the file failed to load.
    pub fn name(&self) -> Option<&ProfileName> {
        self.name.as_ref()
    }

    /// Retained parsed profile for later selection/details without
    /// reopening the file; absent when the file failed to load. Never
    /// fabricated for invalid entries.
    pub fn profile(&self) -> Option<&Profile> {
        self.profile.as_ref()
    }

    /// True only when storage load, TOML parsing, and domain validation
    /// all succeeded. Says nothing about current-hardware applicability.
    pub fn is_valid(&self) -> bool {
        self.profile.is_some()
    }
}

/// Prepared custom-file section of the Profiles catalog.
#[derive(Debug, Clone, Default)]
pub struct ProfileCatalog {
    customs: Vec<CustomProfileEntry>,
    customs_available: bool,
}

impl ProfileCatalog {
    /// Empty catalog with listing considered healthy: what a missing
    /// profiles directory (or a test harness) produces.
    pub fn empty() -> Self {
        Self {
            customs: Vec::new(),
            customs_available: true,
        }
    }

    /// Catalog whose listing itself failed: built-ins still render while
    /// the custom section reports unavailability instead of entries.
    pub fn unavailable() -> Self {
        Self {
            customs: Vec::new(),
            customs_available: false,
        }
    }

    /// Discovers custom profiles through `store`, preserving its lexical
    /// slug order. Malformed files stay visible as invalid entries; a
    /// list-level failure degrades to [`unavailable`](Self::unavailable)
    /// instead of aborting startup. Performs reads only; creates nothing.
    pub fn from_store(store: &ProfileStore) -> Self {
        let slugs = match store.list() {
            Ok(slugs) => slugs,
            Err(_) => return Self::unavailable(),
        };
        let customs = slugs
            .into_iter()
            .map(|slug| match store.load(&slug) {
                Ok(profile) => CustomProfileEntry {
                    name: Some(profile.name().clone()),
                    profile: Some(profile),
                    slug,
                },
                Err(_) => CustomProfileEntry {
                    slug,
                    name: None,
                    profile: None,
                },
            })
            .collect();
        Self {
            customs,
            customs_available: true,
        }
    }

    /// Custom entries in [`ProfileStore::list`] lexical order.
    pub fn customs(&self) -> &[CustomProfileEntry] {
        &self.customs
    }

    /// False only when listing itself failed; missing-directory empty
    /// results still count as available.
    pub fn customs_available(&self) -> bool {
        self.customs_available
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::*;

    fn store_in(dir: &std::path::Path) -> ProfileStore {
        ProfileStore::new(dir.join("profiles"))
    }

    fn write(store: &ProfileStore, name: &str, contents: &[u8]) {
        fs::create_dir_all(store.directory()).unwrap();
        fs::write(store.directory().join(name), contents).unwrap();
    }

    const WORK: &[u8] = b"name = \"My Work\"\n\n[performance]\nfan_mode = \"silent\"\n";

    #[test]
    fn missing_directory_yields_empty_available_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = ProfileCatalog::from_store(&store_in(dir.path()));
        assert!(catalog.customs().is_empty());
        assert!(catalog.customs_available());
        assert!(!dir.path().join("profiles").exists());
    }

    #[test]
    fn valid_custom_profile_is_discovered() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "work.toml", WORK);
        let catalog = ProfileCatalog::from_store(&store);
        assert_eq!(catalog.customs().len(), 1);
        assert!(catalog.customs_available());
    }

    #[test]
    fn lexical_slug_order_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        for name in ["work.toml", "alpha.toml", "quiet-night.toml"] {
            write(&store, name, WORK);
        }
        let catalog = ProfileCatalog::from_store(&store);
        let slugs: Vec<&str> = catalog
            .customs()
            .iter()
            .map(|entry| entry.slug().as_str())
            .collect();
        assert_eq!(slugs, vec!["alpha", "quiet-night", "work"]);
    }

    #[test]
    fn valid_entry_preserves_exact_slug() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "quiet-night.toml", WORK);
        let catalog = ProfileCatalog::from_store(&store);
        assert_eq!(catalog.customs()[0].slug().as_str(), "quiet-night");
    }

    #[test]
    fn valid_entry_preserves_parsed_display_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "work.toml", WORK);
        let catalog = ProfileCatalog::from_store(&store);
        let entry = &catalog.customs()[0];
        assert!(entry.is_valid());
        assert_eq!(entry.name().unwrap().as_str(), "My Work");
    }

    #[test]
    fn valid_entry_retains_parsed_profile() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "work.toml", WORK);
        let catalog = ProfileCatalog::from_store(&store);
        let profile = catalog.customs()[0].profile().expect("valid entry");
        assert_eq!(profile.name().as_str(), "My Work");
        assert_eq!(
            profile
                .performance()
                .fan_mode()
                .map(crate::hardware::FanMode::as_str),
            Some("silent")
        );
    }

    #[test]
    fn malformed_toml_entry_stays_visible_as_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "work.toml", WORK);
        write(&store, "bad.toml", b"name = [unclosed\n");
        write(&store, "quiet.toml", WORK);
        let catalog = ProfileCatalog::from_store(&store);
        let slugs: Vec<&str> = catalog
            .customs()
            .iter()
            .map(|entry| entry.slug().as_str())
            .collect();
        assert_eq!(slugs, vec!["bad", "quiet", "work"]);
        let bad = &catalog.customs()[0];
        assert!(!bad.is_valid());
        assert_eq!(bad.name(), None);
        assert_eq!(bad.profile(), None);
        assert!(catalog.customs()[1].is_valid());
    }

    #[test]
    fn invalid_schema_entry_stays_visible_as_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "weird.toml", b"name = \"X\"\nunknown = true\n");
        let catalog = ProfileCatalog::from_store(&store);
        assert_eq!(catalog.customs().len(), 1);
        assert!(!catalog.customs()[0].is_valid());
    }

    #[test]
    fn symlink_profile_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "real.toml", WORK);
        symlink(
            store.directory().join("real.toml"),
            store.directory().join("linked.toml"),
        )
        .unwrap();
        let catalog = ProfileCatalog::from_store(&store);
        let slugs: Vec<&str> = catalog
            .customs()
            .iter()
            .map(|entry| entry.slug().as_str())
            .collect();
        assert_eq!(slugs, vec!["real"]);
    }

    #[test]
    fn reserved_builtin_slug_is_not_exposed_as_custom() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(&store, "gaming.toml", WORK);
        write(&store, "work.toml", WORK);
        let catalog = ProfileCatalog::from_store(&store);
        let slugs: Vec<&str> = catalog
            .customs()
            .iter()
            .map(|entry| entry.slug().as_str())
            .collect();
        assert_eq!(slugs, vec!["work"]);
    }

    #[test]
    fn shell_looking_text_remains_data_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        write(
            &store,
            "evil.toml",
            b"name = \"$(rm -rf ~)\"\n\n[performance]\nfan_mode = \"$(id)\"\n",
        );
        let catalog = ProfileCatalog::from_store(&store);
        let entry = &catalog.customs()[0];
        assert!(entry.is_valid());
        assert_eq!(entry.name().unwrap().as_str(), "$(rm -rf ~)");
    }

    #[test]
    fn discovery_creates_no_directory_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        let catalog = ProfileCatalog::from_store(&store_in(dir.path()));
        assert!(catalog.customs().is_empty());
        assert!(!profiles.exists());
        // A present directory gains no files from discovery either.
        let store = store_in(dir.path());
        write(&store, "work.toml", WORK);
        let before: Vec<_> = fs::read_dir(store.directory())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        let _ = ProfileCatalog::from_store(&store);
        let after: Vec<_> = fs::read_dir(store.directory())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(before, after);
        assert_eq!(fs::read(store.directory().join("work.toml")).unwrap(), WORK);
    }

    #[test]
    fn list_level_failure_degrades_to_unavailable() {
        // A regular file where the directory should be makes listing fail
        // deterministically without chmod games.
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        fs::write(&profiles, b"not a directory\n").unwrap();
        let catalog = ProfileCatalog::from_store(&store_in(dir.path()));
        assert!(!catalog.customs_available());
        assert!(catalog.customs().is_empty());
    }

    #[test]
    fn empty_and_unavailable_states_differ() {
        assert!(ProfileCatalog::empty().customs_available());
        assert!(!ProfileCatalog::unavailable().customs_available());
        assert!(ProfileCatalog::empty().customs().is_empty());
    }
}
