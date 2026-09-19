//! Production safe-control composition: one typed command through the full
//! safe pipeline.
//!
//! [`execute_hardware_command`] composes the existing layers without
//! duplicating them:
//!
//! ```text
//! SystemPaths (single root)
//!     +
//! LinuxSysfsReader (inspection and readback)
//!     +
//! MsiEcSysfsWriteBoundary (fixed targets, verified writes)
//!     +
//! HardwareCommandExecutor (fresh policy before every crossing)
//! ```
//!
//! The support/capability checks and the write boundary always operate over
//! the same [`SystemPaths`] root: there is a single `paths` argument, cloned
//! for the boundary and moved into the executor, so validating against one
//! root while writing to another is impossible by construction.

use crate::hardware::{
    HardwareCommand, LinuxSysfsReader, SystemPaths, msi_ec_write::MsiEcSysfsWriteBoundary,
};
use crate::profiles::Profile;

use super::{
    CommandExecutionError, HardwareCommandExecutor, ProfileApplyError, ProfileApplyReport,
    ProfileTransactionExecutor,
};

/// Executes one typed hardware command through the entire safe pipeline:
/// fresh support evaluation, fresh capability discovery, pure validation,
/// then a verified write through the restricted boundary.
///
/// - Rechecks support and capabilities on every call via the executor; this
///   function holds no state and caches nothing.
/// - Performs verified writes through the restricted boundary: `Ok(())`
///   means readback showed the requested state.
/// - Performs no privilege elevation. When the current process lacks
///   permission, [`WriteBoundaryError::AccessDenied`] propagates as
///   [`CommandExecutionError::Boundary`]; callers must not retry with
///   elevation on their own.
/// - Never bypasses [`HardwareCommand::validate`]: validation runs inside
///   the executor immediately before the boundary is crossed.
///
/// [`WriteBoundaryError::AccessDenied`]: crate::hardware::WriteBoundaryError::AccessDenied
pub fn execute_hardware_command(
    paths: SystemPaths,
    command: &HardwareCommand,
) -> Result<(), CommandExecutionError> {
    let reader = LinuxSysfsReader;
    let boundary = MsiEcSysfsWriteBoundary::new(paths.clone(), reader);
    let executor = HardwareCommandExecutor::new(paths, reader, boundary);
    executor.execute(command)
}

/// Applies one profile through the entire safe transaction pipeline:
/// fresh support evaluation, fresh capability discovery, pure preview
/// validation, a fresh rollback-baseline snapshot, reversible planning,
/// then verified forward execution with best-effort reverse rollback.
///
/// - Holds no state and caches nothing; the transaction executor
///   regenerates preview, snapshot, and plan on every call from the
///   supplied [`Profile`] alone. There is exactly one `paths` input, so
///   validation, snapshot, writes, and readback all operate against the
///   same root by construction.
/// - Performs no privilege elevation. A write the OS denies propagates
///   through the existing typed transaction failure.
/// - Never bypasses validation: every forward and rollback command runs
///   through [`HardwareCommandExecutor::execute`] immediately before its
///   boundary crossing.
pub fn apply_profile(
    paths: SystemPaths,
    profile: &Profile,
) -> Result<ProfileApplyReport, ProfileApplyError> {
    let reader = LinuxSysfsReader;
    let boundary = MsiEcSysfsWriteBoundary::new(paths.clone(), reader);
    let executor = ProfileTransactionExecutor::new(paths, reader, boundary);
    executor.apply(profile)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::hardware::{
        BatteryThreshold, CommandValidationError, FanMode, ShiftMode, WriteBoundaryError,
    };
    use crate::profiles::Profile;

    /// Test-only minimal fake root. Never touches `/sys`; never copies or
    /// mutates the tracked fixtures. Never names the concrete writer type:
    /// the composed API must be usable without it.
    struct Fixture {
        root: TempDir,
        paths: SystemPaths,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempdir().unwrap();
            Self {
                paths: SystemPaths::new(root.path()),
                root,
            }
        }

        fn write_dmi(&self, name: &str, contents: &[u8]) {
            let dir = self.root.path().join("sys/class/dmi/id");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join(name), contents).unwrap();
        }

        fn write_msi_identity(&self) {
            self.write_dmi("sys_vendor", b"Micro-Star International Co., Ltd.\n");
            self.write_dmi("product_name", b"Test MSI Laptop\n");
        }

        fn ec_dir(&self) -> PathBuf {
            self.root.path().join("sys/devices/platform/msi-ec")
        }

        fn ensure_ec_root(&self) {
            fs::create_dir_all(self.ec_dir()).unwrap();
        }

        fn ec_file(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self.ec_dir().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        /// READY base: MSI identity plus an enumerable but empty driver
        /// root, so every capability is cleanly absent.
        fn ready_base(&self) {
            self.write_msi_identity();
            self.ensure_ec_root();
        }

        fn enable_fan_modes(&self, current: &str, available: &[&str]) {
            self.ec_file("fan_mode", format!("{current}\n").as_bytes());
            let list = available.join("\n") + "\n";
            self.ec_file("available_fan_modes", list.as_bytes());
        }

        fn collect_files(&self) -> Vec<String> {
            let mut files = Vec::new();
            let mut stack = vec![self.root.path().to_path_buf()];
            while let Some(dir) = stack.pop() {
                let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .collect();
                entries.sort();
                for entry in entries {
                    if entry.is_symlink() || entry.is_file() {
                        files.push(
                            entry
                                .strip_prefix(self.root.path())
                                .unwrap()
                                .to_string_lossy()
                                .into_owned(),
                        );
                    } else if entry.is_dir() {
                        stack.push(entry);
                    }
                }
            }
            files.sort();
            files
        }
    }

    fn fan(name: &str) -> FanMode {
        FanMode::try_from(name).unwrap()
    }

    fn shift(name: &str) -> ShiftMode {
        ShiftMode::try_from(name).unwrap()
    }

    fn threshold() -> BatteryThreshold {
        BatteryThreshold::new(70, 80).unwrap()
    }

    #[test]
    fn composed_fan_mode_write_succeeds_with_exact_content() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["auto", "silent"]);
        let command = HardwareCommand::SetFanMode(fan("auto"));
        assert!(execute_hardware_command(fixture.paths.clone(), &command).is_ok());
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"auto\n"
        );
    }

    #[test]
    fn unsupported_fan_mode_fails_before_mutation() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto"]);
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetFanMode(fan("turbo")),
        )
        .expect_err("unadvertised mode must fail validation");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::FanModeNotAdvertised(_))
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"auto\n"
        );
    }

    #[test]
    fn readonly_identity_fails_before_mutation() {
        let fixture = Fixture::new();
        fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
        fixture.write_dmi("product_name", b"XPS 15\n");
        fixture.ensure_ec_root();
        let target = fixture.ec_file("cooler_boost", b"off\n");
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetCoolerBoost(true),
        )
        .expect_err("non-MSI hardware must stay read-only");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::ReadOnly)
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(fs::read(&target).unwrap(), b"off\n");
    }

    #[test]
    fn missing_msi_ec_root_prevents_any_write() {
        let fixture = Fixture::new();
        fixture.write_msi_identity();
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetCoolerBoost(true),
        )
        .expect_err("absent driver root must stay read-only");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::ReadOnly)
            ),
            "unexpected error: {error:?}"
        );
        assert!(!fixture.root.path().join("sys/devices").exists());
    }

    #[test]
    fn supported_cooler_boost_flows_through() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let target = fixture.ec_file("cooler_boost", b"off\n");
        assert!(
            execute_hardware_command(
                fixture.paths.clone(),
                &HardwareCommand::SetCoolerBoost(true)
            )
            .is_ok()
        );
        assert_eq!(fs::read(&target).unwrap(), b"on\n");
    }

    #[test]
    fn unsupported_boolean_fails_without_creating_target() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetCoolerBoost(true),
        )
        .expect_err("absent control must fail validation");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::UnsupportedCapability(_))
            ),
            "unexpected error: {error:?}"
        );
        assert!(!fixture.ec_dir().join("cooler_boost").exists());
    }

    #[test]
    fn supported_backlight_succeeds() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let dir = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("brightness"), b"1\n").unwrap();
        fs::write(dir.join("max_brightness"), b"3\n").unwrap();
        assert!(
            execute_hardware_command(
                fixture.paths.clone(),
                &HardwareCommand::SetKeyboardBacklight(2)
            )
            .is_ok()
        );
        assert_eq!(fs::read(dir.join("brightness")).unwrap(), b"2\n");
    }

    #[test]
    fn above_max_backlight_fails_before_mutation() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let dir = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("brightness"), b"1\n").unwrap();
        fs::write(dir.join("max_brightness"), b"3\n").unwrap();
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetKeyboardBacklight(4),
        )
        .expect_err("level above maximum must fail validation");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::BacklightAboveMaximum {
                    level: 4,
                    max: 3
                })
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(fs::read(dir.join("brightness")).unwrap(), b"1\n");
    }

    #[test]
    fn supported_battery_writes_end_only() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
        fs::write(entry.join("charge_control_end_threshold"), b"60\n").unwrap();
        assert!(
            execute_hardware_command(
                fixture.paths.clone(),
                &HardwareCommand::SetBatteryThreshold(threshold())
            )
            .is_ok()
        );
        // The start view stays byte-identical; only the end limit is written.
        assert_eq!(
            fs::read(entry.join("charge_control_start_threshold")).unwrap(),
            b"70\n"
        );
        assert_eq!(
            fs::read(entry.join("charge_control_end_threshold")).unwrap(),
            b"80\n"
        );
    }

    #[test]
    fn unsupported_battery_fails_without_creating_tree() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetBatteryThreshold(threshold()),
        )
        .expect_err("absent thresholds must fail validation");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::UnsupportedCapability(_))
            ),
            "unexpected error: {error:?}"
        );
        assert!(!fixture.root.path().join("sys/class/power_supply").exists());
    }

    #[test]
    fn missing_targets_are_never_created() {
        let fixture = Fixture::new();
        fixture.ready_base();
        assert!(
            execute_hardware_command(
                fixture.paths.clone(),
                &HardwareCommand::SetFanMode(fan("auto"))
            )
            .is_err()
        );
        assert!(
            execute_hardware_command(
                fixture.paths.clone(),
                &HardwareCommand::SetKeyboardBacklight(2)
            )
            .is_err()
        );
        // Only the identity files the fixture itself created may exist.
        assert_eq!(
            fixture.collect_files(),
            vec![
                "sys/class/dmi/id/product_name",
                "sys/class/dmi/id/sys_vendor",
            ]
        );
    }

    #[test]
    fn unwritable_target_surfaces_typed_boundary_error() {
        // A directory where the node belongs: discovery sees it as present
        // and validation passes, but the restricted write fails
        // deterministically on any UID without chmod games.
        let fixture = Fixture::new();
        fixture.ready_base();
        fs::create_dir_all(fixture.ec_dir().join("fan_mode")).unwrap();
        fixture.ec_file("available_fan_modes", b"auto\n");
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetFanMode(fan("auto")),
        )
        .expect_err("unwritable target must surface a boundary error");
        assert!(
            matches!(
                error,
                CommandExecutionError::Boundary(WriteBoundaryError::ExecutionFailed(_))
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn unreadable_battery_start_surfaces_typed_boundary_error() {
        // Presence-only discovery advertises the control, but readback
        // cannot parse the start view after the end limit was already
        // written, so the failure reports may-have-mutated.
        let fixture = Fixture::new();
        fixture.ready_base();
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), b"abc\n").unwrap();
        fs::write(entry.join("charge_control_end_threshold"), b"60\n").unwrap();
        let error = execute_hardware_command(
            fixture.paths.clone(),
            &HardwareCommand::SetBatteryThreshold(threshold()),
        )
        .expect_err("unreadable start view must fail closed");
        assert!(
            matches!(
                error,
                CommandExecutionError::Boundary(WriteBoundaryError::WriteFailed(_))
            ),
            "unexpected error: {error:?}"
        );
        assert!(matches!(
            error,
            CommandExecutionError::Boundary(error) if error.may_have_mutated()
        ));
    }

    #[test]
    fn verification_failed_stays_wrapped_as_boundary() {
        // The composed API returns the executor's error type directly with
        // no conversion, so Task 4 verification semantics survive unchanged.
        let error =
            CommandExecutionError::Boundary(WriteBoundaryError::VerificationFailed("fan mode"));
        assert_eq!(
            error.to_string(),
            "command failed at the write boundary: hardware write verification failed: fan mode"
        );
    }

    #[test]
    fn repeated_calls_reevaluate_without_caching() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto", "silent"]);
        let command = HardwareCommand::SetFanMode(fan("auto"));
        assert!(execute_hardware_command(fixture.paths.clone(), &command).is_ok());
        // Stay READY but stop advertising "auto": the composed service must
        // observe the new state instead of reusing the first verdict.
        fixture.enable_fan_modes("silent", &["silent"]);
        let error = execute_hardware_command(fixture.paths.clone(), &command)
            .expect_err("second call must observe the new capability set");
        assert!(
            matches!(
                error,
                CommandExecutionError::Validation(CommandValidationError::FanModeNotAdvertised(_))
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn composed_api_needs_no_concrete_writer_type() {
        // Compile-level proof: this module never names the concrete writer.
        // Policy plus transport compose behind one function of paths and a
        // typed command.
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("shift_mode", b"eco\n");
        fixture.ec_file("available_shift_modes", b"eco\ncomfort\n");
        let command = HardwareCommand::SetShiftMode(shift("comfort"));
        assert!(execute_hardware_command(fixture.paths.clone(), &command).is_ok());
        assert_eq!(
            fs::read(fixture.ec_dir().join("shift_mode")).unwrap(),
            b"comfort\n"
        );
    }

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

    fn gaming_profile() -> Profile {
        Profile::parse_toml(GAMING_TOML).unwrap()
    }

    /// Full Gaming fixture where every desire differs from readable state,
    /// except battery which is pre-seeded at the matching pair (plain files
    /// cannot emulate kernel start/end coupling).
    fn full_changed_without_battery(fixture: &Fixture) {
        fixture.ready_base();
        fixture.ec_file("shift_mode", b"comfort\n");
        fixture.ec_file("available_shift_modes", b"comfort\nturbo\n");
        fixture.enable_fan_modes("auto", &["auto", "advanced"]);
        fixture.ec_file("cooler_boost", b"off\n");
        fixture.ec_file("super_battery", b"on\n");
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
        fs::write(entry.join("charge_control_end_threshold"), b"80\n").unwrap();
        let backlight = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight");
        fs::create_dir_all(&backlight).unwrap();
        fs::write(backlight.join("brightness"), b"1\n").unwrap();
        fs::write(backlight.join("max_brightness"), b"3\n").unwrap();
    }

    fn battery_entry(fixture: &Fixture, start: &[u8], end: &[u8]) -> PathBuf {
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), start).unwrap();
        fs::write(entry.join("charge_control_end_threshold"), end).unwrap();
        entry
    }

    #[test]
    fn profile_fan_only_applies_exact_mode() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["silent", "auto"]);
        let profile =
            Profile::parse_toml("name = \"Air\"\n\n[performance]\nfan_mode = \"auto\"\n").unwrap();
        let report = apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
        assert_eq!(
            report.applied(),
            &[HardwareCommand::SetFanMode(fan("auto"))]
        );
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"auto\n"
        );
    }

    #[test]
    fn profile_shift_only_applies_exact_mode() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("shift_mode", b"eco\n");
        fixture.ec_file("available_shift_modes", b"eco\ncomfort\n");
        let profile =
            Profile::parse_toml("name = \"Work\"\n\n[performance]\nshift_mode = \"comfort\"\n")
                .unwrap();
        let report = apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
        assert_eq!(
            report.applied(),
            &[HardwareCommand::SetShiftMode(shift("comfort"))]
        );
        assert_eq!(
            fs::read(fixture.ec_dir().join("shift_mode")).unwrap(),
            b"comfort\n"
        );
    }

    #[test]
    fn profile_cooler_boost_applies_both_states() {
        for (start, state, expected) in [("off\n", "true", "on\n"), ("on\n", "false", "off\n")] {
            let fixture = Fixture::new();
            fixture.ready_base();
            let target = fixture.ec_file("cooler_boost", start.as_bytes());
            let input = format!("name = \"Cool\"\n\n[performance]\ncooler_boost = {state}\n");
            let profile = Profile::parse_toml(&input).unwrap();
            apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
            assert_eq!(fs::read(&target).unwrap(), expected.as_bytes());
        }
    }

    #[test]
    fn profile_super_battery_applies_both_states() {
        for (start, state, expected) in [("off\n", "true", "on\n"), ("on\n", "false", "off\n")] {
            let fixture = Fixture::new();
            fixture.ready_base();
            let target = fixture.ec_file("super_battery", start.as_bytes());
            let input = format!("name = \"Saver\"\n\n[performance]\nsuper_battery = {state}\n");
            let profile = Profile::parse_toml(&input).unwrap();
            apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
            assert_eq!(fs::read(&target).unwrap(), expected.as_bytes());
        }
    }

    #[test]
    fn profile_backlight_applies_exact_level() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let dir = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("brightness"), b"1\n").unwrap();
        fs::write(dir.join("max_brightness"), b"3\n").unwrap();
        let profile =
            Profile::parse_toml("name = \"Glow\"\n\n[device]\nkeyboard_backlight = 2\n").unwrap();
        let report = apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
        assert_eq!(
            report.applied(),
            &[HardwareCommand::SetKeyboardBacklight(2)]
        );
        assert_eq!(fs::read(dir.join("brightness")).unwrap(), b"2\n");
    }

    #[test]
    fn profile_battery_mismatch_fails_closed_with_rollback() {
        // Plain files cannot emulate kernel start/end coupling: the writer
        // changes END only, so readback sees start 50 against desired 70 and
        // fails verification. Rollback then restores END from the snapshot.
        let fixture = Fixture::new();
        fixture.ready_base();
        let entry = battery_entry(&fixture, b"50\n", b"60\n");
        let profile =
            Profile::parse_toml("name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 80\n")
                .unwrap();
        let error = apply_profile(fixture.paths.clone(), &profile)
            .expect_err("mismatched readback must fail closed");
        assert!(
            matches!(
                &error,
                ProfileApplyError::Execution(failure)
                if matches!(
                    failure.source(),
                    CommandExecutionError::Boundary(
                        WriteBoundaryError::VerificationFailed("battery thresholds")
                    )
                )
            ),
            "unexpected error: {error:?}"
        );
        assert_eq!(
            fs::read(entry.join("charge_control_start_threshold")).unwrap(),
            b"50\n"
        );
        assert_eq!(
            fs::read(entry.join("charge_control_end_threshold")).unwrap(),
            b"60\n"
        );
    }

    #[test]
    fn profile_full_five_setting_apply_succeeds() {
        let fixture = Fixture::new();
        full_changed_without_battery(&fixture);
        let report =
            apply_profile(fixture.paths.clone(), &gaming_profile()).expect("apply must succeed");
        assert_eq!(
            report.applied(),
            &[
                HardwareCommand::SetShiftMode(shift("turbo")),
                HardwareCommand::SetFanMode(fan("advanced")),
                HardwareCommand::SetCoolerBoost(true),
                HardwareCommand::SetSuperBattery(false),
                HardwareCommand::SetKeyboardBacklight(2),
            ]
        );
        assert_eq!(
            report.unchanged(),
            &[HardwareCommand::SetBatteryThreshold(threshold())]
        );
        assert!(!report.is_noop());
        assert_eq!(
            fs::read(fixture.ec_dir().join("cooler_boost")).unwrap(),
            b"on\n"
        );
    }

    #[test]
    fn profile_noop_leaves_bytes_untouched() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let target = fixture.ec_file("cooler_boost", b"on\n");
        let profile =
            Profile::parse_toml("name = \"Cool\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let report = apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
        assert!(report.is_noop());
        assert!(report.applied().is_empty());
        assert_eq!(report.unchanged(), &[HardwareCommand::SetCoolerBoost(true)]);
        assert_eq!(fs::read(&target).unwrap(), b"on\n");
    }

    #[test]
    fn profile_readonly_fails_before_mutation() {
        let fixture = Fixture::new();
        fixture.write_dmi("sys_vendor", b"Dell Inc.\n");
        fixture.write_dmi("product_name", b"XPS 15\n");
        fixture.ensure_ec_root();
        let target = fixture.ec_file("cooler_boost", b"off\n");
        let profile =
            Profile::parse_toml("name = \"Cool\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        let error =
            apply_profile(fixture.paths.clone(), &profile).expect_err("read-only apply must fail");
        assert!(
            matches!(error, ProfileApplyError::PreviewRejected(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(fs::read(&target).unwrap(), b"off\n");
    }

    #[test]
    fn profile_missing_root_creates_no_tree() {
        let fixture = Fixture::new();
        fixture.write_msi_identity();
        let profile =
            Profile::parse_toml("name = \"Cool\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        assert!(matches!(
            apply_profile(fixture.paths.clone(), &profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert!(!fixture.root.path().join("sys/devices").exists());
    }

    #[test]
    fn profile_unadvertised_fan_fails_before_mutation() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto"]);
        let profile =
            Profile::parse_toml("name = \"Air\"\n\n[performance]\nfan_mode = \"turbo\"\n").unwrap();
        assert!(matches!(
            apply_profile(fixture.paths.clone(), &profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"auto\n"
        );
    }

    #[test]
    fn profile_unsupported_boolean_creates_no_target() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let profile =
            Profile::parse_toml("name = \"Cool\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        assert!(matches!(
            apply_profile(fixture.paths.clone(), &profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert!(!fixture.ec_dir().join("cooler_boost").exists());
    }

    #[test]
    fn profile_backlight_above_max_fails_before_mutation() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let dir = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("brightness"), b"1\n").unwrap();
        fs::write(dir.join("max_brightness"), b"3\n").unwrap();
        let profile =
            Profile::parse_toml("name = \"Glow\"\n\n[device]\nkeyboard_backlight = 4\n").unwrap();
        assert!(matches!(
            apply_profile(fixture.paths.clone(), &profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert_eq!(fs::read(dir.join("brightness")).unwrap(), b"1\n");
    }

    #[test]
    fn profile_missing_battery_support_creates_no_tree() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let profile =
            Profile::parse_toml("name = \"Saver\"\n\n[battery]\ncharge_end_threshold = 80\n")
                .unwrap();
        assert!(matches!(
            apply_profile(fixture.paths.clone(), &profile),
            Err(ProfileApplyError::PreviewRejected(_))
        ));
        assert!(!fixture.root.path().join("sys/class/power_supply").exists());
    }

    #[test]
    fn profile_unadvertised_request_creates_no_node() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("auto", &["auto"]);
        let profile =
            Profile::parse_toml("name = \"Air\"\n\n[performance]\nfan_mode = \"turbo\"\n").unwrap();
        assert!(apply_profile(fixture.paths.clone(), &profile).is_err());
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"auto\n"
        );
    }

    #[test]
    fn profile_apply_leaves_sentinel_untouched() {
        let fixture = Fixture::new();
        full_changed_without_battery(&fixture);
        let sentinel = fixture.root.path().join("sentinel");
        fs::write(&sentinel, b"do-not-touch\n").unwrap();
        apply_profile(fixture.paths.clone(), &gaming_profile()).expect("apply must succeed");
        assert_eq!(fs::read(&sentinel).unwrap(), b"do-not-touch\n");
    }

    #[test]
    fn profile_mode_text_with_separators_stays_content() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.ec_file("fan_mode", b"auto\n");
        fixture.ec_file("available_fan_modes", b"auto\na/b\n");
        let profile =
            Profile::parse_toml("name = \"Odd\"\n\n[performance]\nfan_mode = \"a/b\"\n").unwrap();
        apply_profile(fixture.paths.clone(), &profile).expect("apply must succeed");
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"a/b\n"
        );
        assert!(!fixture.root.path().join("a").exists());
    }

    #[test]
    fn profile_mixed_writes_only_changed_in_order() {
        let fixture = Fixture::new();
        full_changed_without_battery(&fixture);
        fixture.ec_file("fan_mode", b"advanced\n");
        let report =
            apply_profile(fixture.paths.clone(), &gaming_profile()).expect("apply must succeed");
        assert_eq!(
            report.applied(),
            &[
                HardwareCommand::SetShiftMode(shift("turbo")),
                HardwareCommand::SetCoolerBoost(true),
                HardwareCommand::SetSuperBattery(false),
                HardwareCommand::SetKeyboardBacklight(2),
            ]
        );
        assert_eq!(
            report.unchanged(),
            &[
                HardwareCommand::SetFanMode(fan("advanced")),
                HardwareCommand::SetBatteryThreshold(threshold()),
            ]
        );
        assert_eq!(
            fs::read(fixture.ec_dir().join("fan_mode")).unwrap(),
            b"advanced\n"
        );
    }

    #[test]
    fn profile_repeated_apply_becomes_noop() {
        let fixture = Fixture::new();
        fixture.ready_base();
        fixture.enable_fan_modes("silent", &["silent", "auto"]);
        let profile =
            Profile::parse_toml("name = \"Air\"\n\n[performance]\nfan_mode = \"auto\"\n").unwrap();
        let first = apply_profile(fixture.paths.clone(), &profile).expect("first apply");
        assert!(!first.is_noop());
        let second = apply_profile(fixture.paths.clone(), &profile).expect("second apply");
        assert!(second.is_noop());
        assert_eq!(
            second.unchanged(),
            &[HardwareCommand::SetFanMode(fan("auto"))]
        );
    }

    #[test]
    fn profile_capability_removal_fails_second_apply_closed() {
        let fixture = Fixture::new();
        fixture.ready_base();
        let target = fixture.ec_file("cooler_boost", b"off\n");
        let profile =
            Profile::parse_toml("name = \"Cool\"\n\n[performance]\ncooler_boost = true\n").unwrap();
        apply_profile(fixture.paths.clone(), &profile).expect("first apply");
        assert_eq!(fs::read(&target).unwrap(), b"on\n");
        fs::remove_file(&target).unwrap();
        let error = apply_profile(fixture.paths.clone(), &profile)
            .expect_err("second apply must fail closed");
        assert!(
            matches!(error, ProfileApplyError::PreviewRejected(_)),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn profile_rollback_restores_earlier_mutation() {
        // Cooler applies, then battery readback mismatches on plain files
        // (writer changes END only). Rollback must restore both nodes.
        let fixture = Fixture::new();
        fixture.ready_base();
        let cooler = fixture.ec_file("cooler_boost", b"off\n");
        let entry = battery_entry(&fixture, b"50\n", b"60\n");
        let profile = Profile::parse_toml(concat!(
            "name = \"Mixed\"\n",
            "\n",
            "[performance]\n",
            "cooler_boost = true\n",
            "\n",
            "[battery]\n",
            "charge_end_threshold = 80\n",
        ))
        .unwrap();
        let error = apply_profile(fixture.paths.clone(), &profile)
            .expect_err("battery readback must fail on plain files");
        let ProfileApplyError::Execution(failure) = &error else {
            panic!("expected execution failure, got {error:?}");
        };
        assert_eq!(
            failure.failed_command(),
            &HardwareCommand::SetBatteryThreshold(threshold())
        );
        assert_eq!(failure.rollback_attempts().len(), 2);
        assert!(
            failure
                .rollback_attempts()
                .iter()
                .all(|attempt| attempt.result().is_ok())
        );
        assert_eq!(fs::read(&cooler).unwrap(), b"off\n");
        assert_eq!(
            fs::read(entry.join("charge_control_start_threshold")).unwrap(),
            b"50\n"
        );
        assert_eq!(
            fs::read(entry.join("charge_control_end_threshold")).unwrap(),
            b"60\n"
        );
    }
}
