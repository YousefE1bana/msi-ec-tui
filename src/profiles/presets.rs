//! Capability-aware built-in presets: fixed conservative recipes resolved
//! against supplied capabilities into plain [`Profile`] data.
//!
//! A preset never emits controls the supplied [`Capabilities`] do not
//! advertise: unsupported fields are omitted, and a preset with nothing to
//! request fails closed as unavailable. v0.5 policy covers performance
//! controls only — never battery thresholds, backlight, webcam controls,
//! or automation. The resolved [`Profile`] is data, not authorization:
//! [`ProfilePlanner::preview`] and [`apply_profile`] must still validate
//! freshly before any write.
//!
//! [`apply_profile`]: crate::safety::apply_profile
//! [`ProfilePlanner::preview`]: crate::profiles::ProfilePlanner::preview

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::hardware::Capabilities;

use super::{Profile, ProfileParseError};

/// Built-in conservative performance presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinPreset {
    /// Comfortable everyday performance.
    Balanced,
    /// Quiet low-power runtime behavior.
    Silent,
    /// Maximum responsive performance.
    Gaming,
    /// Low-power runtime behavior (not charging policy).
    BatterySaver,
    /// Cooling controls only.
    MaximumCooling,
}

/// One fixed recipe field: a performance setting included only when the
/// matching capability is advertised.
struct PresetRecipe {
    shift_mode: Option<&'static str>,
    fan_mode: Option<&'static str>,
    cooler_boost: Option<bool>,
    super_battery: Option<bool>,
}

impl BuiltinPreset {
    /// All built-ins in stable catalog order.
    pub fn all() -> [BuiltinPreset; 5] {
        [
            BuiltinPreset::Balanced,
            BuiltinPreset::Silent,
            BuiltinPreset::Gaming,
            BuiltinPreset::BatterySaver,
            BuiltinPreset::MaximumCooling,
        ]
    }

    /// Exact user-facing display name.
    pub fn name(&self) -> &'static str {
        match self {
            BuiltinPreset::Balanced => "Balanced",
            BuiltinPreset::Silent => "Silent",
            BuiltinPreset::Gaming => "Gaming",
            BuiltinPreset::BatterySaver => "Battery Saver",
            BuiltinPreset::MaximumCooling => "Maximum Cooling",
        }
    }

    /// Exact stable machine slug.
    pub fn slug(&self) -> &'static str {
        match self {
            BuiltinPreset::Balanced => "balanced",
            BuiltinPreset::Silent => "silent",
            BuiltinPreset::Gaming => "gaming",
            BuiltinPreset::BatterySaver => "battery-saver",
            BuiltinPreset::MaximumCooling => "maximum-cooling",
        }
    }

    /// Typed lookup from an exact slug. No fuzzy, path-like, or
    /// case-insensitive matching.
    pub fn from_slug(slug: &str) -> Result<Self, BuiltinPresetError> {
        match slug {
            "balanced" => Ok(BuiltinPreset::Balanced),
            "silent" => Ok(BuiltinPreset::Silent),
            "gaming" => Ok(BuiltinPreset::Gaming),
            "battery-saver" => Ok(BuiltinPreset::BatterySaver),
            "maximum-cooling" => Ok(BuiltinPreset::MaximumCooling),
            _ => Err(BuiltinPresetError::UnknownSlug(slug.to_owned())),
        }
    }

    /// Resolves the preset against supplied capabilities into plain
    /// [`Profile`] data, omitting every unsupported field. Fails closed
    /// when nothing would remain.
    pub fn resolve(&self, capabilities: &Capabilities) -> Result<Profile, BuiltinPresetError> {
        let recipe = self.recipe();
        let mut document = format!("name = \"{}\"\n\n[performance]\n", self.name());
        let mut settings = 0;
        if let Some(mode) = recipe.shift_mode
            && capabilities
                .shift_modes
                .iter()
                .any(|advertised| advertised.as_str() == mode)
        {
            document.push_str(&format!("shift_mode = \"{mode}\"\n"));
            settings += 1;
        }
        if let Some(mode) = recipe.fan_mode
            && capabilities
                .fan_modes
                .iter()
                .any(|advertised| advertised.as_str() == mode)
        {
            document.push_str(&format!("fan_mode = \"{mode}\"\n"));
            settings += 1;
        }
        if let Some(value) = recipe.cooler_boost
            && capabilities.cooler_boost
        {
            document.push_str(&format!("cooler_boost = {value}\n"));
            settings += 1;
        }
        if let Some(value) = recipe.super_battery
            && capabilities.super_battery
        {
            document.push_str(&format!("super_battery = {value}\n"));
            settings += 1;
        }
        if settings == 0 {
            return Err(BuiltinPresetError::Unavailable(*self));
        }
        Profile::parse_toml(&document).map_err(BuiltinPresetError::InvalidDefinition)
    }

    fn recipe(&self) -> PresetRecipe {
        match self {
            BuiltinPreset::Balanced => PresetRecipe {
                shift_mode: Some("comfort"),
                fan_mode: Some("auto"),
                cooler_boost: Some(false),
                super_battery: Some(false),
            },
            BuiltinPreset::Silent => PresetRecipe {
                shift_mode: Some("eco"),
                fan_mode: Some("silent"),
                cooler_boost: Some(false),
                super_battery: None,
            },
            BuiltinPreset::Gaming => PresetRecipe {
                shift_mode: Some("turbo"),
                fan_mode: Some("advanced"),
                cooler_boost: Some(true),
                super_battery: Some(false),
            },
            BuiltinPreset::BatterySaver => PresetRecipe {
                shift_mode: Some("eco"),
                fan_mode: Some("silent"),
                cooler_boost: Some(false),
                super_battery: Some(true),
            },
            BuiltinPreset::MaximumCooling => PresetRecipe {
                shift_mode: None,
                fan_mode: Some("advanced"),
                cooler_boost: Some(true),
                super_battery: None,
            },
        }
    }
}

impl fmt::Display for BuiltinPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for BuiltinPreset {
    type Err = BuiltinPresetError;

    fn from_str(slug: &str) -> Result<Self, Self::Err> {
        Self::from_slug(slug)
    }
}

/// Why preset lookup or resolution failed.
#[derive(Debug, Error)]
pub enum BuiltinPresetError {
    /// No built-in has this slug.
    #[error("unknown built-in preset: {0}")]
    UnknownSlug(String),
    /// Capability-aware omission left zero settings.
    #[error("built-in preset is unavailable on current capabilities: {0}")]
    Unavailable(BuiltinPreset),
    /// A fixed internal recipe failed to parse (must not happen).
    #[error("invalid internal built-in preset definition: {0}")]
    InvalidDefinition(#[from] ProfileParseError),
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::hardware::{BacklightCapability, FanMode, ShiftMode, SupportMode};
    use crate::profiles::ProfilePlanner;

    /// Full test capabilities: every mode the recipes may request, plus
    /// battery/backlight support that presets must ignore.
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

    #[test]
    fn exactly_five_builtins_in_catalog_order() {
        assert_eq!(
            BuiltinPreset::all(),
            [
                BuiltinPreset::Balanced,
                BuiltinPreset::Silent,
                BuiltinPreset::Gaming,
                BuiltinPreset::BatterySaver,
                BuiltinPreset::MaximumCooling,
            ]
        );
    }

    #[test]
    fn names_are_exact() {
        let names: Vec<&str> = BuiltinPreset::all()
            .iter()
            .map(|preset| preset.name())
            .collect();
        assert_eq!(
            names,
            vec![
                "Balanced",
                "Silent",
                "Gaming",
                "Battery Saver",
                "Maximum Cooling",
            ]
        );
    }

    #[test]
    fn slugs_are_exact() {
        let slugs: Vec<&str> = BuiltinPreset::all()
            .iter()
            .map(|preset| preset.slug())
            .collect();
        assert_eq!(
            slugs,
            vec![
                "balanced",
                "silent",
                "gaming",
                "battery-saver",
                "maximum-cooling",
            ]
        );
    }

    #[test]
    fn names_and_slugs_are_unique() {
        let names: HashSet<&str> = BuiltinPreset::all()
            .iter()
            .map(|preset| preset.name())
            .collect();
        let slugs: HashSet<&str> = BuiltinPreset::all()
            .iter()
            .map(|preset| preset.slug())
            .collect();
        assert_eq!(names.len(), 5);
        assert_eq!(slugs.len(), 5);
    }

    #[test]
    fn each_exact_slug_parses() {
        for preset in BuiltinPreset::all() {
            assert_eq!(BuiltinPreset::from_slug(preset.slug()).unwrap(), preset);
            assert_eq!(preset.slug().parse::<BuiltinPreset>().unwrap(), preset);
        }
    }

    #[test]
    fn unknown_slug_rejected_typed() {
        assert!(matches!(
            BuiltinPreset::from_slug("ultra"),
            Err(BuiltinPresetError::UnknownSlug(_))
        ));
    }

    #[test]
    fn uppercase_slug_rejected() {
        assert!(BuiltinPreset::from_slug("Gaming").is_err());
    }

    #[test]
    fn whitespace_slug_rejected() {
        assert!(BuiltinPreset::from_slug(" gaming").is_err());
        assert!(BuiltinPreset::from_slug("gaming ").is_err());
    }

    #[test]
    fn path_like_slug_rejected() {
        assert!(BuiltinPreset::from_slug("../gaming").is_err());
        assert!(BuiltinPreset::from_slug("gaming/../x").is_err());
    }

    #[test]
    fn empty_slug_rejected() {
        assert!(matches!(
            BuiltinPreset::from_slug(""),
            Err(BuiltinPresetError::UnknownSlug(_))
        ));
    }

    #[test]
    fn balanced_resolves_full_recipe() {
        let profile = BuiltinPreset::Balanced
            .resolve(&full_capabilities())
            .unwrap();
        assert_eq!(profile.name().as_str(), "Balanced");
        let performance = profile.performance();
        assert_eq!(
            performance.shift_mode().map(ShiftMode::as_str),
            Some("comfort")
        );
        assert_eq!(performance.fan_mode().map(FanMode::as_str), Some("auto"));
        assert_eq!(performance.cooler_boost(), Some(false));
        assert_eq!(performance.super_battery(), Some(false));
    }

    #[test]
    fn balanced_omits_unadvertised_fields() {
        let capabilities = Capabilities {
            shift_modes: vec![ShiftMode::try_from("comfort").unwrap()],
            ..Capabilities::default()
        };
        let profile = BuiltinPreset::Balanced.resolve(&capabilities).unwrap();
        let performance = profile.performance();
        assert_eq!(
            performance.shift_mode().map(ShiftMode::as_str),
            Some("comfort")
        );
        assert_eq!(performance.fan_mode(), None);
        assert_eq!(performance.cooler_boost(), None);
        assert_eq!(performance.super_battery(), None);
    }

    #[test]
    fn silent_resolves_without_super_battery() {
        let profile = BuiltinPreset::Silent.resolve(&full_capabilities()).unwrap();
        let performance = profile.performance();
        assert_eq!(performance.shift_mode().map(ShiftMode::as_str), Some("eco"));
        assert_eq!(performance.fan_mode().map(FanMode::as_str), Some("silent"));
        assert_eq!(performance.cooler_boost(), Some(false));
        assert_eq!(performance.super_battery(), None);
    }

    #[test]
    fn silent_omits_missing_modes() {
        let capabilities = Capabilities {
            cooler_boost: true,
            ..Capabilities::default()
        };
        let profile = BuiltinPreset::Silent.resolve(&capabilities).unwrap();
        assert_eq!(profile.performance().shift_mode(), None);
        assert_eq!(profile.performance().fan_mode(), None);
        assert_eq!(profile.performance().cooler_boost(), Some(false));
    }

    #[test]
    fn gaming_resolves_full_recipe() {
        let profile = BuiltinPreset::Gaming.resolve(&full_capabilities()).unwrap();
        let performance = profile.performance();
        assert_eq!(
            performance.shift_mode().map(ShiftMode::as_str),
            Some("turbo")
        );
        assert_eq!(
            performance.fan_mode().map(FanMode::as_str),
            Some("advanced")
        );
        assert_eq!(performance.cooler_boost(), Some(true));
        assert_eq!(performance.super_battery(), Some(false));
    }

    #[test]
    fn gaming_omits_unadvertised_advanced() {
        let capabilities = Capabilities {
            fan_modes: vec![FanMode::try_from("auto").unwrap()],
            shift_modes: vec![ShiftMode::try_from("turbo").unwrap()],
            cooler_boost: true,
            ..Capabilities::default()
        };
        let profile = BuiltinPreset::Gaming.resolve(&capabilities).unwrap();
        let performance = profile.performance();
        assert_eq!(
            performance.shift_mode().map(ShiftMode::as_str),
            Some("turbo")
        );
        assert_eq!(performance.fan_mode(), None);
        assert_eq!(performance.cooler_boost(), Some(true));
        // super_battery capability is absent: omitted, not requested false.
        assert_eq!(performance.super_battery(), None);
    }

    #[test]
    fn gaming_omits_unadvertised_turbo() {
        let capabilities = Capabilities {
            shift_modes: vec![ShiftMode::try_from("eco").unwrap()],
            fan_modes: vec![FanMode::try_from("advanced").unwrap()],
            cooler_boost: true,
            super_battery: true,
            ..Capabilities::default()
        };
        let profile = BuiltinPreset::Gaming.resolve(&capabilities).unwrap();
        assert_eq!(profile.performance().shift_mode(), None);
        assert_eq!(
            profile.performance().fan_mode().map(FanMode::as_str),
            Some("advanced")
        );
    }

    #[test]
    fn battery_saver_resolves_full_recipe() {
        let profile = BuiltinPreset::BatterySaver
            .resolve(&full_capabilities())
            .unwrap();
        let performance = profile.performance();
        assert_eq!(performance.shift_mode().map(ShiftMode::as_str), Some("eco"));
        assert_eq!(performance.fan_mode().map(FanMode::as_str), Some("silent"));
        assert_eq!(performance.cooler_boost(), Some(false));
        assert_eq!(performance.super_battery(), Some(true));
    }

    #[test]
    fn battery_saver_never_sets_thresholds() {
        let profile = BuiltinPreset::BatterySaver
            .resolve(&full_capabilities())
            .unwrap();
        assert_eq!(profile.battery().threshold(), None);
    }

    #[test]
    fn maximum_cooling_resolves_cooling_only() {
        let profile = BuiltinPreset::MaximumCooling
            .resolve(&full_capabilities())
            .unwrap();
        let performance = profile.performance();
        assert_eq!(
            performance.fan_mode().map(FanMode::as_str),
            Some("advanced")
        );
        assert_eq!(performance.cooler_boost(), Some(true));
        assert_eq!(performance.shift_mode(), None);
        assert_eq!(performance.super_battery(), None);
    }

    #[test]
    fn partial_capabilities_produce_partial_profile() {
        let capabilities = Capabilities {
            fan_modes: vec![FanMode::try_from("auto").unwrap()],
            ..Capabilities::default()
        };
        let profile = BuiltinPreset::Balanced.resolve(&capabilities).unwrap();
        assert_eq!(
            profile.performance().fan_mode().map(FanMode::as_str),
            Some("auto")
        );
        assert_eq!(profile.performance().shift_mode(), None);
        assert_eq!(profile.performance().cooler_boost(), None);
        assert_eq!(profile.performance().super_battery(), None);
    }

    #[test]
    fn zero_relevant_capabilities_fail_closed() {
        let error = BuiltinPreset::Gaming
            .resolve(&Capabilities::default())
            .expect_err("empty capabilities must fail closed");
        assert_eq!(
            error.to_string(),
            "built-in preset is unavailable on current capabilities: Gaming"
        );
        assert!(matches!(
            error,
            BuiltinPresetError::Unavailable(BuiltinPreset::Gaming)
        ));
    }

    #[test]
    fn resolved_profile_is_always_non_empty() {
        for preset in BuiltinPreset::all() {
            let profile = preset.resolve(&full_capabilities()).unwrap();
            let performance = profile.performance();
            assert!(
                performance.shift_mode().is_some()
                    || performance.fan_mode().is_some()
                    || performance.cooler_boost().is_some()
                    || performance.super_battery().is_some(),
                "{preset} resolved to no settings"
            );
        }
    }

    #[test]
    fn resolution_is_deterministic() {
        let capabilities = full_capabilities();
        for preset in BuiltinPreset::all() {
            assert_eq!(
                preset.resolve(&capabilities).unwrap(),
                preset.resolve(&capabilities).unwrap()
            );
        }
    }

    #[test]
    fn resolution_does_not_mutate_capabilities() {
        let capabilities = full_capabilities();
        let before = capabilities.clone();
        for preset in BuiltinPreset::all() {
            let _ = preset.resolve(&capabilities);
        }
        assert_eq!(capabilities, before);
    }

    #[test]
    fn no_preset_sets_battery_threshold() {
        for preset in BuiltinPreset::all() {
            let profile = preset.resolve(&full_capabilities()).unwrap();
            assert_eq!(profile.battery().threshold(), None, "{preset}");
        }
    }

    #[test]
    fn no_preset_sets_keyboard_backlight() {
        for preset in BuiltinPreset::all() {
            let profile = preset.resolve(&full_capabilities()).unwrap();
            assert_eq!(profile.device().keyboard_backlight(), None, "{preset}");
        }
    }

    #[test]
    fn resolved_preview_applies_against_matching_capabilities() {
        let capabilities = full_capabilities();
        let profile = BuiltinPreset::Gaming.resolve(&capabilities).unwrap();
        let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &capabilities);
        assert!(preview.is_applicable());
    }

    #[test]
    fn later_capability_loss_rejects_resolved_profile() {
        let profile = BuiltinPreset::Gaming.resolve(&full_capabilities()).unwrap();
        let reduced = Capabilities {
            shift_modes: vec![ShiftMode::try_from("eco").unwrap()],
            fan_modes: vec![FanMode::try_from("auto").unwrap()],
            cooler_boost: true,
            super_battery: true,
            ..Capabilities::default()
        };
        let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &reduced);
        assert!(!preview.is_applicable());
    }

    #[test]
    fn resolution_grants_no_authorization() {
        // Compile-level proof: resolve() returns plain Profile data. Only a
        // later preview/apply against fresh state can authorize anything.
        let profile = BuiltinPreset::Silent.resolve(&full_capabilities()).unwrap();
        assert_eq!(profile.name().as_str(), "Silent");
    }

    #[test]
    fn purity_needs_no_backend_types() {
        // Compile-level proof: this test names no paths, readers, backends,
        // executors, or process types.
        let profile = BuiltinPreset::Balanced
            .resolve(&full_capabilities())
            .unwrap();
        assert_eq!(profile.name().as_str(), "Balanced");
    }

    #[test]
    fn preset_equality_hash_and_copy() {
        use std::collections::HashSet;

        let preset = BuiltinPreset::Gaming;
        let copied = preset;
        assert_eq!(preset, copied);
        assert_eq!(format!("{preset}"), "Gaming");
        let set: HashSet<BuiltinPreset> = [preset, copied].into_iter().collect();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn display_uses_exact_names() {
        assert_eq!(BuiltinPreset::BatterySaver.to_string(), "Battery Saver");
        assert_eq!(BuiltinPreset::MaximumCooling.to_string(), "Maximum Cooling");
    }

    #[test]
    fn error_display_is_stable() {
        assert_eq!(
            BuiltinPresetError::UnknownSlug("ultra".to_owned()).to_string(),
            "unknown built-in preset: ultra"
        );
    }
}
