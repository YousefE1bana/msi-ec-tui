//! Built-in presets through the public API only: no filesystem, no TempDir.

use mec::hardware::{BacklightCapability, Capabilities, FanMode, ShiftMode, SupportMode};
use mec::profiles::{BuiltinPreset, BuiltinPresetError, ProfilePlanner};

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
fn catalog_enumerates_five_stable_presets() {
    assert_eq!(
        BuiltinPreset::all().map(|preset| (preset.name(), preset.slug())),
        [
            ("Balanced", "balanced"),
            ("Silent", "silent"),
            ("Gaming", "gaming"),
            ("Battery Saver", "battery-saver"),
            ("Maximum Cooling", "maximum-cooling"),
        ]
    );
}

#[test]
fn gaming_resolves_fully_and_previews_applicable() {
    let capabilities = full_capabilities();
    let profile = BuiltinPreset::Gaming.resolve(&capabilities).unwrap();
    assert_eq!(profile.name().as_str(), "Gaming");
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &capabilities);
    assert_eq!(preview.entries().len(), 4);
    assert!(preview.is_applicable());
}

#[test]
fn partial_gaming_omits_unadvertised_fields() {
    let capabilities = Capabilities {
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
    assert_eq!(performance.super_battery(), None);
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &capabilities);
    assert!(preview.is_applicable());
}

#[test]
fn unavailable_preset_returns_typed_error() {
    let error = BuiltinPreset::MaximumCooling
        .resolve(&Capabilities::default())
        .expect_err("no cooling capability must fail closed");
    assert!(matches!(
        error,
        BuiltinPresetError::Unavailable(BuiltinPreset::MaximumCooling)
    ));
}

#[test]
fn no_preset_includes_battery_or_backlight() {
    for preset in BuiltinPreset::all() {
        let profile = preset.resolve(&full_capabilities()).unwrap();
        assert_eq!(profile.battery().threshold(), None, "{preset}");
        assert_eq!(profile.device().keyboard_backlight(), None, "{preset}");
    }
}

#[test]
fn slugs_round_trip_through_lookup() {
    for preset in BuiltinPreset::all() {
        assert_eq!(BuiltinPreset::from_slug(preset.slug()).unwrap(), preset);
    }
    assert!(matches!(
        BuiltinPreset::from_slug("turbo"),
        Err(BuiltinPresetError::UnknownSlug(_))
    ));
}
