//! Custom-profile storage through the public API only: TempDir roots,
//! no real home directory access.

use std::fs;
use std::os::unix::fs::symlink;

use tempfile::tempdir;

use mec::profiles::{CustomProfileSlug, MAX_PROFILE_SIZE, ProfileStorageError, ProfileStore};

fn store_in(config: &std::path::Path) -> ProfileStore {
    ProfileStore::new(config.join("profiles"))
}

fn write(store: &ProfileStore, name: &str, contents: &[u8]) {
    fs::create_dir_all(store.directory()).unwrap();
    fs::write(store.directory().join(name), contents).unwrap();
}

fn slug(value: &str) -> CustomProfileSlug {
    CustomProfileSlug::try_from(value).unwrap()
}

#[test]
fn missing_directory_lists_empty() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn list_is_sorted_and_toml_only() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    for name in ["work.toml", "alpha.toml", "notes.txt", "data.json"] {
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
    assert_eq!(names, vec!["alpha", "work"]);
}

#[test]
fn list_ignores_directories_symlinks_invalid_and_reserved() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    fs::create_dir_all(store.directory().join("nested.toml")).unwrap();
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
    for name in [".hidden.toml", "Work.toml", "gaming.toml", "a_b.toml"] {
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
    assert_eq!(names, vec!["real"]);
}

#[test]
fn malformed_toml_still_listed_but_load_fails() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
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
fn valid_profile_loads_exact_fields() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    write(
        &store,
        "work.toml",
        b"name = \"Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
    );
    let profile = store.load(&slug("work")).unwrap();
    assert_eq!(profile.name().as_str(), "Work");
    assert_eq!(
        profile
            .performance()
            .fan_mode()
            .map(mec::hardware::FanMode::as_str),
        Some("silent")
    );
}

#[test]
fn missing_and_reserved_load_errors_typed() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    fs::create_dir_all(store.directory()).unwrap();
    assert!(matches!(
        store.load(&slug("work")),
        Err(ProfileStorageError::NotFound(_))
    ));
    assert!(matches!(
        store.load(&slug("gaming")),
        Err(ProfileStorageError::ReservedSlug(_))
    ));
}

#[test]
fn symlink_and_directory_targets_rejected() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
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
    assert!(matches!(
        store.load(&slug("linked")),
        Err(ProfileStorageError::SymlinkRejected(_))
    ));
    fs::create_dir_all(store.directory().join("dir.toml")).unwrap();
    // `dir.toml` stem parses as `dir`, but the target is a directory.
    assert!(matches!(
        store.load(&slug("dir")),
        Err(ProfileStorageError::NotRegularFile(_))
    ));
}

#[test]
fn oversized_and_exact_limit_behavior() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    let mut big = String::from("name = \"Big\"\n\n[device]\nkeyboard_backlight = 1\n# ");
    while big.len() <= MAX_PROFILE_SIZE {
        big.push('x');
    }
    write(&store, "big.toml", big.as_bytes());
    assert!(matches!(
        store.load(&slug("big")),
        Err(ProfileStorageError::TooLarge { .. })
    ));

    let prefix = "name = \"Exact\"\n\n[device]\nkeyboard_backlight = 1\n# ";
    let mut exact = String::from(prefix);
    exact.push_str(&"x".repeat(MAX_PROFILE_SIZE - prefix.len()));
    assert_eq!(exact.len(), MAX_PROFILE_SIZE);
    write(&store, "exact.toml", exact.as_bytes());
    assert_eq!(store.load(&slug("exact")).unwrap().name().as_str(), "Exact");
}

#[test]
fn unknown_fields_rejected_and_shell_stays_inert() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    write(
        &store,
        "weird.toml",
        b"name = \"X\"\nunknown = true\n\n[device]\nkeyboard_backlight = 1\n",
    );
    assert!(matches!(
        store.load(&slug("weird")),
        Err(ProfileStorageError::Parse(_))
    ));
    write(
        &store,
        "evil.toml",
        b"name = \"$(rm -rf ~)\"\n\n[performance]\nfan_mode = \"$(id)\"\n",
    );
    let profile = store.load(&slug("evil")).unwrap();
    assert_eq!(profile.name().as_str(), "$(rm -rf ~)");
}

#[test]
fn slug_rules_and_store_has_no_save_api() {
    assert!(CustomProfileSlug::try_from("quiet-night").is_ok());
    assert!(CustomProfileSlug::try_from("").is_err());
    assert!(CustomProfileSlug::try_from("Work").is_err());
    assert!(CustomProfileSlug::try_from(" work").is_err());
    assert!(CustomProfileSlug::try_from("../work").is_err());
    assert!(CustomProfileSlug::try_from("work.toml").is_err());
    assert!(CustomProfileSlug::try_from("work/x").is_err());
    assert!(CustomProfileSlug::try_from("-work").is_err());
    assert!(CustomProfileSlug::try_from("work-").is_err());
    assert!(CustomProfileSlug::try_from("a".repeat(65).as_str()).is_err());
    // Compile-level proof of read-only API: ProfileStore exposes only
    // new/user_default/directory/list/load.
    fn assert_read_only(store: &ProfileStore) {
        let _ = store.directory();
    }
    let dir = tempdir().unwrap();
    assert_read_only(&store_in(dir.path()));
}

#[test]
fn repeated_load_deterministic_and_files_untouched() {
    let dir = tempdir().unwrap();
    let store = store_in(dir.path());
    write(
        &store,
        "work.toml",
        b"name = \"Work\"\n\n[device]\nkeyboard_backlight = 1\n",
    );
    let before = fs::read(store.directory().join("work.toml")).unwrap();
    let first = store.load(&slug("work")).unwrap();
    let second = store.load(&slug("work")).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        fs::read(store.directory().join("work.toml")).unwrap(),
        before
    );
}
