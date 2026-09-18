//! Pure reversible transaction planning: describe what applying a validated
//! preview would change, and how each change could be undone.
//!
//! [`ProfileTransactionPlanner::plan`] combines a [`ProfilePreview`] with a
//! caller-supplied [`HardwareSnapshot`] into a [`ProfileTransactionPlan`]:
//! forward mutation steps with paired rollback commands, plus the requested
//! settings already satisfied. Planning performs no reads, no writes, no
//! discovery, and no evaluation.
//!
//! A plan is descriptive data, NOT an authorization token. Later execution
//! must take a fresh snapshot immediately before application, perform fresh
//! support evaluation and capability discovery, and validate commands again
//! before writes. Future rollback execution must traverse successfully
//! applied steps in REVERSE application order; each step already carries its
//! own rollback command, so there is a single source of rollback truth.

use thiserror::Error;

use crate::hardware::{BatteryThreshold, BatteryThresholdError, HardwareCommand, HardwareSnapshot};

use super::{ProfileName, ProfilePreview};

/// Pure planner: applicable preview plus current snapshot in, reversible
/// descriptive plan out. Holds no state.
pub struct ProfileTransactionPlanner;

impl ProfileTransactionPlanner {
    /// Builds a reversible plan from an applicable `preview` and the
    /// `snapshot` it was compared against. Returns
    /// [`TransactionPlanError::PreviewRejected`] unless every preview entry
    /// is applicable, and fails closed when a requested setting has no
    /// known current value to roll back to.
    pub fn plan(
        preview: &ProfilePreview,
        snapshot: &HardwareSnapshot,
    ) -> Result<ProfileTransactionPlan, TransactionPlanError> {
        if !preview.is_applicable() {
            return Err(TransactionPlanError::PreviewRejected);
        }
        let mut steps = Vec::new();
        let mut unchanged = Vec::new();
        for entry in preview.entries() {
            match step_for(entry.command(), snapshot)? {
                StepOutcome::Changed { forward, rollback } => {
                    steps.push(TransactionStep { forward, rollback });
                }
                StepOutcome::Unchanged(command) => unchanged.push(command),
            }
        }
        Ok(ProfileTransactionPlan {
            name: preview.name().clone(),
            steps,
            unchanged,
        })
    }
}

/// Descriptive reversible plan: mutation steps with paired rollback
/// commands, plus already-satisfied requests. Never an authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileTransactionPlan {
    name: ProfileName,
    steps: Vec<TransactionStep>,
    unchanged: Vec<HardwareCommand>,
}

/// One mutation: the forward command plus the exact typed command restoring
/// the pre-transaction value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionStep {
    forward: HardwareCommand,
    rollback: HardwareCommand,
}

/// Why transaction planning failed. All failures are fail-closed: no
/// partial plan is ever returned.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransactionPlanError {
    /// The preview contains rejected commands; planning needs an
    /// applicable preview.
    #[error("profile preview contains rejected commands")]
    PreviewRejected,
    /// A requested setting has no known current value to roll back to.
    #[error("current value unavailable for {0}")]
    MissingCurrentValue(&'static str),
    /// The current battery pair violates the typed threshold invariant.
    #[error("current battery threshold is invalid: {0}")]
    InvalidCurrentBatteryThreshold(BatteryThresholdError),
    /// A preview entry is not part of the profile schema.
    #[error("unsupported command in profile preview: {0}")]
    UnsupportedPreviewCommand(&'static str),
}

impl ProfileTransactionPlan {
    /// Profile display name, preserved from the preview.
    pub fn name(&self) -> &ProfileName {
        &self.name
    }

    /// Mutation steps in preview (application) order.
    pub fn steps(&self) -> &[TransactionStep] {
        &self.steps
    }

    /// Requested commands already satisfied, in preview relative order.
    pub fn unchanged(&self) -> &[HardwareCommand] {
        &self.unchanged
    }

    /// True when there is nothing to mutate.
    pub fn is_noop(&self) -> bool {
        self.steps.is_empty()
    }
}

impl TransactionStep {
    /// The command applying the desired setting.
    pub fn forward(&self) -> &HardwareCommand {
        &self.forward
    }

    /// The command restoring the pre-transaction value.
    pub fn rollback(&self) -> &HardwareCommand {
        &self.rollback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Capabilities, FanMode, ReadOnlyReason, ShiftMode, SupportMode};
    use crate::profiles::{Profile, ProfilePlanner};

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

    fn threshold(start: u8, end: u8) -> BatteryThreshold {
        BatteryThreshold::new(start, end).unwrap()
    }

    fn full_capabilities() -> Capabilities {
        Capabilities {
            fan_modes: vec![fan("advanced"), fan("auto")],
            shift_modes: vec![shift("turbo"), shift("comfort")],
            cooler_boost: true,
            super_battery: true,
            keyboard_backlight: Some(crate::hardware::BacklightCapability { max_brightness: 3 }),
            battery_thresholds: true,
            ..Capabilities::default()
        }
    }

    fn applicable_preview() -> ProfilePreview {
        let profile = Profile::parse_toml(CANONICAL_TOML).unwrap();
        ProfilePlanner::preview(&profile, &SupportMode::Ready, &full_capabilities())
    }

    /// Every desired value differs from the snapshot.
    fn changed_snapshot() -> HardwareSnapshot {
        HardwareSnapshot {
            shift_mode: Some(shift("comfort")),
            fan_mode: Some(fan("auto")),
            cooler_boost: Some(false),
            super_battery: Some(true),
            battery_start_threshold: Some(60),
            battery_end_threshold: Some(70),
            keyboard_backlight: Some(1),
            ..HardwareSnapshot::default()
        }
    }

    /// Every desired value already matches the snapshot.
    fn matching_snapshot() -> HardwareSnapshot {
        HardwareSnapshot {
            shift_mode: Some(shift("turbo")),
            fan_mode: Some(fan("advanced")),
            cooler_boost: Some(true),
            super_battery: Some(false),
            battery_start_threshold: Some(70),
            battery_end_threshold: Some(80),
            keyboard_backlight: Some(2),
            ..HardwareSnapshot::default()
        }
    }

    fn plan_for(preview: &ProfilePreview, snapshot: &HardwareSnapshot) -> ProfileTransactionPlan {
        ProfileTransactionPlanner::plan(preview, snapshot).unwrap()
    }

    fn forwards(plan: &ProfileTransactionPlan) -> Vec<&HardwareCommand> {
        plan.steps().iter().map(|step| step.forward()).collect()
    }

    fn rollbacks(plan: &ProfileTransactionPlan) -> Vec<&HardwareCommand> {
        plan.steps().iter().map(|step| step.rollback()).collect()
    }

    #[test]
    fn fully_changed_preview_produces_six_steps() {
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        assert_eq!(plan.steps().len(), 6);
        assert!(!plan.is_noop());
    }

    #[test]
    fn plan_preserves_profile_name() {
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        assert_eq!(plan.name().as_str(), "Gaming");
    }

    #[test]
    fn forward_order_equals_preview_order() {
        let preview = applicable_preview();
        let plan = plan_for(&preview, &changed_snapshot());
        let preview_commands: Vec<&HardwareCommand> = preview
            .entries()
            .iter()
            .map(|entry| entry.command())
            .collect();
        assert_eq!(forwards(&plan), preview_commands);
    }

    #[test]
    fn forwards_preserve_exact_desired_values() {
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        assert_eq!(
            forwards(&plan),
            vec![
                &HardwareCommand::SetShiftMode(shift("turbo")),
                &HardwareCommand::SetFanMode(fan("advanced")),
                &HardwareCommand::SetCoolerBoost(true),
                &HardwareCommand::SetSuperBattery(false),
                &HardwareCommand::SetBatteryThreshold(threshold(70, 80)),
                &HardwareCommand::SetKeyboardBacklight(2),
            ]
        );
    }

    #[test]
    fn rollbacks_preserve_exact_snapshot_values() {
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        assert_eq!(
            rollbacks(&plan),
            vec![
                &HardwareCommand::SetShiftMode(shift("comfort")),
                &HardwareCommand::SetFanMode(fan("auto")),
                &HardwareCommand::SetCoolerBoost(false),
                &HardwareCommand::SetSuperBattery(true),
                &HardwareCommand::SetBatteryThreshold(threshold(60, 70)),
                &HardwareCommand::SetKeyboardBacklight(1),
            ]
        );
    }

    #[test]
    fn unchanged_shift_becomes_unchanged_entry() {
        let snapshot = HardwareSnapshot {
            shift_mode: Some(shift("turbo")),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(plan.steps().len(), 5);
        assert_eq!(
            plan.unchanged(),
            &[HardwareCommand::SetShiftMode(shift("turbo"))]
        );
    }

    #[test]
    fn unchanged_fan_becomes_unchanged_entry() {
        let snapshot = HardwareSnapshot {
            fan_mode: Some(fan("advanced")),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(plan.steps().len(), 5);
        assert_eq!(
            plan.unchanged(),
            &[HardwareCommand::SetFanMode(fan("advanced"))]
        );
    }

    #[test]
    fn unchanged_cooler_boost_becomes_unchanged_entry() {
        let snapshot = HardwareSnapshot {
            cooler_boost: Some(true),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(plan.steps().len(), 5);
        assert_eq!(plan.unchanged(), &[HardwareCommand::SetCoolerBoost(true)]);
    }

    #[test]
    fn unchanged_super_battery_becomes_unchanged_entry() {
        let snapshot = HardwareSnapshot {
            super_battery: Some(false),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(plan.steps().len(), 5);
        assert_eq!(plan.unchanged(), &[HardwareCommand::SetSuperBattery(false)]);
    }

    #[test]
    fn unchanged_battery_becomes_unchanged_entry() {
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(70),
            battery_end_threshold: Some(80),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(plan.steps().len(), 5);
        assert_eq!(
            plan.unchanged(),
            &[HardwareCommand::SetBatteryThreshold(threshold(70, 80))]
        );
    }

    #[test]
    fn unchanged_backlight_becomes_unchanged_entry() {
        let snapshot = HardwareSnapshot {
            keyboard_backlight: Some(2),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(plan.steps().len(), 5);
        assert_eq!(
            plan.unchanged(),
            &[HardwareCommand::SetKeyboardBacklight(2)]
        );
    }

    #[test]
    fn fully_unchanged_profile_produces_zero_steps() {
        let plan = plan_for(&applicable_preview(), &matching_snapshot());
        assert!(plan.steps().is_empty());
        assert!(plan.is_noop());
        assert_eq!(plan.unchanged().len(), 6);
    }

    #[test]
    fn mixed_profile_preserves_changed_relative_order() {
        let snapshot = HardwareSnapshot {
            shift_mode: Some(shift("turbo")),
            fan_mode: Some(fan("advanced")),
            ..changed_snapshot()
        };
        let plan = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(
            forwards(&plan),
            vec![
                &HardwareCommand::SetCoolerBoost(true),
                &HardwareCommand::SetSuperBattery(false),
                &HardwareCommand::SetBatteryThreshold(threshold(70, 80)),
                &HardwareCommand::SetKeyboardBacklight(2),
            ]
        );
        assert_eq!(
            plan.unchanged(),
            &[
                HardwareCommand::SetShiftMode(shift("turbo")),
                HardwareCommand::SetFanMode(fan("advanced")),
            ]
        );
    }

    #[test]
    fn shift_forward_and_rollback_are_exact() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[performance]\nshift_mode = \"turbo\"\n")
                .unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                shift_modes: vec![shift("turbo")],
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            shift_mode: Some(shift("comfort")),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetShiftMode(shift("turbo"))
        );
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetShiftMode(shift("comfort"))
        );
    }

    #[test]
    fn fan_forward_and_rollback_are_exact() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[performance]\nfan_mode = \"silent\"\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                fan_modes: vec![fan("silent")],
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            fan_mode: Some(fan("auto")),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetFanMode(fan("silent"))
        );
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetFanMode(fan("auto"))
        );
    }

    #[test]
    fn cooler_forward_and_rollback_are_exact() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[performance]\ncooler_boost = true\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                cooler_boost: true,
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            cooler_boost: Some(false),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetCoolerBoost(true)
        );
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetCoolerBoost(false)
        );
    }

    #[test]
    fn super_battery_forward_and_rollback_are_exact() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[performance]\nsuper_battery = false\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                super_battery: true,
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            super_battery: Some(true),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetSuperBattery(false)
        );
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetSuperBattery(true)
        );
    }

    #[test]
    fn battery_forward_and_rollback_are_exact_typed_pairs() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(50),
            battery_end_threshold: Some(60),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetBatteryThreshold(threshold(70, 80))
        );
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetBatteryThreshold(threshold(50, 60))
        );
    }

    #[test]
    fn backlight_forward_and_rollback_are_exact() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[device]\nkeyboard_backlight = 2\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                keyboard_backlight: Some(crate::hardware::BacklightCapability {
                    max_brightness: 3,
                }),
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            keyboard_backlight: Some(0),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetKeyboardBacklight(2)
        );
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetKeyboardBacklight(0)
        );
    }

    #[test]
    fn missing_shift_current_value_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                shift_mode: None,
                ..changed_snapshot()
            },
        )
        .expect_err("missing shift value must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("shift mode")
        );
    }

    #[test]
    fn missing_fan_current_value_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                fan_mode: None,
                ..changed_snapshot()
            },
        )
        .expect_err("missing fan value must fail closed");
        assert_eq!(error, TransactionPlanError::MissingCurrentValue("fan mode"));
    }

    #[test]
    fn missing_cooler_boost_current_value_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                cooler_boost: None,
                ..changed_snapshot()
            },
        )
        .expect_err("missing cooler boost value must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("cooler boost")
        );
    }

    #[test]
    fn missing_super_battery_current_value_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                super_battery: None,
                ..changed_snapshot()
            },
        )
        .expect_err("missing super battery value must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("super battery")
        );
    }

    #[test]
    fn missing_backlight_current_value_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                keyboard_backlight: None,
                ..changed_snapshot()
            },
        )
        .expect_err("missing backlight value must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("keyboard backlight")
        );
    }

    #[test]
    fn missing_both_battery_values_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                battery_start_threshold: None,
                battery_end_threshold: None,
                ..changed_snapshot()
            },
        )
        .expect_err("missing battery values must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("battery thresholds")
        );
    }

    #[test]
    fn missing_battery_start_only_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                battery_start_threshold: None,
                ..changed_snapshot()
            },
        )
        .expect_err("half-present battery values must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("battery thresholds")
        );
    }

    #[test]
    fn missing_battery_end_only_fails() {
        let error = ProfileTransactionPlanner::plan(
            &applicable_preview(),
            &HardwareSnapshot {
                battery_end_threshold: None,
                ..changed_snapshot()
            },
        )
        .expect_err("half-present battery values must fail closed");
        assert_eq!(
            error,
            TransactionPlanError::MissingCurrentValue("battery thresholds")
        );
    }

    #[test]
    fn valid_current_threshold_pairs_are_accepted() {
        for (start, end) in [(0, 10), (90, 100)] {
            let preview = ProfilePlanner::preview(
                &Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n")
                    .unwrap(),
                &SupportMode::Ready,
                &Capabilities {
                    battery_thresholds: true,
                    ..Capabilities::default()
                },
            );
            let snapshot = HardwareSnapshot {
                battery_start_threshold: Some(start),
                battery_end_threshold: Some(end),
                ..HardwareSnapshot::default()
            };
            let plan = plan_for(&preview, &snapshot);
            assert_eq!(
                plan.steps()[0].rollback(),
                &HardwareCommand::SetBatteryThreshold(threshold(start, end)),
                "current {start}/{end}"
            );
        }
    }

    #[test]
    fn incoherent_current_threshold_fails_typed() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(70),
            battery_end_threshold: Some(90),
            ..HardwareSnapshot::default()
        };
        let error = ProfileTransactionPlanner::plan(&preview, &snapshot)
            .expect_err("incoherent current pair must fail");
        assert_eq!(
            error,
            TransactionPlanError::InvalidCurrentBatteryThreshold(
                crate::hardware::BatteryThresholdError::UnsupportedGap { start: 70, end: 90 }
            )
        );
    }

    #[test]
    fn inverted_current_threshold_fails_typed() {
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(80),
            battery_end_threshold: Some(70),
            ..HardwareSnapshot::default()
        };
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        let error = ProfileTransactionPlanner::plan(&preview, &snapshot)
            .expect_err("inverted current pair must fail");
        assert!(matches!(
            error,
            TransactionPlanError::InvalidCurrentBatteryThreshold(_)
        ));
    }

    #[test]
    fn equal_current_threshold_fails_typed() {
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(80),
            battery_end_threshold: Some(80),
            ..HardwareSnapshot::default()
        };
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        let error = ProfileTransactionPlanner::plan(&preview, &snapshot)
            .expect_err("equal current pair must fail");
        assert!(matches!(
            error,
            TransactionPlanError::InvalidCurrentBatteryThreshold(_)
        ));
    }

    #[test]
    fn planner_never_clamps_threshold_values() {
        // 71/80 is close to valid but not representable: it must fail
        // rather than being normalized into a nearby valid pair.
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(71),
            battery_end_threshold: Some(80),
            ..HardwareSnapshot::default()
        };
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"A\"\n\n[battery]\ncharge_end_threshold = 80\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        assert!(matches!(
            ProfileTransactionPlanner::plan(&preview, &snapshot),
            Err(TransactionPlanError::InvalidCurrentBatteryThreshold(_))
        ));
    }

    #[test]
    fn rejected_preview_fails_as_preview_rejected() {
        let profile = Profile::parse_toml(CANONICAL_TOML).unwrap();
        let preview =
            ProfilePlanner::preview(&profile, &SupportMode::Ready, &Capabilities::default());
        assert!(!preview.is_applicable());
        assert_eq!(
            ProfileTransactionPlanner::plan(&preview, &changed_snapshot()),
            Err(TransactionPlanError::PreviewRejected)
        );
    }

    #[test]
    fn mixed_preview_fails_completely() {
        let profile = Profile::parse_toml(CANONICAL_TOML).unwrap();
        let preview = ProfilePlanner::preview(
            &profile,
            &SupportMode::Ready,
            &Capabilities {
                fan_modes: vec![fan("advanced")],
                ..Capabilities::default()
            },
        );
        assert!(!preview.is_applicable());
        assert_eq!(
            ProfileTransactionPlanner::plan(&preview, &changed_snapshot()),
            Err(TransactionPlanError::PreviewRejected)
        );
    }

    #[test]
    fn fan_only_changed_profile_yields_one_step() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"Quiet\"\n\n[performance]\nfan_mode = \"silent\"\n")
                .unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                fan_modes: vec![fan("silent")],
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            fan_mode: Some(fan("auto")),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(
            plan.steps()[0].forward(),
            &HardwareCommand::SetFanMode(fan("silent"))
        );
        assert!(plan.unchanged().is_empty());
        assert!(!plan.is_noop());
    }

    #[test]
    fn battery_only_changed_profile_yields_one_step() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 60\n")
                .unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                battery_thresholds: true,
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            battery_start_threshold: Some(70),
            battery_end_threshold: Some(80),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(plan.steps().len(), 1);
        assert_eq!(
            plan.steps()[0].rollback(),
            &HardwareCommand::SetBatteryThreshold(threshold(70, 80))
        );
    }

    #[test]
    fn device_only_changed_profile_yields_one_step() {
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml("name = \"Glow\"\n\n[device]\nkeyboard_backlight = 1\n").unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                keyboard_backlight: Some(crate::hardware::BacklightCapability {
                    max_brightness: 3,
                }),
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            keyboard_backlight: Some(0),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        assert_eq!(plan.steps().len(), 1);
        assert!(!plan.is_noop());
    }

    #[test]
    fn planning_invents_no_commands_absent_from_preview() {
        let preview = applicable_preview();
        let plan = plan_for(&preview, &changed_snapshot());
        let preview_commands: Vec<&HardwareCommand> = preview
            .entries()
            .iter()
            .map(|entry| entry.command())
            .collect();
        let planned: Vec<&HardwareCommand> = forwards(&plan)
            .into_iter()
            .chain(plan.unchanged().iter())
            .collect();
        assert_eq!(planned, preview_commands);
    }

    #[test]
    fn planning_does_not_mutate_preview() {
        let preview = applicable_preview();
        let before = preview.clone();
        let _ = plan_for(&preview, &changed_snapshot());
        assert_eq!(preview, before);
    }

    #[test]
    fn planning_does_not_mutate_snapshot() {
        let snapshot = changed_snapshot();
        let before = snapshot.clone();
        let _ = plan_for(&applicable_preview(), &snapshot);
        assert_eq!(snapshot, before);
    }

    #[test]
    fn repeated_planning_with_same_inputs_is_equal() {
        let preview = applicable_preview();
        let snapshot = changed_snapshot();
        assert_eq!(plan_for(&preview, &snapshot), plan_for(&preview, &snapshot));
    }

    #[test]
    fn plan_clone_and_equality_work() {
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        assert_eq!(plan, plan.clone());
        assert_ne!(plan, plan_for(&applicable_preview(), &matching_snapshot()));
    }

    #[test]
    fn accessors_expose_shared_references_only() {
        // Compile-level proof: name(), steps(), unchanged(), forward(),
        // and rollback() all yield shared references.
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        let _: &ProfileName = plan.name();
        let _: &[TransactionStep] = plan.steps();
        let _: &[HardwareCommand] = plan.unchanged();
        let _: &HardwareCommand = plan.steps()[0].forward();
        let _: &HardwareCommand = plan.steps()[0].rollback();
    }

    #[test]
    fn planning_needs_no_backend_types() {
        // Compile-level proof: this test names no executor, boundary,
        // backend, path, reader, or process type. Planning is pure
        // transformation over supplied data.
        let plan = plan_for(&applicable_preview(), &changed_snapshot());
        assert_eq!(plan.steps().len(), 6);
    }

    #[test]
    fn reverse_iteration_gives_rollback_order() {
        // Future rollback execution must traverse successfully applied
        // steps in reverse; paired commands make that deterministic.
        let preview = ProfilePlanner::preview(
            &Profile::parse_toml(concat!(
                "name = \"Trio\"\n",
                "\n",
                "[performance]\n",
                "shift_mode = \"turbo\"\n",
                "fan_mode = \"advanced\"\n",
                "cooler_boost = true\n",
            ))
            .unwrap(),
            &SupportMode::Ready,
            &Capabilities {
                shift_modes: vec![shift("turbo")],
                fan_modes: vec![fan("advanced")],
                cooler_boost: true,
                ..Capabilities::default()
            },
        );
        let snapshot = HardwareSnapshot {
            shift_mode: Some(shift("comfort")),
            fan_mode: Some(fan("auto")),
            cooler_boost: Some(false),
            ..HardwareSnapshot::default()
        };
        let plan = plan_for(&preview, &snapshot);
        let reverse_rollbacks: Vec<&HardwareCommand> = plan
            .steps()
            .iter()
            .rev()
            .map(|step| step.rollback())
            .collect();
        assert_eq!(
            reverse_rollbacks,
            vec![
                &HardwareCommand::SetCoolerBoost(false),
                &HardwareCommand::SetFanMode(fan("auto")),
                &HardwareCommand::SetShiftMode(shift("comfort")),
            ]
        );
    }

    #[test]
    fn webcam_preview_command_fails_closed() {
        let error = step_for(&HardwareCommand::SetWebcam(true), &changed_snapshot())
            .expect_err("webcam has no profile mapping");
        assert_eq!(
            error,
            TransactionPlanError::UnsupportedPreviewCommand("webcam")
        );
    }

    #[test]
    fn webcam_block_preview_command_fails_closed() {
        let error = step_for(&HardwareCommand::SetWebcamBlock(false), &changed_snapshot())
            .expect_err("webcam block has no profile mapping");
        assert_eq!(
            error,
            TransactionPlanError::UnsupportedPreviewCommand("webcam block")
        );
    }

    #[test]
    fn error_display_is_stable_and_human_readable() {
        assert_eq!(
            TransactionPlanError::PreviewRejected.to_string(),
            "profile preview contains rejected commands"
        );
        assert_eq!(
            TransactionPlanError::MissingCurrentValue("fan mode").to_string(),
            "current value unavailable for fan mode"
        );
        assert!(
            !TransactionPlanError::UnsupportedPreviewCommand("webcam")
                .to_string()
                .is_empty()
        );
    }

    #[test]
    fn readonly_preview_always_rejected_for_planning() {
        let profile = Profile::parse_toml(CANONICAL_TOML).unwrap();
        let preview = ProfilePlanner::preview(
            &profile,
            &SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware),
            &full_capabilities(),
        );
        assert_eq!(
            ProfileTransactionPlanner::plan(&preview, &changed_snapshot()),
            Err(TransactionPlanError::PreviewRejected)
        );
    }
}

/// Planning outcome for one preview command.
#[derive(Debug, Clone, PartialEq, Eq)]
enum StepOutcome {
    Changed {
        forward: HardwareCommand,
        rollback: HardwareCommand,
    },
    Unchanged(HardwareCommand),
}

fn split(forward: HardwareCommand, rollback: HardwareCommand) -> StepOutcome {
    if forward == rollback {
        StepOutcome::Unchanged(forward)
    } else {
        StepOutcome::Changed { forward, rollback }
    }
}

fn step_for(
    command: &HardwareCommand,
    snapshot: &HardwareSnapshot,
) -> Result<StepOutcome, TransactionPlanError> {
    match command {
        HardwareCommand::SetShiftMode(_) => {
            let current = snapshot
                .shift_mode
                .clone()
                .ok_or(TransactionPlanError::MissingCurrentValue("shift mode"))?;
            Ok(split(
                command.clone(),
                HardwareCommand::SetShiftMode(current),
            ))
        }
        HardwareCommand::SetFanMode(_) => {
            let current = snapshot
                .fan_mode
                .clone()
                .ok_or(TransactionPlanError::MissingCurrentValue("fan mode"))?;
            Ok(split(command.clone(), HardwareCommand::SetFanMode(current)))
        }
        HardwareCommand::SetCoolerBoost(_) => {
            let current = snapshot
                .cooler_boost
                .ok_or(TransactionPlanError::MissingCurrentValue("cooler boost"))?;
            Ok(split(
                command.clone(),
                HardwareCommand::SetCoolerBoost(current),
            ))
        }
        HardwareCommand::SetSuperBattery(_) => {
            let current = snapshot
                .super_battery
                .ok_or(TransactionPlanError::MissingCurrentValue("super battery"))?;
            Ok(split(
                command.clone(),
                HardwareCommand::SetSuperBattery(current),
            ))
        }
        HardwareCommand::SetBatteryThreshold(_) => {
            let (start, end) = match (
                snapshot.battery_start_threshold,
                snapshot.battery_end_threshold,
            ) {
                (Some(start), Some(end)) => (start, end),
                _ => {
                    return Err(TransactionPlanError::MissingCurrentValue(
                        "battery thresholds",
                    ));
                }
            };
            let current = BatteryThreshold::new(start, end)
                .map_err(TransactionPlanError::InvalidCurrentBatteryThreshold)?;
            Ok(split(
                command.clone(),
                HardwareCommand::SetBatteryThreshold(current),
            ))
        }
        HardwareCommand::SetKeyboardBacklight(_) => {
            let current =
                snapshot
                    .keyboard_backlight
                    .ok_or(TransactionPlanError::MissingCurrentValue(
                        "keyboard backlight",
                    ))?;
            Ok(split(
                command.clone(),
                HardwareCommand::SetKeyboardBacklight(current),
            ))
        }
        HardwareCommand::SetWebcam(_) => {
            Err(TransactionPlanError::UnsupportedPreviewCommand("webcam"))
        }
        HardwareCommand::SetWebcamBlock(_) => Err(TransactionPlanError::UnsupportedPreviewCommand(
            "webcam block",
        )),
    }
}
