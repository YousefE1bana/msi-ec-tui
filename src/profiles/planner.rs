//! Pure profile planning: compile a profile into validated command intents.
//!
//! [`ProfilePlanner::preview`] is deterministic transformation plus pure
//! validation only: no I/O, no discovery, no evaluation, no writes. A
//! [`ProfilePreview`] describes what a profile requests and how each request
//! fares against the supplied [`SupportMode`] and [`Capabilities`] data. A
//! preview is NOT an authorization token: it must never approve future
//! execution, which always requires fresh support/capability evaluation
//! immediately before any write.

use crate::hardware::{Capabilities, CommandValidationError, HardwareCommand, SupportMode};

use super::{Profile, ProfileName};

/// Pure planner: profiles in, validated command intents out. Holds no
/// state; every call depends only on its arguments.
pub struct ProfilePlanner;

impl ProfilePlanner {
    /// Compiles `profile` into typed [`HardwareCommand`] requests in fixed
    /// order and validates each one with [`HardwareCommand::validate`]
    /// against the supplied `mode` and `capabilities`. All entries are
    /// evaluated; validation never stops at the first rejection.
    pub fn preview(
        profile: &Profile,
        mode: &SupportMode,
        capabilities: &Capabilities,
    ) -> ProfilePreview {
        // Fixed stable order: performance, then battery, then device.
        // Only explicitly specified settings become commands.
        let mut commands = Vec::new();
        let performance = profile.performance();
        if let Some(mode) = performance.shift_mode() {
            commands.push(HardwareCommand::SetShiftMode(mode.clone()));
        }
        if let Some(mode) = performance.fan_mode() {
            commands.push(HardwareCommand::SetFanMode(mode.clone()));
        }
        if let Some(value) = performance.cooler_boost() {
            commands.push(HardwareCommand::SetCoolerBoost(value));
        }
        if let Some(value) = performance.super_battery() {
            commands.push(HardwareCommand::SetSuperBattery(value));
        }
        if let Some(threshold) = profile.battery().threshold() {
            commands.push(HardwareCommand::SetBatteryThreshold(threshold));
        }
        if let Some(level) = profile.device().keyboard_backlight() {
            commands.push(HardwareCommand::SetKeyboardBacklight(level));
        }
        let entries = commands
            .into_iter()
            .map(|command| {
                let status = match command.validate(mode, capabilities) {
                    Ok(()) => ProfilePreviewStatus::Applicable,
                    Err(error) => ProfilePreviewStatus::Rejected(error),
                };
                ProfilePreviewEntry { command, status }
            })
            .collect();
        ProfilePreview {
            name: profile.name().clone(),
            entries,
        }
    }
}

/// What a profile requests and how each request validates: descriptive
/// only, never an authorization for future execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePreview {
    name: ProfileName,
    entries: Vec<ProfilePreviewEntry>,
}

/// One requested command plus its pure validation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfilePreviewEntry {
    command: HardwareCommand,
    status: ProfilePreviewStatus,
}

/// Pure validation outcome for one requested command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfilePreviewStatus {
    /// The command passes validation against the supplied data.
    Applicable,
    /// The command fails validation with the authoritative hardware error.
    Rejected(CommandValidationError),
}

impl ProfilePreview {
    /// Profile display name, preserved from the parsed profile.
    pub fn name(&self) -> &ProfileName {
        &self.name
    }

    /// Preview entries in deterministic command order.
    pub fn entries(&self) -> &[ProfilePreviewEntry] {
        &self.entries
    }

    /// True only when every entry is applicable.
    pub fn is_applicable(&self) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.status.is_applicable())
    }
}

impl ProfilePreviewEntry {
    /// The requested typed hardware command.
    pub fn command(&self) -> &HardwareCommand {
        &self.command
    }

    /// The pure validation outcome for this command.
    pub fn status(&self) -> &ProfilePreviewStatus {
        &self.status
    }
}

impl ProfilePreviewStatus {
    /// True when the command passes validation.
    pub fn is_applicable(&self) -> bool {
        matches!(self, Self::Applicable)
    }

    /// The validation error, if the command was rejected.
    pub fn error(&self) -> Option<&CommandValidationError> {
        match self {
            Self::Applicable => None,
            Self::Rejected(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{
        BacklightCapability, BatteryThreshold, FanMode, ReadOnlyReason, ShiftMode,
    };
    use crate::profiles::Profile;

    const CANONICAL_TOML: &str = concat!(
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

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    fn canonical() -> Profile {
        Profile::parse_toml(CANONICAL_TOML).unwrap()
    }

    /// Capabilities under which the canonical profile fully applies.
    fn full_capabilities() -> Capabilities {
        Capabilities {
            fan_modes: vec![fan("advanced"), fan("auto")],
            shift_modes: vec![shift("turbo"), shift("comfort")],
            cooler_boost: true,
            super_battery: true,
            keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
            battery_thresholds: true,
            ..Capabilities::default()
        }
    }

    fn ready_preview(profile: &Profile, capabilities: &Capabilities) -> ProfilePreview {
        ProfilePlanner::preview(profile, &SupportMode::Ready, capabilities)
    }

    fn commands(preview: &ProfilePreview) -> Vec<&HardwareCommand> {
        preview
            .entries()
            .iter()
            .map(|entry| entry.command())
            .collect()
    }

    #[test]
    fn canonical_profile_creates_exactly_six_entries() {
        let preview = ready_preview(&canonical(), &full_capabilities());
        assert_eq!(preview.entries().len(), 6);
    }

    #[test]
    fn entries_follow_deterministic_command_order() {
        let preview = ready_preview(&canonical(), &full_capabilities());
        let entries = preview.entries();
        assert!(matches!(
            entries[0].command(),
            HardwareCommand::SetShiftMode(_)
        ));
        assert!(matches!(
            entries[1].command(),
            HardwareCommand::SetFanMode(_)
        ));
        assert!(matches!(
            entries[2].command(),
            HardwareCommand::SetCoolerBoost(_)
        ));
        assert!(matches!(
            entries[3].command(),
            HardwareCommand::SetSuperBattery(_)
        ));
        assert!(matches!(
            entries[4].command(),
            HardwareCommand::SetBatteryThreshold(_)
        ));
        assert!(matches!(
            entries[5].command(),
            HardwareCommand::SetKeyboardBacklight(_)
        ));
    }

    #[test]
    fn exact_values_survive_mapping() {
        let preview = ready_preview(&canonical(), &full_capabilities());
        assert_eq!(
            commands(&preview),
            vec![
                &HardwareCommand::SetShiftMode(shift("turbo")),
                &HardwareCommand::SetFanMode(fan("advanced")),
                &HardwareCommand::SetCoolerBoost(true),
                &HardwareCommand::SetSuperBattery(false),
                &HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap()),
                &HardwareCommand::SetKeyboardBacklight(2),
            ]
        );
    }

    #[test]
    fn toml_key_ordering_does_not_change_command_order() {
        let reordered = concat!(
            "name = \"Gaming\"\n",
            "\n",
            "[device]\n",
            "keyboard_backlight = 2\n",
            "\n",
            "[battery]\n",
            "charge_end_threshold = 80\n",
            "\n",
            "[performance]\n",
            "super_battery = false\n",
            "cooler_boost = true\n",
            "fan_mode = \"advanced\"\n",
            "shift_mode = \"turbo\"\n",
        );
        let preview = ready_preview(
            &Profile::parse_toml(reordered).unwrap(),
            &full_capabilities(),
        );
        assert_eq!(
            commands(&preview),
            commands(&ready_preview(&canonical(), &full_capabilities()))
        );
    }

    #[test]
    fn fan_only_profile_creates_exactly_one_command() {
        let profile =
            Profile::parse_toml("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                fan_modes: vec![fan("silent")],
                ..Capabilities::default()
            },
        );
        assert_eq!(
            commands(&preview),
            vec![&HardwareCommand::SetFanMode(fan("silent"))]
        );
    }

    #[test]
    fn battery_only_creates_exactly_threshold_command() {
        let profile =
            Profile::parse_toml("name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 60\n")
                .unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        assert_eq!(
            commands(&preview),
            vec![&HardwareCommand::SetBatteryThreshold(
                BatteryThreshold::new(50, 60).unwrap()
            )]
        );
    }

    #[test]
    fn device_only_creates_exactly_backlight_command() {
        let profile =
            Profile::parse_toml("name = \"Glow\"\n\n[device]\nkeyboard_backlight = 1\n").unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
                ..Capabilities::default()
            },
        );
        assert_eq!(
            commands(&preview),
            vec![&HardwareCommand::SetKeyboardBacklight(1)]
        );
    }

    #[test]
    fn missing_booleans_create_no_command() {
        let profile =
            Profile::parse_toml("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap();
        let preview = ready_preview(&profile, &full_capabilities());
        assert_eq!(preview.entries().len(), 1);
        assert!(!commands(&preview).iter().any(|command| matches!(
            command,
            HardwareCommand::SetCoolerBoost(_) | HardwareCommand::SetSuperBattery(_)
        )));
    }

    #[test]
    fn omitted_sections_create_no_command() {
        let profile =
            Profile::parse_toml("name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 60\n")
                .unwrap();
        let preview = ready_preview(&profile, &full_capabilities());
        assert_eq!(preview.entries().len(), 1);
    }

    #[test]
    fn no_implicit_settings_are_invented() {
        let profile =
            Profile::parse_toml("name = \"Glow\"\n\n[device]\nkeyboard_backlight = 1\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(preview.entries().len(), 1);
        assert_eq!(
            commands(&preview),
            vec![&HardwareCommand::SetKeyboardBacklight(1)]
        );
    }

    #[test]
    fn fully_capable_ready_device_is_fully_applicable() {
        let preview = ready_preview(&canonical(), &full_capabilities());
        assert!(preview.is_applicable());
        assert!(
            preview
                .entries()
                .iter()
                .all(|entry| entry.status().is_applicable())
        );
        assert!(
            preview
                .entries()
                .iter()
                .all(|entry| entry.status().error().is_none())
        );
    }

    #[test]
    fn is_applicable_requires_every_entry() {
        let profile =
            Profile::parse_toml("name = \"Mixed\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap();
        let applicable = ready_preview(
            &profile,
            &Capabilities {
                fan_modes: vec![fan("silent")],
                ..Capabilities::default()
            },
        );
        assert!(applicable.is_applicable());
        let rejected = ready_preview(&profile, &Capabilities::default());
        assert!(!rejected.is_applicable());
    }

    #[test]
    fn readonly_rejects_every_entry() {
        let preview = ProfilePlanner::preview(
            &canonical(),
            &SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware),
            &Capabilities::default(),
        );
        assert_eq!(preview.entries().len(), 6);
        assert!(!preview.is_applicable());
        for entry in preview.entries() {
            assert_eq!(
                entry.status().error(),
                Some(&CommandValidationError::ReadOnly)
            );
        }
    }

    #[test]
    fn readonly_beats_fully_populated_capabilities() {
        let preview = ProfilePlanner::preview(
            &canonical(),
            &SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface),
            &full_capabilities(),
        );
        assert!(!preview.is_applicable());
        assert!(
            preview
                .entries()
                .iter()
                .all(|entry| entry.status().error() == Some(&CommandValidationError::ReadOnly))
        );
    }

    #[test]
    fn unadvertised_fan_mode_gets_exact_error() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"turbo\"\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(preview.entries().len(), 1);
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::FanModeNotAdvertised(fan("turbo")))
        );
        assert!(!preview.is_applicable());
    }

    #[test]
    fn advertised_future_fan_mode_is_applicable() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"hyper-boost\"\n")
                .unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                fan_modes: vec![fan("hyper-boost")],
                ..Capabilities::default()
            },
        );
        assert!(preview.is_applicable());
    }

    #[test]
    fn unadvertised_shift_mode_gets_exact_error() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nshift_mode = \"sport\"\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::ShiftModeNotAdvertised(shift(
                "sport"
            )))
        );
    }

    #[test]
    fn advertised_future_shift_mode_is_applicable() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nshift_mode = \"ultra-turbo\"\n")
                .unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                shift_modes: vec![shift("ultra-turbo")],
                ..Capabilities::default()
            },
        );
        assert!(preview.is_applicable());
    }

    #[test]
    fn missing_cooler_boost_produces_exact_error() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::UnsupportedCapability(
                "cooler boost"
            ))
        );
    }

    #[test]
    fn missing_super_battery_produces_exact_error() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\nsuper_battery = false\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::UnsupportedCapability(
                "super battery"
            ))
        );
    }

    #[test]
    fn false_boolean_still_rejected_when_capability_absent() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = false\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(
            preview.entries()[0].command(),
            &HardwareCommand::SetCoolerBoost(false)
        );
        assert!(!preview.is_applicable());
    }

    #[test]
    fn both_boolean_states_accepted_when_capability_exists() {
        for value in [true, false] {
            let input = format!("name = \"A\"\n\n[performance]\ncooler_boost = {value}\n");
            let preview = ready_preview(
                &Profile::parse_toml(&input).unwrap(),
                &Capabilities {
                    cooler_boost: true,
                    ..Capabilities::default()
                },
            );
            assert!(preview.is_applicable(), "cooler_boost = {value}");
        }
    }

    #[test]
    fn missing_battery_capability_rejects_threshold_command() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(
            preview.entries()[0].command(),
            &HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap())
        );
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::UnsupportedCapability(
                "battery thresholds"
            ))
        );
    }

    #[test]
    fn supported_threshold_is_applicable() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        assert!(preview.is_applicable());
    }

    #[test]
    fn absent_backlight_capability_rejected() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[device]\nkeyboard_backlight = 1\n").unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::UnsupportedCapability(
                "keyboard backlight"
            ))
        );
    }

    #[test]
    fn backlight_above_max_produces_exact_error() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[device]\nkeyboard_backlight = 4\n").unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
                ..Capabilities::default()
            },
        );
        assert_eq!(
            preview.entries()[0].status().error(),
            Some(&CommandValidationError::BacklightAboveMaximum { level: 4, max: 3 })
        );
        assert!(!preview.is_applicable());
    }

    #[test]
    fn backlight_at_max_applies() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[device]\nkeyboard_backlight = 3\n").unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
                ..Capabilities::default()
            },
        );
        assert!(preview.is_applicable());
    }

    #[test]
    fn backlight_zero_applies_when_permitted() {
        let profile =
            Profile::parse_toml("name = \"A\"\n\n[device]\nkeyboard_backlight = 0\n").unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
                ..Capabilities::default()
            },
        );
        assert!(preview.is_applicable());
    }

    #[test]
    fn multiple_rejections_are_all_collected() {
        let profile = Profile::parse_toml(concat!(
            "name = \"Broken\"\n",
            "\n",
            "[performance]\n",
            "fan_mode = \"future\"\n",
            "cooler_boost = true\n",
            "\n",
            "[device]\n",
            "keyboard_backlight = 4\n",
        ))
        .unwrap();
        let preview = ready_preview(&profile, &Capabilities::default());
        assert_eq!(preview.entries().len(), 3);
        assert!(
            preview
                .entries()
                .iter()
                .all(|entry| !entry.status().is_applicable())
        );
        assert!(!preview.is_applicable());
    }

    #[test]
    fn later_entries_validate_after_earlier_rejection() {
        let profile = Profile::parse_toml(concat!(
            "name = \"Mixed\"\n",
            "\n",
            "[performance]\n",
            "fan_mode = \"future\"\n",
            "cooler_boost = true\n",
        ))
        .unwrap();
        let preview = ready_preview(
            &profile,
            &Capabilities {
                cooler_boost: true,
                ..Capabilities::default()
            },
        );
        assert_eq!(preview.entries().len(), 2);
        assert!(!preview.entries()[0].status().is_applicable());
        assert!(preview.entries()[1].status().is_applicable());
        assert!(!preview.is_applicable());
    }

    #[test]
    fn preview_does_not_mutate_profile() {
        let profile = canonical();
        let before = profile.clone();
        let _ = ready_preview(&profile, &full_capabilities());
        assert_eq!(profile, before);
    }

    #[test]
    fn repeated_preview_with_same_inputs_is_equal() {
        let profile = canonical();
        assert_eq!(
            ready_preview(&profile, &full_capabilities()),
            ready_preview(&profile, &full_capabilities())
        );
    }

    #[test]
    fn preview_clone_and_equality_work() {
        let preview = ready_preview(&canonical(), &full_capabilities());
        assert_eq!(preview, preview.clone());
        assert_ne!(
            preview,
            ready_preview(&canonical(), &Capabilities::default())
        );
    }

    #[test]
    fn profile_name_is_preserved() {
        let preview = ready_preview(&canonical(), &full_capabilities());
        assert_eq!(preview.name().as_str(), "Gaming");
    }

    #[test]
    fn preview_exposes_no_mutable_command_access() {
        // Compile-level proof: entries() yields a shared slice, command()
        // and status() yield shared references, so no test code here can
        // obtain `&mut HardwareCommand` from a preview.
        let preview = ready_preview(&canonical(), &full_capabilities());
        let commands: Vec<&HardwareCommand> = commands(&preview);
        assert_eq!(commands.len(), 6);
    }

    #[test]
    fn preview_needs_no_execution_types() {
        // Compile-level proof: this test names no executor, boundary,
        // backend, path, reader, or process type. Preview is pure
        // transformation plus validation over supplied data.
        let preview = ready_preview(&canonical(), &full_capabilities());
        assert!(preview.is_applicable());
    }
}
