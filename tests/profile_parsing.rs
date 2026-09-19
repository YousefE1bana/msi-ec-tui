//! Profile parsing through the public API only: no hardware, no disk.

use mec::hardware::{BatteryThreshold, FanMode, ShiftMode};
use mec::profiles::{Profile, ProfileParseError, ProfileValidationError};

const GAMING_TOML: &str = concat!(
    "name = \"Gaming\"\n",
    "\n",
    "[performance]\n",
    "shift_mode = \"turbo\"\n",
    "fan_mode = \"advanced\"\n",
    "cooler_boost = true\n",
    "super_battery = false\n",
    "\n",
    "[battery]\n",
    "charge_end_threshold = 80\n",
    "\n",
    "[device]\n",
    "keyboard_backlight = 2\n",
);

#[test]
fn canonical_profile_parses_through_public_api() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    assert_eq!(profile.name().as_str(), "Gaming");
    assert_eq!(
        profile.performance().shift_mode().map(ShiftMode::as_str),
        Some("turbo")
    );
    assert_eq!(
        profile.performance().fan_mode().map(FanMode::as_str),
        Some("advanced")
    );
    assert_eq!(profile.performance().cooler_boost(), Some(true));
    assert_eq!(profile.performance().super_battery(), Some(false));
    assert_eq!(
        profile.battery().threshold(),
        Some(BatteryThreshold::new(70, 80).unwrap())
    );
    assert_eq!(profile.device().keyboard_backlight(), Some(2));
}

#[test]
fn partial_profile_parses_through_public_api() {
    let profile: Profile = "name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n"
        .parse()
        .unwrap();
    assert_eq!(
        profile.performance().fan_mode().map(FanMode::as_str),
        Some("silent")
    );
    assert_eq!(profile.battery().threshold(), None);
    assert_eq!(profile.device().keyboard_backlight(), None);
}

#[test]
fn unknown_field_rejected_through_public_api() {
    let error = "name = \"Gaming\"\nwebcam = true\n"
        .parse::<Profile>()
        .expect_err("unknown top-level field must be rejected");
    assert!(matches!(error, ProfileParseError::Toml(_)));
}

#[test]
fn terminal_control_characters_rejected_in_name_and_modes() {
    for input in [
        "name = \"Gam\x1bing\"\n\n[device]\nkeyboard_backlight = 1\n",
        "name = \"A\"\n\n[performance]\nfan_mode = \"au\x1bto\"\n",
        "name = \"A\"\n\n[performance]\nshift_mode = \"eco\x7f\"\n",
        "name = \"A\"\n\n[performance]\nfan_mode = \"au\tto\"\n",
        "name = \"A\"\n\n[performance]\nshift_mode = \"\u{7f}eco\"\n",
    ] {
        assert!(
            input.parse::<Profile>().is_err(),
            "{input:?} must be rejected"
        );
    }
}

#[test]
fn ordinary_future_modes_remain_valid() {
    for mode in ["future-mode", "vendor_mode_2", "future-mode_v2"] {
        let fan: FanMode = mode.try_into().unwrap();
        assert_eq!(fan.as_str(), mode);
        let shift: ShiftMode = mode.try_into().unwrap();
        assert_eq!(shift.as_str(), mode);
        let input = format!("name = \"A\"\n\n[performance]\nfan_mode = \"{mode}\"\n");
        assert!(input.parse::<Profile>().is_ok());
    }
}

#[test]
fn empty_profile_rejected_as_no_settings() {
    let error = "name = \"Empty\"\n"
        .parse::<Profile>()
        .expect_err("name-only profile must be rejected");
    assert!(matches!(
        error,
        ProfileParseError::Validation(ProfileValidationError::NoSettings)
    ));
}

#[test]
fn invalid_battery_rejected_through_public_api() {
    let error = "name = \"A\"\n\n[battery]\ncharge_end_threshold = 9\n"
        .parse::<Profile>()
        .expect_err("unrepresentable end limit must be rejected");
    assert!(matches!(
        error,
        ProfileParseError::Validation(ProfileValidationError::InvalidBatteryThreshold(_))
    ));
}

#[test]
fn shell_looking_values_stay_data() {
    let profile: Profile = "name = \"A\"\n\n[performance]\nfan_mode = \"$(id)\"\n"
        .parse()
        .unwrap();
    assert_eq!(
        profile.performance().fan_mode().map(FanMode::as_str),
        Some("$(id)")
    );
}

#[test]
fn profiles_clone_and_compare() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    assert_eq!(profile, profile.clone());
}
