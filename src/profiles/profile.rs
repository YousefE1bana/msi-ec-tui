//! Profile domain: strict TOML parsing into validated, data-only profiles.
//!
//! A [`Profile`] is data only. Parsing performs no I/O, no shell expansion,
//! no environment interpolation, and no hardware access: [`Profile::parse_toml`]
//! turns a TOML document into a typed profile, and a string that merely
//! looks shell-like stays inert data. Converting profiles into commands and
//! applying them belongs to later PLAN-005 slices.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use thiserror::Error;

use crate::hardware::{
    BatteryThreshold, BatteryThresholdError, FanMode, ModeValidationError, ShiftMode,
};

/// A validated profile: display name plus the subset of controls it sets.
/// Every section is optional; a profile must set at least one control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    name: ProfileName,
    performance: PerformanceProfile,
    battery: BatteryProfile,
    device: DeviceProfile,
}

/// A validated profile display name: non-empty, trimmed, no newline,
/// carriage return, or NUL, at most 64 UTF-8 bytes. Internal spaces are
/// allowed. Never a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProfileName(String);

/// Performance controls; every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerformanceProfile {
    shift_mode: Option<ShiftMode>,
    fan_mode: Option<FanMode>,
    cooler_boost: Option<bool>,
    super_battery: Option<bool>,
}

/// Battery controls; the threshold is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatteryProfile {
    threshold: Option<BatteryThreshold>,
}

/// Device controls; every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceProfile {
    keyboard_backlight: Option<u8>,
}

/// Why [`Profile::parse_toml`] failed: malformed TOML or a typed domain
/// violation. TOML syntax errors preserve the parser error.
#[derive(Debug, Error)]
pub enum ProfileParseError {
    /// The document is not valid TOML for the profile schema.
    #[error("invalid profile TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// The document parsed but violates profile domain rules.
    #[error("invalid profile: {0}")]
    Validation(#[from] ProfileValidationError),
}

/// Why a parsed profile document violates domain rules.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProfileValidationError {
    /// The profile name violates its invariant.
    #[error("invalid profile name: {0}")]
    InvalidName(#[from] ProfileNameError),
    /// The fan mode is syntactically invalid.
    #[error("invalid fan mode: {0}")]
    InvalidFanMode(ModeValidationError),
    /// The shift mode is syntactically invalid.
    #[error("invalid shift mode: {0}")]
    InvalidShiftMode(ModeValidationError),
    /// The battery end limit is not representable.
    #[error("invalid battery threshold: {0}")]
    InvalidBatteryThreshold(#[from] BatteryThresholdError),
    /// The profile describes no settings at all.
    #[error("profile describes no settings")]
    NoSettings,
}

/// Why a profile name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProfileNameError {
    /// The name is empty.
    #[error("profile name must not be empty")]
    Empty,
    /// The name has leading or trailing whitespace.
    #[error("profile name must already be trimmed")]
    Untrimmed,
    /// The name contains newline, carriage return, or NUL.
    #[error("profile name must not contain newline, carriage return, or NUL")]
    ForbiddenCharacter,
    /// The name exceeds 64 UTF-8 bytes.
    #[error("profile name must not exceed 64 UTF-8 bytes")]
    TooLong,
}

/// Private deserialization DTO: primitive TOML values only, never exposed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    name: String,
    #[serde(default)]
    performance: RawPerformanceProfile,
    #[serde(default)]
    battery: RawBatteryProfile,
    #[serde(default)]
    device: RawDeviceProfile,
}

/// Private deserialization DTO for `[performance]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawPerformanceProfile {
    shift_mode: Option<String>,
    fan_mode: Option<String>,
    cooler_boost: Option<bool>,
    super_battery: Option<bool>,
}

/// Private deserialization DTO for `[battery]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawBatteryProfile {
    charge_end_threshold: Option<u8>,
}

/// Private deserialization DTO for `[device]`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawDeviceProfile {
    keyboard_backlight: Option<u8>,
}

fn validate_profile_name(value: &str) -> Result<(), ProfileNameError> {
    if value.is_empty() {
        return Err(ProfileNameError::Empty);
    }
    if value.contains(['\n', '\r', '\0']) {
        return Err(ProfileNameError::ForbiddenCharacter);
    }
    if value.trim() != value {
        return Err(ProfileNameError::Untrimmed);
    }
    if value.len() > 64 {
        return Err(ProfileNameError::TooLong);
    }
    Ok(())
}

impl ProfileName {
    /// The display name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ProfileName {
    type Error = ProfileNameError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        validate_profile_name(value)?;
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for ProfileName {
    type Error = ProfileNameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        validate_profile_name(&value)?;
        Ok(Self(value))
    }
}

impl fmt::Display for ProfileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Profile {
    /// Parses a TOML profile document into a validated [`Profile`]. Pure:
    /// no filesystem, environment, shell, or hardware access.
    pub fn parse_toml(input: &str) -> Result<Self, ProfileParseError> {
        let raw: RawProfile = toml::from_str(input)?;
        let performance = PerformanceProfile {
            shift_mode: raw
                .performance
                .shift_mode
                .map(|mode| {
                    ShiftMode::try_from(mode.as_str())
                        .map_err(ProfileValidationError::InvalidShiftMode)
                })
                .transpose()?,
            fan_mode: raw
                .performance
                .fan_mode
                .map(|mode| {
                    FanMode::try_from(mode.as_str()).map_err(ProfileValidationError::InvalidFanMode)
                })
                .transpose()?,
            cooler_boost: raw.performance.cooler_boost,
            super_battery: raw.performance.super_battery,
        };
        let battery = BatteryProfile {
            threshold: raw
                .battery
                .charge_end_threshold
                .map(BatteryThreshold::from_end_percent)
                .transpose()
                .map_err(ProfileValidationError::InvalidBatteryThreshold)?,
        };
        let device = DeviceProfile {
            keyboard_backlight: raw.device.keyboard_backlight,
        };
        if performance.is_empty() && battery.is_empty() && device.is_empty() {
            return Err(ProfileValidationError::NoSettings.into());
        }
        Ok(Self {
            name: ProfileName::try_from(raw.name).map_err(ProfileValidationError::InvalidName)?,
            performance,
            battery,
            device,
        })
    }

    /// The profile display name.
    pub fn name(&self) -> &ProfileName {
        &self.name
    }

    /// The performance section (possibly empty).
    pub fn performance(&self) -> &PerformanceProfile {
        &self.performance
    }

    /// The battery section (possibly empty).
    pub fn battery(&self) -> &BatteryProfile {
        &self.battery
    }

    /// The device section (possibly empty).
    pub fn device(&self) -> &DeviceProfile {
        &self.device
    }
}

impl FromStr for Profile {
    type Err = ProfileParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::parse_toml(input)
    }
}

impl PerformanceProfile {
    /// Desired shift mode, if set.
    pub fn shift_mode(&self) -> Option<&ShiftMode> {
        self.shift_mode.as_ref()
    }

    /// Desired fan mode, if set.
    pub fn fan_mode(&self) -> Option<&FanMode> {
        self.fan_mode.as_ref()
    }

    /// Desired cooler boost state, if set.
    pub fn cooler_boost(&self) -> Option<bool> {
        self.cooler_boost
    }

    /// Desired super battery state, if set.
    pub fn super_battery(&self) -> Option<bool> {
        self.super_battery
    }

    fn is_empty(&self) -> bool {
        self.shift_mode.is_none()
            && self.fan_mode.is_none()
            && self.cooler_boost.is_none()
            && self.super_battery.is_none()
    }
}

impl BatteryProfile {
    /// Desired charge threshold pair, if set.
    pub fn threshold(&self) -> Option<BatteryThreshold> {
        self.threshold
    }

    fn is_empty(&self) -> bool {
        self.threshold.is_none()
    }
}

impl DeviceProfile {
    /// Desired keyboard backlight level, if set.
    pub fn keyboard_backlight(&self) -> Option<u8> {
        self.keyboard_backlight
    }

    fn is_empty(&self) -> bool {
        self.keyboard_backlight.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn parse(input: &str) -> Profile {
        Profile::parse_toml(input).unwrap()
    }

    #[test]
    fn canonical_gaming_toml_parses_successfully() {
        assert!(Profile::parse_toml(GAMING_TOML).is_ok());
    }

    #[test]
    fn canonical_name_preserved_exactly() {
        assert_eq!(parse(GAMING_TOML).name().as_str(), "Gaming");
    }

    #[test]
    fn canonical_performance_values() {
        let performance = parse(GAMING_TOML).performance().clone();
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
    fn canonical_battery_end_becomes_typed_pair() {
        let threshold = parse(GAMING_TOML).battery().threshold().unwrap();
        assert_eq!(threshold.start_percent(), 70);
        assert_eq!(threshold.end_percent(), 80);
    }

    #[test]
    fn canonical_backlight_level() {
        assert_eq!(parse(GAMING_TOML).device().keyboard_backlight(), Some(2));
    }

    #[test]
    fn performance_only_profile_parses() {
        let profile = parse("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n");
        assert_eq!(profile.name().as_str(), "Quiet");
        assert_eq!(
            profile.performance().fan_mode().map(FanMode::as_str),
            Some("silent")
        );
        assert_eq!(profile.performance().shift_mode(), None);
        assert_eq!(profile.performance().cooler_boost(), None);
        assert_eq!(profile.performance().super_battery(), None);
        assert_eq!(profile.battery().threshold(), None);
        assert_eq!(profile.device().keyboard_backlight(), None);
    }

    #[test]
    fn battery_only_profile_parses() {
        let profile = parse("name = \"Battery Saver\"\n\n[battery]\ncharge_end_threshold = 80\n");
        assert_eq!(profile.battery().threshold().unwrap().end_percent(), 80);
        assert!(profile.performance().is_empty());
        assert!(profile.device().is_empty());
    }

    #[test]
    fn device_only_profile_parses() {
        let profile = parse("name = \"Glow\"\n\n[device]\nkeyboard_backlight = 3\n");
        assert_eq!(profile.device().keyboard_backlight(), Some(3));
        assert!(profile.performance().is_empty());
        assert!(profile.battery().is_empty());
    }

    #[test]
    fn omitted_sections_expose_no_settings() {
        let profile = parse("name = \"Minimal\"\n\n[device]\nkeyboard_backlight = 1\n");
        assert!(profile.performance().is_empty());
        assert!(profile.battery().is_empty());
        assert_eq!(profile.device().keyboard_backlight(), Some(1));
    }

    #[test]
    fn normal_name_accepted() {
        assert_eq!(ProfileName::try_from("Gaming").unwrap().as_str(), "Gaming");
    }

    #[test]
    fn internal_spaces_accepted() {
        for name in ["Battery Saver", "Maximum Cooling"] {
            assert_eq!(ProfileName::try_from(name).unwrap().as_str(), name);
        }
    }

    #[test]
    fn empty_name_rejected() {
        assert_eq!(ProfileName::try_from(""), Err(ProfileNameError::Empty));
    }

    #[test]
    fn leading_whitespace_rejected() {
        assert_eq!(
            ProfileName::try_from(" Gaming"),
            Err(ProfileNameError::Untrimmed)
        );
    }

    #[test]
    fn trailing_whitespace_rejected() {
        assert_eq!(
            ProfileName::try_from("Gaming "),
            Err(ProfileNameError::Untrimmed)
        );
    }

    #[test]
    fn newline_rejected() {
        assert_eq!(
            ProfileName::try_from("Gaming\nProfile"),
            Err(ProfileNameError::ForbiddenCharacter)
        );
    }

    #[test]
    fn carriage_return_rejected() {
        assert_eq!(
            ProfileName::try_from("Gaming\rProfile"),
            Err(ProfileNameError::ForbiddenCharacter)
        );
    }

    #[test]
    fn nul_rejected() {
        assert_eq!(
            ProfileName::try_from("Gaming\0Profile"),
            Err(ProfileNameError::ForbiddenCharacter)
        );
    }

    #[test]
    fn exactly_64_bytes_accepted() {
        assert_eq!("a".repeat(64).len(), 64);
        assert!(ProfileName::try_from("a".repeat(64)).is_ok());
        assert!(ProfileName::try_from("é".repeat(32)).is_ok());
    }

    #[test]
    fn over_64_bytes_rejected() {
        assert_eq!(
            ProfileName::try_from("a".repeat(65)),
            Err(ProfileNameError::TooLong)
        );
        assert_eq!(
            ProfileName::try_from("é".repeat(33)),
            Err(ProfileNameError::TooLong)
        );
    }

    #[test]
    fn unknown_top_level_field_rejected() {
        let input = "name = \"Gaming\"\nunknown = true\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn unknown_performance_field_rejected() {
        let input = "name = \"Gaming\"\n\n[performance]\nfan_mode = \"auto\"\nraw_ec = \"0x98\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn unknown_battery_field_rejected() {
        let input = "name = \"Gaming\"\n\n[battery]\ncharge_end_threshold = 80\nstart = 70\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn unknown_device_field_rejected() {
        let input =
            "name = \"Gaming\"\n\n[device]\nkeyboard_backlight = 2\ncommand = \"sudo something\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn wrong_boolean_type_rejected() {
        let input = "name = \"Gaming\"\n\n[performance]\ncooler_boost = \"yes\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn wrong_integer_type_rejected() {
        let input = "name = \"Gaming\"\n\n[device]\nkeyboard_backlight = \"high\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn malformed_toml_rejected() {
        assert!(matches!(
            Profile::parse_toml("name = [unclosed\n"),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn duplicate_key_rejected() {
        let input = "name = \"Gaming\"\nname = \"Gaming\"\n\n[performance]\nfan_mode = \"auto\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn known_fan_mode_parses() {
        let profile = parse("name = \"A\"\n\n[performance]\nfan_mode = \"auto\"\n");
        assert_eq!(
            profile.performance().fan_mode().map(FanMode::as_str),
            Some("auto")
        );
    }

    #[test]
    fn future_fan_mode_parses() {
        let profile = parse("name = \"A\"\n\n[performance]\nfan_mode = \"hyper-boost\"\n");
        assert_eq!(
            profile.performance().fan_mode().map(FanMode::as_str),
            Some("hyper-boost")
        );
    }

    #[test]
    fn invalid_fan_syntax_is_typed_error() {
        let input = "name = \"A\"\n\n[performance]\nfan_mode = \"\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Validation(
                ProfileValidationError::InvalidFanMode(ModeValidationError::Empty)
            ))
        ));
    }

    #[test]
    fn known_shift_mode_parses() {
        let profile = parse("name = \"A\"\n\n[performance]\nshift_mode = \"comfort\"\n");
        assert_eq!(
            profile.performance().shift_mode().map(ShiftMode::as_str),
            Some("comfort")
        );
    }

    #[test]
    fn future_shift_mode_parses() {
        let profile = parse("name = \"A\"\n\n[performance]\nshift_mode = \"ultra-turbo\"\n");
        assert_eq!(
            profile.performance().shift_mode().map(ShiftMode::as_str),
            Some("ultra-turbo")
        );
    }

    #[test]
    fn invalid_shift_syntax_is_typed_error() {
        let input = "name = \"A\"\n\n[performance]\nshift_mode = \" turbo\"\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Validation(
                ProfileValidationError::InvalidShiftMode(ModeValidationError::Untrimmed)
            ))
        ));
    }

    #[test]
    fn battery_end_10_becomes_pair() {
        let profile = parse("name = \"A\"\n\n[battery]\ncharge_end_threshold = 10\n");
        let threshold = profile.battery().threshold().unwrap();
        assert_eq!(
            (threshold.start_percent(), threshold.end_percent()),
            (0, 10)
        );
    }

    #[test]
    fn battery_end_60_becomes_pair() {
        let profile = parse("name = \"A\"\n\n[battery]\ncharge_end_threshold = 60\n");
        let threshold = profile.battery().threshold().unwrap();
        assert_eq!(
            (threshold.start_percent(), threshold.end_percent()),
            (50, 60)
        );
    }

    #[test]
    fn battery_end_80_becomes_pair() {
        let profile = parse("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n");
        let threshold = profile.battery().threshold().unwrap();
        assert_eq!(
            (threshold.start_percent(), threshold.end_percent()),
            (70, 80)
        );
    }

    #[test]
    fn battery_end_100_becomes_pair() {
        let profile = parse("name = \"A\"\n\n[battery]\ncharge_end_threshold = 100\n");
        let threshold = profile.battery().threshold().unwrap();
        assert_eq!(
            (threshold.start_percent(), threshold.end_percent()),
            (90, 100)
        );
    }

    #[test]
    fn battery_end_9_rejected() {
        let input = "name = \"A\"\n\n[battery]\ncharge_end_threshold = 9\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Validation(
                ProfileValidationError::InvalidBatteryThreshold(
                    BatteryThresholdError::EndOutOfRange(9)
                )
            ))
        ));
    }

    #[test]
    fn battery_end_101_rejected() {
        // 101 exceeds u8 range, so TOML decoding itself refuses it.
        let input = "name = \"A\"\n\n[battery]\ncharge_end_threshold = 101\n";
        assert!(Profile::parse_toml(input).is_err());
    }

    #[test]
    fn backlight_level_0_parses() {
        let profile = parse("name = \"A\"\n\n[device]\nkeyboard_backlight = 0\n");
        assert_eq!(profile.device().keyboard_backlight(), Some(0));
    }

    #[test]
    fn backlight_level_255_parses() {
        let profile = parse("name = \"A\"\n\n[device]\nkeyboard_backlight = 255\n");
        assert_eq!(profile.device().keyboard_backlight(), Some(255));
    }

    #[test]
    fn backlight_above_u8_rejected_by_parsing() {
        let input = "name = \"A\"\n\n[device]\nkeyboard_backlight = 300\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Toml(_))
        ));
    }

    #[test]
    fn parser_invents_no_capability_validation() {
        // Level 200 may exceed every real device maximum; the parser still
        // accepts it. Capability checks belong to preview/apply.
        let profile = parse("name = \"A\"\n\n[device]\nkeyboard_backlight = 200\n");
        assert_eq!(profile.device().keyboard_backlight(), Some(200));
    }

    #[test]
    fn name_only_profile_rejected_as_no_settings() {
        assert!(matches!(
            Profile::parse_toml("name = \"Empty\"\n"),
            Err(ProfileParseError::Validation(
                ProfileValidationError::NoSettings
            ))
        ));
    }

    #[test]
    fn empty_sections_rejected_as_no_settings() {
        let input = "name = \"Empty\"\n\n[performance]\n\n[battery]\n\n[device]\n";
        assert!(matches!(
            Profile::parse_toml(input),
            Err(ProfileParseError::Validation(
                ProfileValidationError::NoSettings
            ))
        ));
    }

    #[test]
    fn shell_looking_mode_is_data_only() {
        let profile = parse("name = \"A\"\n\n[performance]\nfan_mode = \"$(id)\"\n");
        assert_eq!(
            profile.performance().fan_mode().map(FanMode::as_str),
            Some("$(id)")
        );
    }

    #[test]
    fn parsing_is_pure_and_repeatable() {
        // No filesystem, environment, shell, or hardware access: the same
        // document always yields the same profile.
        let first = Profile::parse_toml(GAMING_TOML).unwrap();
        let second = Profile::parse_toml(GAMING_TOML).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn clone_and_equality_work() {
        let profile = parse(GAMING_TOML);
        assert_eq!(profile, profile.clone());
        assert_ne!(
            parse(GAMING_TOML),
            parse("name = \"Other\"\n\n[device]\nkeyboard_backlight = 2\n")
        );
    }

    #[test]
    fn accessors_preserve_exact_typed_values() {
        let profile = parse(GAMING_TOML);
        assert_eq!(profile.name().to_string(), "Gaming");
        assert_eq!(profile.battery().threshold(), Some(threshold_70_80()));
    }

    #[test]
    fn parsing_does_not_mutate_input() {
        let input = String::from(GAMING_TOML);
        let before = input.clone();
        let _ = Profile::parse_toml(&input).unwrap();
        assert_eq!(input, before);
    }

    #[test]
    fn error_display_is_stable_and_human_readable() {
        assert_eq!(
            ProfileValidationError::NoSettings.to_string(),
            "profile describes no settings"
        );
        assert_eq!(
            ProfileValidationError::InvalidName(ProfileNameError::TooLong).to_string(),
            "invalid profile name: profile name must not exceed 64 UTF-8 bytes"
        );
        assert!(
            !ProfileParseError::Validation(ProfileValidationError::NoSettings)
                .to_string()
                .is_empty()
        );
    }

    fn threshold_70_80() -> BatteryThreshold {
        BatteryThreshold::new(70, 80).unwrap()
    }
}
