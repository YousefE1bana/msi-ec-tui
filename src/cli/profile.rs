//! Profile CLI helpers: resolution, stable rendering, list/show helpers.
//!
//! This module owns no filesystem access beyond [`ProfileStore`], performs
//! no writes, and never reads sysfs directly. Built-in presets resolve
//! against caller-supplied [`Capabilities`]; applying a resolved [`Profile`]
//! always goes through `mec::safety::apply_profile`, which remains the
//! authoritative write gate.

use thiserror::Error;

use crate::hardware::{
    Capabilities, CapabilityDetector, CapabilityDiscoveryError, LinuxSysfsReader, SystemPaths,
};
use crate::profiles::{
    BuiltinPreset, BuiltinPresetError, CustomProfileSlug, Profile, ProfileStorageError,
    ProfileStore,
};
use crate::safety::{ProfileApplyError, ProfileApplyReport};

/// Where a resolved profile came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    /// A capability-aware built-in preset.
    Builtin,
    /// A custom file under the profile store.
    Custom,
}

impl ProfileSource {
    /// Stable source label used in `profile show`.
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileSource::Builtin => "built-in",
            ProfileSource::Custom => "custom",
        }
    }
}

/// A resolved profile with its canonical slug and source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfile {
    /// Canonical slug: the built-in slug or the custom filename stem.
    pub slug: String,
    /// Built-in or custom origin.
    pub source: ProfileSource,
    /// The resolved profile data.
    pub profile: Profile,
}

/// Why profile list/show/apply failed, without hiding typed causes.
#[derive(Debug, Error)]
pub enum ProfileCliError {
    /// Custom-profile storage failure.
    #[error("{0}")]
    Storage(#[from] ProfileStorageError),
    /// Capability discovery failure.
    #[error("{0}")]
    Capability(#[from] CapabilityDiscoveryError),
    /// Built-in preset resolution failure.
    #[error("{0}")]
    Preset(#[from] BuiltinPresetError),
    /// Safe transactional application failure.
    #[error("{0}")]
    Apply(#[from] ProfileApplyError),
}

/// Discovers current capabilities through the high-level detector.
/// Never reads sysfs files directly.
pub fn discover_capabilities(paths: SystemPaths) -> Result<Capabilities, ProfileCliError> {
    CapabilityDetector::new(paths, LinuxSysfsReader)
        .discover()
        .map_err(ProfileCliError::from)
}

/// Resolves one built-in preset against supplied capabilities.
/// The result is data, not authorization.
pub fn resolve_builtin(
    capabilities: &Capabilities,
    preset: BuiltinPreset,
) -> Result<Profile, ProfileCliError> {
    preset.resolve(capabilities).map_err(ProfileCliError::from)
}

/// Loads one custom profile through the storage module. Storage owns
/// all filesystem access; this function reads no files directly.
pub fn resolve_custom(
    store: &ProfileStore,
    slug_text: &str,
) -> Result<(CustomProfileSlug, Profile), ProfileCliError> {
    let slug = CustomProfileSlug::try_from(slug_text)
        .map_err(|error| ProfileCliError::Storage(ProfileStorageError::InvalidSlug(error)))?;
    let profile = store.load(&slug)?;
    Ok((slug, profile))
}

/// Resolves a PROFILE argument with built-in-first precedence:
/// an exact [`BuiltinPreset`] slug wins; otherwise the text must name a
/// custom profile in `store`. Unknown or invalid input is a typed error.
pub fn resolve_profile(
    store: &ProfileStore,
    capabilities: &Capabilities,
    slug_text: &str,
) -> Result<ResolvedProfile, ProfileCliError> {
    if let Ok(preset) = BuiltinPreset::from_slug(slug_text) {
        let profile = preset.resolve(capabilities)?;
        return Ok(ResolvedProfile {
            slug: preset.slug().to_owned(),
            source: ProfileSource::Builtin,
            profile,
        });
    }
    let (slug, profile) = resolve_custom(store, slug_text)?;
    Ok(ResolvedProfile {
        slug: slug.as_str().to_owned(),
        source: ProfileSource::Custom,
        profile,
    })
}

/// Renders the stable `profile list` view: built-ins in catalog order,
/// then custom slugs in the caller-supplied (lexical) order.
/// Performs no hardware access.
pub fn render_list_text(custom_slugs: &[CustomProfileSlug]) -> String {
    let mut output = String::from("Built-in profiles:\n");
    for preset in BuiltinPreset::all() {
        output.push_str(&format!("  {:<18}{}\n", preset.slug(), preset.name()));
    }
    output.push_str("Custom profiles:\n");
    if custom_slugs.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for slug in custom_slugs {
            output.push_str(&format!("  {slug}\n"));
        }
    }
    output
}

/// Renders the deterministic `profile show` view. Only fields present in
/// the profile are printed; battery renders the end threshold only.
pub fn render_show_text(slug: &str, source: ProfileSource, profile: &Profile) -> String {
    let mut output = format!(
        "Profile: {}\nSource: {}\nSlug: {slug}\n",
        profile.name(),
        source.as_str()
    );
    let mut sections = Vec::new();
    let performance = profile.performance();
    let mut block = String::from("[performance]\n");
    let mut has_performance = false;
    if let Some(mode) = performance.shift_mode() {
        block.push_str(&format!("shift_mode = {mode}\n"));
        has_performance = true;
    }
    if let Some(mode) = performance.fan_mode() {
        block.push_str(&format!("fan_mode = {mode}\n"));
        has_performance = true;
    }
    if let Some(value) = performance.cooler_boost() {
        block.push_str(&format!("cooler_boost = {value}\n"));
        has_performance = true;
    }
    if let Some(value) = performance.super_battery() {
        block.push_str(&format!("super_battery = {value}\n"));
        has_performance = true;
    }
    if has_performance {
        sections.push(block);
    }
    if let Some(threshold) = profile.battery().threshold() {
        sections.push(format!(
            "[battery]\ncharge_end_threshold = {}\n",
            threshold.end_percent()
        ));
    }
    if let Some(level) = profile.device().keyboard_backlight() {
        sections.push(format!("[device]\nkeyboard_backlight = {level}\n"));
    }
    if !sections.is_empty() {
        output.push('\n');
        output.push_str(&sections.join("\n"));
    }
    output
}

/// Renders the stable `profile apply` success line from actual report counts.
pub fn render_apply_success(profile: &Profile, report: &ProfileApplyReport) -> String {
    let changed = report.applied().len();
    let unchanged = report.unchanged().len();
    if report.is_noop() {
        format!(
            "MEC profile already applied: {} (0 changed, {unchanged} unchanged)",
            profile.name()
        )
    } else {
        format!(
            "MEC profile applied: {} ({changed} changed, {unchanged} unchanged)",
            profile.name()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{BacklightCapability, FanMode, ShiftMode};

    fn full_capabilities() -> Capabilities {
        Capabilities {
            fan_modes: vec![
                FanMode::try_from("auto").unwrap(),
                FanMode::try_from("silent").unwrap(),
                FanMode::try_from("advanced").unwrap(),
            ],
            shift_modes: vec![
                ShiftMode::try_from("eco").unwrap(),
                ShiftMode::try_from("comfort").unwrap(),
                ShiftMode::try_from("turbo").unwrap(),
            ],
            cooler_boost: true,
            super_battery: true,
            battery_thresholds: true,
            keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
            ..Capabilities::default()
        }
    }

    fn temp_store() -> (tempfile::TempDir, ProfileStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(dir.path().join("profiles"));
        (dir, store)
    }

    #[test]
    fn list_renders_builtins_in_order_with_none() {
        let text = render_list_text(&[]);
        assert_eq!(
            text,
            "Built-in profiles:\n  balanced          Balanced\n  silent            Silent\n  gaming            Gaming\n  battery-saver     Battery Saver\n  maximum-cooling   Maximum Cooling\nCustom profiles:\n  (none)\n"
        );
    }

    #[test]
    fn list_renders_customs_sorted() {
        let customs = vec![
            CustomProfileSlug::try_from("work").unwrap(),
            CustomProfileSlug::try_from("quiet-night").unwrap(),
        ];
        let text = render_list_text(&customs);
        assert!(text.ends_with("Custom profiles:\n  work\n  quiet-night\n"));
    }

    #[test]
    fn builtin_wins_over_custom_name() {
        let (_dir, store) = temp_store();
        let capabilities = full_capabilities();
        let resolved = resolve_profile(&store, &capabilities, "gaming").unwrap();
        assert_eq!(resolved.source, ProfileSource::Builtin);
        assert_eq!(resolved.slug, "gaming");
        assert_eq!(resolved.profile.name().as_str(), "Gaming");
    }

    #[test]
    fn unknown_profile_is_typed_not_found() {
        let (_dir, store) = temp_store();
        let error = resolve_profile(&store, &full_capabilities(), "work").unwrap_err();
        assert!(matches!(
            error,
            ProfileCliError::Storage(ProfileStorageError::NotFound(_))
        ));
    }

    #[test]
    fn invalid_slug_is_typed() {
        let (_dir, store) = temp_store();
        let error = resolve_profile(&store, &full_capabilities(), "Work").unwrap_err();
        assert!(matches!(
            error,
            ProfileCliError::Storage(ProfileStorageError::InvalidSlug(_))
        ));
    }

    #[test]
    fn show_renders_only_present_fields() {
        let profile: Profile = "name = \"Work\"\n\n[performance]\nfan_mode = \"silent\"\n"
            .parse()
            .unwrap();
        let text = render_show_text("work", ProfileSource::Custom, &profile);
        assert_eq!(
            text,
            "Profile: Work\nSource: custom\nSlug: work\n\n[performance]\nfan_mode = silent\n"
        );
    }

    #[test]
    fn show_renders_battery_end_only() {
        let profile: Profile = "name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 80\n"
            .parse()
            .unwrap();
        let text = render_show_text("saver", ProfileSource::Custom, &profile);
        assert!(text.contains("[battery]\ncharge_end_threshold = 80\n"));
        assert!(!text.contains("70"));
    }

    #[test]
    fn show_renders_backlight() {
        let profile: Profile = "name = \"Glow\"\n\n[device]\nkeyboard_backlight = 2\n"
            .parse()
            .unwrap();
        let text = render_show_text("glow", ProfileSource::Custom, &profile);
        assert!(text.contains("[device]\nkeyboard_backlight = 2\n"));
    }

    #[test]
    fn show_uses_inner_display_name() {
        let profile: Profile = "name = \"My Work\"\n\n[device]\nkeyboard_backlight = 1\n"
            .parse()
            .unwrap();
        let text = render_show_text("work", ProfileSource::Custom, &profile);
        assert!(text.starts_with("Profile: My Work\n"));
    }

    #[test]
    fn builtin_resolution_omits_unsupported_fields() {
        let capabilities = Capabilities {
            shift_modes: vec![ShiftMode::try_from("turbo").unwrap()],
            cooler_boost: true,
            ..Capabilities::default()
        };
        let profile = resolve_builtin(&capabilities, BuiltinPreset::Gaming).unwrap();
        assert_eq!(
            profile.performance().shift_mode().map(ShiftMode::as_str),
            Some("turbo")
        );
        assert_eq!(profile.performance().fan_mode(), None);
    }

    #[test]
    fn unavailable_builtin_is_typed() {
        let error = resolve_builtin(&Capabilities::default(), BuiltinPreset::Gaming).unwrap_err();
        assert!(matches!(error, ProfileCliError::Preset(_)));
    }

    #[test]
    fn source_labels_stable() {
        assert_eq!(ProfileSource::Builtin.as_str(), "built-in");
        assert_eq!(ProfileSource::Custom.as_str(), "custom");
    }
}
