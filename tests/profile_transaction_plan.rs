//! Transaction planning through the public API only: parse, preview
//! against supplied data, combine with a directly constructed snapshot.
//! No filesystem, no backend, no execution.

use mec::hardware::{
    BacklightCapability, BatteryThreshold, Capabilities, FanMode, HardwareCommand,
    HardwareSnapshot, ShiftMode, SupportMode,
};
use mec::profiles::{Profile, ProfilePlanner, ProfileTransactionPlanner, TransactionPlanError};

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

fn changed_snapshot() -> HardwareSnapshot {
    HardwareSnapshot {
        shift_mode: Some(ShiftMode::try_from("comfort").unwrap()),
        fan_mode: Some(FanMode::try_from("auto").unwrap()),
        cooler_boost: Some(false),
        super_battery: Some(true),
        battery_start_threshold: Some(60),
        battery_end_threshold: Some(70),
        keyboard_backlight: Some(1),
        ..HardwareSnapshot::default()
    }
}

#[test]
fn full_plan_has_six_steps_and_no_unchanged() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities());
    assert!(preview.is_applicable());
    let plan = ProfileTransactionPlanner::plan(&preview, &changed_snapshot()).unwrap();
    assert_eq!(plan.name().as_str(), "Gaming");
    assert_eq!(plan.steps().len(), 6);
    assert!(plan.unchanged().is_empty());
    assert!(!plan.is_noop());
    assert_eq!(
        plan.steps()[0].forward(),
        &HardwareCommand::SetShiftMode(ShiftMode::try_from("turbo").unwrap())
    );
    assert_eq!(
        plan.steps()[0].rollback(),
        &HardwareCommand::SetShiftMode(ShiftMode::try_from("comfort").unwrap())
    );
    assert_eq!(
        plan.steps()[4].forward(),
        &HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(70, 80).unwrap())
    );
    assert_eq!(
        plan.steps()[4].rollback(),
        &HardwareCommand::SetBatteryThreshold(BatteryThreshold::new(60, 70).unwrap())
    );
}

#[test]
fn satisfied_profile_plans_noop() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities());
    let snapshot = HardwareSnapshot {
        shift_mode: Some(ShiftMode::try_from("turbo").unwrap()),
        fan_mode: Some(FanMode::try_from("advanced").unwrap()),
        cooler_boost: Some(true),
        super_battery: Some(false),
        battery_start_threshold: Some(70),
        battery_end_threshold: Some(80),
        keyboard_backlight: Some(2),
        ..HardwareSnapshot::default()
    };
    let plan = ProfileTransactionPlanner::plan(&preview, &snapshot).unwrap();
    assert!(plan.is_noop());
    assert_eq!(plan.unchanged().len(), 6);
}

#[test]
fn rejected_preview_never_plans() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &Capabilities::default());
    assert!(!preview.is_applicable());
    assert_eq!(
        ProfileTransactionPlanner::plan(&preview, &changed_snapshot()),
        Err(TransactionPlanError::PreviewRejected)
    );
}

#[test]
fn missing_current_value_fails_closed() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities());
    let snapshot = HardwareSnapshot {
        fan_mode: None,
        ..changed_snapshot()
    };
    assert_eq!(
        ProfileTransactionPlanner::plan(&preview, &snapshot),
        Err(TransactionPlanError::MissingCurrentValue("fan mode"))
    );
}

#[test]
fn partial_profile_plans_partial_steps() {
    let profile: Profile = "name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n"
        .parse()
        .unwrap();
    let preview = ProfilePlanner::preview(
        &profile,
        &SupportMode::Ready,
        &Capabilities {
            fan_modes: vec![FanMode::try_from("silent").unwrap()],
            ..Capabilities::default()
        },
    );
    let snapshot = HardwareSnapshot {
        fan_mode: Some(FanMode::try_from("auto").unwrap()),
        ..HardwareSnapshot::default()
    };
    let plan = ProfileTransactionPlanner::plan(&preview, &snapshot).unwrap();
    assert_eq!(plan.steps().len(), 1);
    assert_eq!(
        plan.steps()[0].rollback(),
        &HardwareCommand::SetFanMode(FanMode::try_from("auto").unwrap())
    );
}

#[test]
fn reverse_iteration_gives_rollback_order() {
    let profile: Profile = GAMING_TOML.parse().unwrap();
    let preview = ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities());
    let plan = ProfileTransactionPlanner::plan(&preview, &changed_snapshot()).unwrap();
    let reverse: Vec<&HardwareCommand> = plan
        .steps()
        .iter()
        .rev()
        .map(|step| step.rollback())
        .collect();
    assert_eq!(
        reverse.first(),
        Some(&&HardwareCommand::SetKeyboardBacklight(1))
    );
    assert_eq!(
        reverse.last(),
        Some(&&HardwareCommand::SetShiftMode(
            ShiftMode::try_from("comfort").unwrap()
        ))
    );
    assert_eq!(reverse.len(), 6);
}
