//! Narrow TUI execution adapter: the only TUI module allowed to call the
//! production safe APIs.
//!
//! [`TuiMutationExecutor`] exposes exactly two operations — one typed
//! command, one retained profile — so tests inject a fake that counts
//! calls and records exact typed values. The production adapter owns the
//! same [`SystemPaths`] root used to compose the read side and delegates
//! ONLY to [`execute_hardware_command`] and [`apply_profile`]. No boundary
//! types, no writer types, no sysfs writes, no shell, no elevation.

use crate::hardware::{HardwareCommand, SystemPaths};
use crate::profiles::Profile;
use crate::safety::{
    CommandExecutionError, ProfileApplyError, ProfileApplyReport, apply_profile,
    execute_hardware_command,
};

/// Counts for a successful profile application, for the result notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileApplySummary {
    /// Profile display name.
    pub name: String,
    /// Forward commands applied.
    pub applied: usize,
    /// Requested commands already satisfied.
    pub unchanged: usize,
}

impl ProfileApplySummary {
    /// Builds counts from a production report using public accessors only.
    pub fn from_report(report: &ProfileApplyReport) -> Self {
        Self {
            name: report.name().as_str().to_owned(),
            applied: report.applied().len(),
            unchanged: report.unchanged().len(),
        }
    }
}

/// Test / production execution boundary for confirmed TUI mutations.
/// Object-safe so production uses a boxed trait object and tests inject a
/// fake. `&mut` lets fakes record calls without interior mutability.
pub trait TuiMutationExecutor {
    /// Executes one typed command through the safe pipeline.
    fn execute_command(&mut self, command: &HardwareCommand) -> Result<(), CommandExecutionError>;

    /// Applies one retained profile through the safe transaction pipeline.
    fn apply_profile(
        &mut self,
        profile: &Profile,
    ) -> Result<ProfileApplySummary, ProfileApplyError>;
}

/// Production adapter: same [`SystemPaths`] root for reads and writes,
/// delegation only to the two safe APIs.
pub struct SafeTuiExecutor {
    paths: SystemPaths,
}

impl SafeTuiExecutor {
    /// Owns a clone of the same root used to compose the read side.
    pub fn new(paths: SystemPaths) -> Self {
        Self { paths }
    }
}

impl TuiMutationExecutor for SafeTuiExecutor {
    fn execute_command(&mut self, command: &HardwareCommand) -> Result<(), CommandExecutionError> {
        execute_hardware_command(self.paths.clone(), command)
    }

    fn apply_profile(
        &mut self,
        profile: &Profile,
    ) -> Result<ProfileApplySummary, ProfileApplyError> {
        apply_profile(self.paths.clone(), profile)
            .map(|report| ProfileApplySummary::from_report(&report))
    }
}

/// Fake executor for tests: counts calls and records exact typed values.
/// Configured results let failure paths stay deterministic without touching
/// `/sys` or tracked fixtures.
#[derive(Debug, Default)]
pub struct FakeTuiExecutor {
    commands: Vec<HardwareCommand>,
    profiles: Vec<Profile>,
    command_error: Option<CommandExecutionError>,
    profile_error: Option<ProfileApplyError>,
    profile_summary: Option<ProfileApplySummary>,
}

impl FakeTuiExecutor {
    /// Fresh fake succeeding with default counts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Next command call fails with this typed error.
    pub fn with_command_error(error: CommandExecutionError) -> Self {
        Self {
            command_error: Some(error),
            ..Self::default()
        }
    }

    /// Next profile call fails with this typed error.
    pub fn with_profile_error(error: ProfileApplyError) -> Self {
        Self {
            profile_error: Some(error),
            ..Self::default()
        }
    }

    /// Successful profile counts for the notice.
    pub fn with_profile_summary(summary: ProfileApplySummary) -> Self {
        Self {
            profile_summary: Some(summary),
            ..Self::default()
        }
    }

    /// How many command executions reached the fake.
    pub fn command_calls(&self) -> usize {
        self.commands.len()
    }

    /// How many profile applies reached the fake.
    pub fn profile_calls(&self) -> usize {
        self.profiles.len()
    }

    /// Exact typed commands received, in order.
    pub fn received_commands(&self) -> &[HardwareCommand] {
        &self.commands
    }

    /// Exact retained profiles received, in order.
    pub fn received_profiles(&self) -> &[Profile] {
        &self.profiles
    }
}

impl TuiMutationExecutor for FakeTuiExecutor {
    fn execute_command(&mut self, command: &HardwareCommand) -> Result<(), CommandExecutionError> {
        self.commands.push(command.clone());
        match self.command_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn apply_profile(
        &mut self,
        profile: &Profile,
    ) -> Result<ProfileApplySummary, ProfileApplyError> {
        self.profiles.push(profile.clone());
        if let Some(error) = self.profile_error.take() {
            return Err(error);
        }
        Ok(self.profile_summary.clone().unwrap_or(ProfileApplySummary {
            name: profile.name().as_str().to_owned(),
            applied: 1,
            unchanged: 0,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{CommandValidationError, FanMode};

    fn command() -> HardwareCommand {
        HardwareCommand::SetFanMode(FanMode::try_from("silent").unwrap())
    }

    #[test]
    fn fake_records_exact_command() {
        let mut fake = FakeTuiExecutor::new();
        fake.execute_command(&command()).expect("fake succeeds");
        assert_eq!(fake.command_calls(), 1);
        assert_eq!(fake.received_commands(), &[command()]);
        assert_eq!(fake.profile_calls(), 0);
    }

    #[test]
    fn fake_returns_configured_command_error() {
        let mut fake = FakeTuiExecutor::with_command_error(CommandExecutionError::Validation(
            CommandValidationError::ReadOnly,
        ));
        let error = fake.execute_command(&command()).unwrap_err();
        assert!(error.to_string().contains("read-only"));
        assert_eq!(fake.command_calls(), 1);
    }

    #[test]
    fn production_adapter_writes_through_safe_pipeline_on_tempdir() {
        use crate::hardware::SystemPaths;
        use std::fs;
        use tempfile::tempdir;
        // READY fake MSI root with one writable fan control. Never touches
        // tracked fixtures or physical /sys.
        let root = tempdir().expect("TempDir constructs");
        let paths = SystemPaths::new(root.path());
        let dmi = root.path().join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).expect("dmi constructs");
        fs::write(
            dmi.join("sys_vendor"),
            b"Micro-Star International Co., Ltd.\n",
        )
        .expect("vendor writes");
        fs::write(dmi.join("product_name"), b"Temp MSI\n").expect("product writes");
        let ec = root.path().join("sys/devices/platform/msi-ec");
        fs::create_dir_all(&ec).expect("ec constructs");
        fs::write(ec.join("fan_mode"), b"auto\n").expect("mode writes");
        fs::write(ec.join("available_fan_modes"), b"auto\nsilent\n").expect("modes write");
        let mut executor = SafeTuiExecutor::new(paths);
        executor
            .execute_command(&command())
            .expect("tempdir fan write succeeds");
        assert_eq!(
            fs::read(ec.join("fan_mode")).expect("readback"),
            b"silent\n"
        );
    }

    #[test]
    fn production_adapter_read_only_leaves_tempdir_untouched() {
        use crate::hardware::SystemPaths;
        use std::fs;
        use tempfile::tempdir;
        let root = tempdir().expect("TempDir constructs");
        let paths = SystemPaths::new(root.path());
        let dmi = root.path().join("sys/class/dmi/id");
        fs::create_dir_all(&dmi).expect("dmi constructs");
        fs::write(dmi.join("sys_vendor"), b"Dell Inc.\n").expect("vendor writes");
        fs::write(dmi.join("product_name"), b"XPS\n").expect("product writes");
        let ec = root.path().join("sys/devices/platform/msi-ec");
        fs::create_dir_all(&ec).expect("ec constructs");
        fs::write(ec.join("cooler_boost"), b"off\n").expect("boost writes");
        let mut executor = SafeTuiExecutor::new(paths);
        let error = executor
            .execute_command(&crate::hardware::HardwareCommand::SetCoolerBoost(true))
            .expect_err("non-MSI must stay read-only");
        assert!(error.to_string().contains("read-only"));
        assert_eq!(
            fs::read(ec.join("cooler_boost")).expect("readback"),
            b"off\n"
        );
    }

    #[test]
    fn fake_profile_summary_defaults_to_input_name() {
        use crate::profiles::ProfileStore;

        let dir = tempfile::tempdir().expect("fake TempDir constructs");
        let store = ProfileStore::new(dir.path().join("profiles"));
        std::fs::create_dir_all(store.directory()).expect("profile dir constructs");
        std::fs::write(
            store.directory().join("work.toml"),
            b"name = \"Fake Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        )
        .expect("profile writes");
        let profile = store.load(&"work".parse().unwrap()).expect("profile loads");
        let mut fake = FakeTuiExecutor::new();
        let summary = fake.apply_profile(&profile).expect("fake succeeds");
        assert_eq!(summary.name, "Fake Work");
        assert_eq!(fake.profile_calls(), 1);
        assert_eq!(fake.received_profiles(), &[profile]);
    }
}
