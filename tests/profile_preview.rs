//! Profile preview through the public API only: parse, supply
//! support/capability data directly, inspect typed results. No filesystem.

use mec::hardware::{
    BacklightCapability, Capabilities, CommandValidationError, FanMode, HardwareCommand, ShiftMode,
    SupportMode,
};
use mec::profiles::{Profile, ProfilePlanner};

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

fn full_capabilities() -> Capabilities {
    Capabilities {
        fan_modes: vec![
            FanMode::try_from("advanced").unwrap(),
            FanMode::try_from("auto").unwrap(),
        ],
        shift_modes: vec![
            ShiftMode::try_from("turbo").unwrap(),
            ShiftMode::try_from("comfort").unwrap(),
        ],
        cooler_boost: true,
        super_battery: true,
        keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
        battery_thresholds: true,
        ..Capabilities::default()
    }
}

#[test]
fn canonical_preview_is_fully_applicable_in_order() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities());
    assert_eq!(preview.name().as_str(), "Gaming");
    assert_eq!(preview.entries().len(), 6);
    assert!(preview.is_applicable());
    let commands: Vec<&HardwareCommand> = preview
        .entries()
        .iter()
        .map(|entry| entry.command())
        .collect();
    assert_eq!(
        commands,
        vec![
            &HardwareCommand::SetShiftMode(ShiftMode::try_from("turbo").unwrap()),
            &HardwareCommand::SetFanMode(FanMode::try_from("advanced").unwrap()),
            &HardwareCommand::SetCoolerBoost(true),
            &HardwareCommand::SetSuperBattery(false),
            &HardwareCommand::SetBatteryThreshold(
                mec::hardware::BatteryThreshold::new(70, 80).unwrap()
            ),
            &HardwareCommand::SetKeyboardBacklight(2),
        ]
    );
}

#[test]
fn partial_preview_contains_only_requested_commands() {
    let profile: Profile = "name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n"
        .parse()
        .unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities());
    assert_eq!(preview.entries().len(), 1);
    assert_eq!(
        preview.entries()[0].command(),
        &HardwareCommand::SetFanMode(FanMode::try_from("silent").unwrap())
    );
}

#[test]
fn readonly_preview_rejects_everything() {
    use mec::hardware::ReadOnlyReason;

    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(
        &profile,
        &SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware),
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
fn preview_collects_all_rejections_without_fail_fast() {
    let profile: Profile = concat!(
        "name = \"Broken\"\n",
        "\n",
        "[performance]\n",
        "fan_mode = \"future\"\n",
        "cooler_boost = true\n",
        "\n",
        "[device]\n",
        "keyboard_backlight = 4\n",
    )
    .parse()
    .unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &Capabilities::default());
    assert_eq!(preview.entries().len(), 3);
    assert!(!preview.is_applicable());
    assert!(
        preview
            .entries()
            .iter()
            .all(|entry| !entry.status().is_applicable())
    );
}
