//! Restricted `msi-ec` sysfs writer with mandatory readback verification.
//!
//! [`MsiEcSysfsWriteBoundary`] is the single production implementation of
//! [`HardwareWriteBoundary`]. It maps each [`HardwareCommand`] variant to
//! one fixed sysfs target, writes the exact payload into the already
//! existing node, drops the write handle, and returns `Ok(())` only after
//! readback shows the requested state. There is no generic path/value API:
//! targets are hard-coded literals, never derived from command or user data.
//!
//! The boundary performs no validation itself. [`HardwareCommand::validate`]
//! runs in the executor immediately before every crossing.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    BatteryThreshold, HardwareCommand, HardwareWriteBoundary, SysfsError, SysfsReader, SystemPaths,
    WriteBoundaryError,
};

/// Delay between readback polls while EC-backed sysfs settles. Physical
/// measurement (GF63 Thin 11UC): a stale value can persist ~0.4 ms after
/// the write and converges well within a few milliseconds.
const VERIFY_RETRY_DELAY: Duration = Duration::from_millis(2);

/// Delayed readback polls after the immediate read. One immediate read
/// plus up to five delayed reads bound the settling window to
/// approximately 10 ms.
const VERIFY_DELAYED_POLLS: usize = 5;

/// Production `msi-ec` sysfs boundary: fixed mapping, existing-node writes
/// only, mandatory readback. Crate-private; the public abstraction remains
/// [`HardwareWriteBoundary`]. Performs no privilege elevation and no
/// validation.
pub(crate) struct MsiEcSysfsWriteBoundary<R> {
    paths: SystemPaths,
    reader: R,
    /// Readback settling sleeper. Production uses [`std::thread::sleep`];
    /// tests substitute a no-op or recording sleeper for determinism.
    sleeper: fn(Duration),
}

impl<R> MsiEcSysfsWriteBoundary<R>
where
    R: SysfsReader,
{
    /// Creates a writer over `paths` using `reader` for existence checks
    /// and readback. Stores no commands and no safety verdicts.
    pub(crate) fn new(paths: SystemPaths, reader: R) -> Self {
        Self {
            paths,
            reader,
            sleeper: std::thread::sleep,
        }
    }

    /// Resolves a hard-coded platform attribute. The literals at the call
    /// sites are always valid; the mapping keeps the function total without
    /// panicking and without leaking paths.
    fn platform_path(
        &self,
        attribute: &'static str,
        field: &'static str,
    ) -> Result<PathBuf, WriteBoundaryError> {
        self.paths
            .msi_ec(attribute)
            .map_err(|_| WriteBoundaryError::ExecutionFailed(format!("{field} target")))
    }

    /// Bounded readback settling driver. Polls once immediately, then up
    /// to [`VERIFY_DELAYED_POLLS`] delayed polls spaced by
    /// [`VERIFY_RETRY_DELAY`]. Only semantic mismatches (`Ok(false)`)
    /// retry; structural read errors return at once. Never writes: the
    /// single hardware write always happens before this runs.
    fn settle_readback(
        &self,
        mut settled: impl FnMut() -> Result<bool, WriteBoundaryError>,
        field: &'static str,
    ) -> Result<(), WriteBoundaryError> {
        if settled()? {
            return Ok(());
        }
        for _ in 0..VERIFY_DELAYED_POLLS {
            (self.sleeper)(VERIFY_RETRY_DELAY);
            if settled()? {
                return Ok(());
            }
        }
        Err(WriteBoundaryError::VerificationFailed(field))
    }

    /// One semantic string poll: `Ok(true)` means settled, `Ok(false)` a
    /// mismatch worth polling. Structural errors surface at once with the
    /// existing post-write mapping.
    fn poll_string(
        &self,
        target: &Path,
        expected: &str,
        field: &'static str,
    ) -> Result<bool, WriteBoundaryError> {
        match self.reader.read_string(target) {
            Ok(actual) => Ok(actual == expected),
            Err(error) => Err(map_post_write_read_error(error, field)),
        }
    }

    /// One semantic `u8` poll, mirroring [`Self::poll_string`].
    fn poll_u8(
        &self,
        target: &Path,
        expected: u8,
        field: &'static str,
    ) -> Result<bool, WriteBoundaryError> {
        match self.reader.read_u8(target) {
            Ok(actual) => Ok(actual == expected),
            Err(error) => Err(map_post_write_read_error(error, field)),
        }
    }

    /// Writes one boolean control and verifies the semantic `on`/`off`
    /// state by bounded readback polling.
    fn write_bool(
        &self,
        attribute: &'static str,
        field: &'static str,
        value: bool,
    ) -> Result<(), WriteBoundaryError> {
        let target = self.platform_path(attribute, field)?;
        let expected = if value { "on" } else { "off" };
        write_existing_text(&target, &format!("{expected}\n"), field)?;
        self.settle_readback(|| self.poll_string(&target, expected, field), field)
    }

    /// Selects the first sorted power-supply entry exposing both threshold
    /// attributes, then writes the end limit and verifies both views.
    fn write_threshold(&self, threshold: BatteryThreshold) -> Result<(), WriteBoundaryError> {
        const FIELD: &str = "battery thresholds";
        let entry = self.select_threshold_entry()?;
        // `msi-ec` exposes one EC charge-control state through both
        // attributes: writing the end limit is sufficient and avoids a
        // redundant second EC write.
        let end_target = entry.join("charge_control_end_threshold");
        let start_target = entry.join("charge_control_start_threshold");
        write_existing_text(
            &end_target,
            &format!("{}\n", threshold.end_percent()),
            FIELD,
        )?;
        // The EC state behind both attributes may settle after the single
        // end-limit write: poll the pair, never re-write.
        self.settle_readback(
            || {
                let start = self
                    .reader
                    .read_u8(&start_target)
                    .map_err(|error| map_post_write_read_error(error, FIELD))?;
                let end = self
                    .reader
                    .read_u8(&end_target)
                    .map_err(|error| map_post_write_read_error(error, FIELD))?;
                Ok(start == threshold.start_percent() && end == threshold.end_percent())
            },
            FIELD,
        )
    }

    /// Deterministically selects the first sorted entry with a complete
    /// threshold pair. Half-present pairs fail closed; no entry means the
    /// control does not exist. Never creates files.
    fn select_threshold_entry(&self) -> Result<PathBuf, WriteBoundaryError> {
        const FIELD: &str = "battery thresholds";
        let entries = self
            .reader
            .list_entries(&self.paths.power_supply_root())
            .map_err(|error| match error {
                SysfsError::NotFound(_) => WriteBoundaryError::Unavailable,
                SysfsError::PermissionDenied(_) => WriteBoundaryError::AccessDenied,
                _ => {
                    WriteBoundaryError::ExecutionFailed("battery thresholds entry scan".to_owned())
                }
            })?;
        for entry in &entries {
            let start_present = self
                .reader
                .exists(&entry.join("charge_control_start_threshold"))
                .map_err(|error| map_discovery_error(error, FIELD))?;
            let end_present = self
                .reader
                .exists(&entry.join("charge_control_end_threshold"))
                .map_err(|error| map_discovery_error(error, FIELD))?;
            match (start_present, end_present) {
                (true, true) => return Ok(entry.clone()),
                (false, false) => {}
                _ => {
                    return Err(WriteBoundaryError::ExecutionFailed(
                        "battery thresholds inconsistent".to_owned(),
                    ));
                }
            }
        }
        Err(WriteBoundaryError::Unavailable)
    }
}

impl<R> HardwareWriteBoundary for MsiEcSysfsWriteBoundary<R>
where
    R: SysfsReader,
{
    fn execute(&self, command: &HardwareCommand) -> Result<(), WriteBoundaryError> {
        match command {
            HardwareCommand::SetFanMode(mode) => {
                const FIELD: &str = "fan mode";
                let target = self.platform_path("fan_mode", FIELD)?;
                write_existing_text(&target, &format!("{}\n", mode.as_str()), FIELD)?;
                self.settle_readback(|| self.poll_string(&target, mode.as_str(), FIELD), FIELD)
            }
            HardwareCommand::SetShiftMode(mode) => {
                const FIELD: &str = "shift mode";
                let target = self.platform_path("shift_mode", FIELD)?;
                write_existing_text(&target, &format!("{}\n", mode.as_str()), FIELD)?;
                self.settle_readback(|| self.poll_string(&target, mode.as_str(), FIELD), FIELD)
            }
            HardwareCommand::SetCoolerBoost(value) => {
                self.write_bool("cooler_boost", "cooler boost", *value)
            }
            HardwareCommand::SetSuperBattery(value) => {
                self.write_bool("super_battery", "super battery", *value)
            }
            HardwareCommand::SetWebcam(value) => self.write_bool("webcam", "webcam", *value),
            HardwareCommand::SetWebcamBlock(value) => {
                self.write_bool("webcam_block", "webcam block", *value)
            }
            HardwareCommand::SetKeyboardBacklight(level) => {
                const FIELD: &str = "keyboard backlight";
                let target = self
                    .paths
                    .leds_root()
                    .join("msiacpi::kbd_backlight")
                    .join("brightness");
                write_existing_text(&target, &format!("{level}\n"), FIELD)?;
                self.settle_readback(|| self.poll_u8(&target, *level, FIELD), FIELD)
            }
            HardwareCommand::SetBatteryThreshold(threshold) => self.write_threshold(*threshold),
        }
    }
}

/// Writes `payload` into an already-existing node. Never creates a missing
/// node, never appends. The handle drops on return, before any readback.
/// Opening uses truncate, so once the open succeeds the target may already
/// be affected: only open failures are pre-write; write failures after a
/// successful open are classified as may-have-mutated.
fn write_existing_text(
    target: &Path,
    payload: &str,
    field: &'static str,
) -> Result<(), WriteBoundaryError> {
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(false)
        .open(target)
        .map_err(|error| map_open_error(error, field))?;
    file.write_all(payload.as_bytes())
        .map_err(|error| map_post_open_write_error(error, field))?;
    Ok(())
}

/// Maps target-open failures: no write-capable handle existed, so nothing
/// could have mutated. Never leaks paths.
fn map_open_error(error: std::io::Error, field: &'static str) -> WriteBoundaryError {
    match error.kind() {
        std::io::ErrorKind::NotFound => WriteBoundaryError::Unavailable,
        std::io::ErrorKind::PermissionDenied => WriteBoundaryError::AccessDenied,
        _ => WriteBoundaryError::ExecutionFailed(format!("{field} write")),
    }
}

/// Maps failures writing through an already-opened handle: truncate may
/// already have taken effect, so mutation cannot be ruled out whatever the
/// I/O kind. Never leaks paths.
fn map_post_open_write_error(error: std::io::Error, field: &'static str) -> WriteBoundaryError {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            WriteBoundaryError::WriteFailed(format!("{field} write target vanished"))
        }
        std::io::ErrorKind::PermissionDenied => {
            WriteBoundaryError::WriteFailed(format!("{field} write access denied"))
        }
        _ => WriteBoundaryError::WriteFailed(format!("{field} write")),
    }
}

/// Maps target/entry discovery failures: they occur before any
/// write-capable handle exists. Never leaks paths.
fn map_discovery_error(error: SysfsError, field: &'static str) -> WriteBoundaryError {
    match error {
        SysfsError::NotFound(_) => WriteBoundaryError::Unavailable,
        SysfsError::PermissionDenied(_) => WriteBoundaryError::AccessDenied,
        _ => WriteBoundaryError::ExecutionFailed(format!("{field} readback")),
    }
}

/// Maps readback failures after a write was attempted: the target was
/// already opened for writing, so mutation cannot be ruled out whatever
/// the read failure kind. Never leaks paths.
fn map_post_write_read_error(error: SysfsError, field: &'static str) -> WriteBoundaryError {
    match error {
        SysfsError::NotFound(_) => {
            WriteBoundaryError::WriteFailed(format!("{field} readback missing"))
        }
        SysfsError::PermissionDenied(_) => {
            WriteBoundaryError::WriteFailed(format!("{field} readback access denied"))
        }
        _ => WriteBoundaryError::WriteFailed(format!("{field} readback")),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::{TempDir, tempdir};

    use super::*;
    use crate::hardware::{FanMode, LinuxSysfsReader, ShiftMode};

    /// Test-only reader decorator: delegates everything except scripted
    /// read overrides/failures, so readback behavior can be driven without
    /// depending on process UID or file modes.
    #[derive(Clone)]
    struct ControlledReads<R> {
        inner: R,
        string_override: Option<String>,
        u8_override: Option<u8>,
        start_override: Option<u8>,
        end_override: Option<u8>,
        read_failure: Option<ReadFailure>,
    }

    #[derive(Debug, Clone, Copy)]
    enum ReadFailure {
        Denied,
        Missing,
        Broken,
    }

    impl<R> ControlledReads<R> {
        fn plain(inner: R) -> Self {
            Self {
                inner,
                string_override: None,
                u8_override: None,
                start_override: None,
                end_override: None,
                read_failure: None,
            }
        }

        fn failure(&self, path: &Path) -> Option<SysfsError> {
            match self.read_failure {
                None => None,
                Some(ReadFailure::Denied) => Some(SysfsError::PermissionDenied(path.to_path_buf())),
                Some(ReadFailure::Missing) => Some(SysfsError::NotFound(path.to_path_buf())),
                Some(ReadFailure::Broken) => Some(SysfsError::InvalidValue {
                    path: path.to_path_buf(),
                    value: "junk".to_owned(),
                }),
            }
        }
    }

    impl<R: SysfsReader> SysfsReader for ControlledReads<R> {
        fn exists(&self, path: &Path) -> Result<bool, SysfsError> {
            self.inner.exists(path)
        }

        fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
            if let Some(error) = self.failure(path) {
                return Err(error);
            }
            if let Some(value) = &self.string_override {
                return Ok(value.clone());
            }
            self.inner.read_string(path)
        }

        fn read_u8(&self, path: &Path) -> Result<u8, SysfsError> {
            if let Some(error) = self.failure(path) {
                return Err(error);
            }
            if path.ends_with("charge_control_start_threshold")
                && let Some(value) = self.start_override
            {
                return Ok(value);
            }
            if path.ends_with("charge_control_end_threshold")
                && let Some(value) = self.end_override
            {
                return Ok(value);
            }
            if let Some(value) = self.u8_override {
                return Ok(value);
            }
            self.inner.read_u8(path)
        }

        fn read_u16(&self, path: &Path) -> Result<u16, SysfsError> {
            self.inner.read_u16(path)
        }

        fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_dirs(path)
        }

        fn list_entries(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_entries(path)
        }
    }

    /// Test-only readback sequencer: every verification read pops the
    /// front scripted value; an empty queue delegates to the real file.
    /// Counts each verification read; an optional persistent structural
    /// failure proves read errors are never retried.
    struct ScriptedReads<R> {
        inner: R,
        strings: RefCell<VecDeque<String>>,
        levels: RefCell<VecDeque<u8>>,
        starts: RefCell<VecDeque<u8>>,
        ends: RefCell<VecDeque<u8>>,
        failure: Option<ReadFailure>,
        reads: Rc<Cell<usize>>,
    }

    impl ScriptedReads<LinuxSysfsReader> {
        fn sequenced() -> (Self, Rc<Cell<usize>>) {
            let reads = Rc::new(Cell::new(0));
            (
                Self {
                    inner: LinuxSysfsReader,
                    strings: RefCell::new(VecDeque::new()),
                    levels: RefCell::new(VecDeque::new()),
                    starts: RefCell::new(VecDeque::new()),
                    ends: RefCell::new(VecDeque::new()),
                    failure: None,
                    reads: Rc::clone(&reads),
                },
                reads,
            )
        }
    }

    impl<R> ScriptedReads<R> {
        fn scripted_failure(&self, path: &Path) -> Option<SysfsError> {
            match self.failure {
                None => None,
                Some(ReadFailure::Denied) => Some(SysfsError::PermissionDenied(path.to_path_buf())),
                Some(ReadFailure::Missing) => Some(SysfsError::NotFound(path.to_path_buf())),
                Some(ReadFailure::Broken) => Some(SysfsError::InvalidValue {
                    path: path.to_path_buf(),
                    value: "junk".to_owned(),
                }),
            }
        }
    }

    impl<R: SysfsReader> SysfsReader for ScriptedReads<R> {
        fn exists(&self, path: &Path) -> Result<bool, SysfsError> {
            self.inner.exists(path)
        }

        fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
            self.reads.set(self.reads.get() + 1);
            if let Some(error) = self.scripted_failure(path) {
                return Err(error);
            }
            if let Some(value) = self.strings.borrow_mut().pop_front() {
                return Ok(value);
            }
            self.inner.read_string(path)
        }

        fn read_u8(&self, path: &Path) -> Result<u8, SysfsError> {
            self.reads.set(self.reads.get() + 1);
            if let Some(error) = self.scripted_failure(path) {
                return Err(error);
            }
            if path.ends_with("charge_control_start_threshold") {
                if let Some(value) = self.starts.borrow_mut().pop_front() {
                    return Ok(value);
                }
            } else if path.ends_with("charge_control_end_threshold") {
                if let Some(value) = self.ends.borrow_mut().pop_front() {
                    return Ok(value);
                }
            } else if let Some(value) = self.levels.borrow_mut().pop_front() {
                return Ok(value);
            }
            self.inner.read_u8(path)
        }

        fn read_u16(&self, path: &Path) -> Result<u16, SysfsError> {
            self.inner.read_u16(path)
        }

        fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_dirs(path)
        }

        fn list_entries(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
            self.inner.list_entries(path)
        }
    }

    /// Deterministic test sleeper: never waits.
    fn no_op_sleep(_: Duration) {}

    /// Sleep-call counter for proving the bounded retry cadence. Tests
    /// that assert counts hold [`sleep_guard`] so parallel tests cannot
    /// pollute the shared counter.
    static SLEEP_CALLS: AtomicUsize = AtomicUsize::new(0);

    static SLEEP_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Recording test sleeper: counts polls without waiting.
    fn counting_sleep(_: Duration) {
        SLEEP_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    fn reset_sleep_calls() {
        SLEEP_CALLS.store(0, Ordering::SeqCst);
    }

    fn sleep_calls() -> usize {
        SLEEP_CALLS.load(Ordering::SeqCst)
    }

    /// Holds the sleep-counter guard and resets the count. Every test
    /// asserting sleep counts must hold the returned guard for the whole
    /// arrange/act/assert span.
    fn sleep_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = SLEEP_GUARD.lock().unwrap();
        reset_sleep_calls();
        guard
    }

    /// Test-only fake root. Never touches `/sys`; every test builds only
    /// the nodes it needs under a `TempDir`.
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

        fn writer(&self) -> MsiEcSysfsWriteBoundary<LinuxSysfsReader> {
            let mut boundary = MsiEcSysfsWriteBoundary::new(self.paths.clone(), LinuxSysfsReader);
            boundary.sleeper = no_op_sleep;
            boundary
        }

        fn controlled(
            &self,
            reads: ControlledReads<LinuxSysfsReader>,
        ) -> MsiEcSysfsWriteBoundary<ControlledReads<LinuxSysfsReader>> {
            let mut boundary = MsiEcSysfsWriteBoundary::new(self.paths.clone(), reads);
            boundary.sleeper = no_op_sleep;
            boundary
        }

        fn settling(
            &self,
            reads: ScriptedReads<LinuxSysfsReader>,
            sleeper: fn(Duration),
        ) -> MsiEcSysfsWriteBoundary<ScriptedReads<LinuxSysfsReader>> {
            let mut boundary = MsiEcSysfsWriteBoundary::new(self.paths.clone(), reads);
            boundary.sleeper = sleeper;
            boundary
        }

        fn ec_file(&self, relative: &str, contents: &[u8]) -> PathBuf {
            let path = self
                .root
                .path()
                .join("sys/devices/platform/msi-ec")
                .join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        fn ec_path(&self, relative: &str) -> PathBuf {
            self.root
                .path()
                .join("sys/devices/platform/msi-ec")
                .join(relative)
        }

        fn backlight_file(&self, contents: &[u8]) -> PathBuf {
            let dir = self
                .root
                .path()
                .join("sys/class/leds/msiacpi::kbd_backlight");
            fs::create_dir_all(&dir).unwrap();
            let path = dir.join("brightness");
            fs::write(&path, contents).unwrap();
            path
        }

        fn battery_entry(&self, name: &str, start: &[u8], end: &[u8]) -> PathBuf {
            let entry = self.root.path().join("sys/class/power_supply").join(name);
            fs::create_dir_all(&entry).unwrap();
            fs::write(entry.join("charge_control_start_threshold"), start).unwrap();
            fs::write(entry.join("charge_control_end_threshold"), end).unwrap();
            entry
        }

        fn read_raw(&self, path: &Path) -> Vec<u8> {
            fs::read(path).unwrap()
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
    fn fan_mode_writes_exact_requested_mode() {
        let fixture = Fixture::new();
        let target = fixture.ec_file("fan_mode", b"silent\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetFanMode(fan("auto")))
                .is_ok()
        );
        assert_eq!(fixture.read_raw(&target), b"auto\n");
    }

    #[test]
    fn fan_mode_shorter_replacement_fully_truncates() {
        let fixture = Fixture::new();
        let target = fixture.ec_file("fan_mode", b"advanced\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetFanMode(fan("auto")))
                .is_ok()
        );
        assert_eq!(fixture.read_raw(&target), b"auto\n");
    }

    #[test]
    fn shift_mode_writes_exact_requested_mode() {
        let fixture = Fixture::new();
        let target = fixture.ec_file("shift_mode", b"eco\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetShiftMode(shift("sport")))
                .is_ok()
        );
        assert_eq!(fixture.read_raw(&target), b"sport\n");
    }

    #[test]
    fn cooler_boost_writes_on_and_off() {
        for (value, expected) in [(false, b"off\n".as_slice()), (true, b"on\n".as_slice())] {
            let fixture = Fixture::new();
            let target = fixture.ec_file("cooler_boost", b"sentinel\n");
            assert!(
                fixture
                    .writer()
                    .execute(&HardwareCommand::SetCoolerBoost(value))
                    .is_ok()
            );
            assert_eq!(fixture.read_raw(&target), expected);
        }
    }

    #[test]
    fn super_battery_writes_both_states() {
        for value in [true, false] {
            let fixture = Fixture::new();
            let target = fixture.ec_file("super_battery", b"off\n");
            let command = HardwareCommand::SetSuperBattery(value);
            assert!(fixture.writer().execute(&command).is_ok());
            let expected = if value { "on\n" } else { "off\n" };
            assert_eq!(fixture.read_raw(&target), expected.as_bytes());
        }
    }

    #[test]
    fn webcam_writes_both_states() {
        for value in [true, false] {
            let fixture = Fixture::new();
            let target = fixture.ec_file("webcam", b"on\n");
            assert!(
                fixture
                    .writer()
                    .execute(&HardwareCommand::SetWebcam(value))
                    .is_ok()
            );
            let expected = if value { "on\n" } else { "off\n" };
            assert_eq!(fixture.read_raw(&target), expected.as_bytes());
        }
    }

    #[test]
    fn webcam_block_writes_both_states() {
        for value in [true, false] {
            let fixture = Fixture::new();
            let target = fixture.ec_file("webcam_block", b"off\n");
            assert!(
                fixture
                    .writer()
                    .execute(&HardwareCommand::SetWebcamBlock(value))
                    .is_ok()
            );
            let expected = if value { "on\n" } else { "off\n" };
            assert_eq!(fixture.read_raw(&target), expected.as_bytes());
        }
    }

    #[test]
    fn keyboard_backlight_writes_decimal_and_verifies() {
        let fixture = Fixture::new();
        let target = fixture.backlight_file(b"1\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetKeyboardBacklight(2))
                .is_ok()
        );
        assert_eq!(fixture.read_raw(&target), b"2\n");
    }

    #[test]
    fn battery_selects_first_sorted_complete_entry() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.path().join("sys/class/power_supply/AC")).unwrap();
        fixture.battery_entry("BAT0", b"70\n", b"60\n");
        let other = fixture.battery_entry("BAT1", b"50\n", b"60\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetBatteryThreshold(threshold()))
                .is_ok()
        );
        // First sorted complete entry wins; the later entry is untouched.
        assert_eq!(
            fixture.read_raw(&other.join("charge_control_end_threshold")),
            b"60\n"
        );
    }

    #[test]
    fn battery_writes_only_end_and_verifies_both() {
        let fixture = Fixture::new();
        let entry = fixture.battery_entry("BAT0", b"70\n", b"60\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetBatteryThreshold(threshold()))
                .is_ok()
        );
        // The start file keeps its exact bytes: the writer never modifies it.
        assert_eq!(
            fixture.read_raw(&entry.join("charge_control_start_threshold")),
            b"70\n"
        );
        assert_eq!(
            fixture.read_raw(&entry.join("charge_control_end_threshold")),
            b"80\n"
        );
    }

    #[test]
    fn battery_without_complete_pair_is_unavailable() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.root.path().join("sys/class/power_supply/AC")).unwrap();
        assert_eq!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetBatteryThreshold(threshold())),
            Err(WriteBoundaryError::Unavailable)
        );
        // Fail closed: no threshold files may appear.
        assert!(
            fixture
                .collect_files()
                .iter()
                .all(|file| !file.contains("charge_control"))
        )
    }

    #[test]
    fn battery_half_present_pair_fails_closed() {
        let fixture = Fixture::new();
        let entry = fixture.root.path().join("sys/class/power_supply/BAT0");
        fs::create_dir_all(&entry).unwrap();
        fs::write(entry.join("charge_control_start_threshold"), b"70\n").unwrap();
        let error = fixture
            .writer()
            .execute(&HardwareCommand::SetBatteryThreshold(threshold()))
            .expect_err("half-present pair must fail closed");
        assert!(
            matches!(error, WriteBoundaryError::ExecutionFailed(_)),
            "unexpected error: {error:?}"
        );
        assert!(!entry.join("charge_control_end_threshold").exists());
    }

    #[test]
    fn battery_symlinked_entry_remains_usable() {
        let fixture = Fixture::new();
        let real = fixture.root.path().join("devices/BAT0");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("charge_control_start_threshold"), b"70\n").unwrap();
        fs::write(real.join("charge_control_end_threshold"), b"60\n").unwrap();
        let class = fixture.root.path().join("sys/class/power_supply");
        fs::create_dir_all(&class).unwrap();
        symlink(&real, class.join("BAT0")).unwrap();
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetBatteryThreshold(threshold()))
                .is_ok()
        );
        // The write passes through the symlink without canonicalizing it.
        assert_eq!(
            fs::read(real.join("charge_control_end_threshold")).unwrap(),
            b"80\n"
        );
    }

    #[test]
    fn missing_fan_mode_target_is_not_created() {
        let fixture = Fixture::new();
        let target = fixture.ec_path("fan_mode");
        assert_eq!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetFanMode(fan("auto"))),
            Err(WriteBoundaryError::Unavailable)
        );
        assert!(!target.exists());
    }

    #[test]
    fn missing_shift_mode_target_is_not_created() {
        let fixture = Fixture::new();
        let target = fixture.ec_path("shift_mode");
        assert_eq!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetShiftMode(shift("sport"))),
            Err(WriteBoundaryError::Unavailable)
        );
        assert!(!target.exists());
    }

    #[test]
    fn missing_boolean_target_is_not_created() {
        let fixture = Fixture::new();
        let target = fixture.ec_path("cooler_boost");
        assert_eq!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetCoolerBoost(true)),
            Err(WriteBoundaryError::Unavailable)
        );
        assert!(!target.exists());
    }

    #[test]
    fn missing_backlight_target_is_not_created() {
        let fixture = Fixture::new();
        let target = fixture
            .root
            .path()
            .join("sys/class/leds/msiacpi::kbd_backlight/brightness");
        assert_eq!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetKeyboardBacklight(2)),
            Err(WriteBoundaryError::Unavailable)
        );
        assert!(!target.exists());
    }

    #[test]
    fn open_permission_denied_maps_to_access_denied() {
        // No chmod/UID dependence: the mapper is unit-tested with a
        // synthetic permission error.
        let error = map_open_error(
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "test-denied"),
            "fan mode",
        );
        assert_eq!(error, WriteBoundaryError::AccessDenied);
        assert!(!error.may_have_mutated());
    }

    #[test]
    fn open_not_found_maps_to_unavailable() {
        let error = map_open_error(
            std::io::Error::new(std::io::ErrorKind::NotFound, "test-missing"),
            "fan mode",
        );
        assert_eq!(error, WriteBoundaryError::Unavailable);
        assert!(!error.may_have_mutated());
    }

    #[test]
    fn open_generic_failure_maps_without_path() {
        let error = map_open_error(std::io::Error::other("test-boom"), "fan mode");
        assert_eq!(
            error,
            WriteBoundaryError::ExecutionFailed("fan mode write".to_owned())
        );
        assert_eq!(error.to_string(), "hardware write failed: fan mode write");
        assert!(!error.may_have_mutated());
    }

    #[test]
    fn post_open_write_failure_reports_may_have_mutated() {
        // The open already succeeded, so truncate may have taken effect:
        // every kind maps to a may-have-mutated error without paths.
        for (kind, message) in [
            (
                std::io::ErrorKind::NotFound,
                "fan mode write target vanished",
            ),
            (
                std::io::ErrorKind::PermissionDenied,
                "fan mode write access denied",
            ),
        ] {
            let error = map_post_open_write_error(std::io::Error::new(kind, "test"), "fan mode");
            assert_eq!(error, WriteBoundaryError::WriteFailed(message.to_owned()));
            assert!(error.may_have_mutated());
        }
        let error = map_post_open_write_error(std::io::Error::other("test-boom"), "fan mode");
        assert_eq!(
            error,
            WriteBoundaryError::WriteFailed("fan mode write".to_owned())
        );
        assert!(error.may_have_mutated());
    }

    #[test]
    fn readback_denied_reports_may_have_mutated() {
        // The write was already attempted before readback, so a denied
        // readback cannot prove the hardware is untouched.
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.read_failure = Some(ReadFailure::Denied);
        let error = fixture
            .controlled(reads)
            .execute(&HardwareCommand::SetFanMode(fan("auto")))
            .expect_err("denied readback must fail closed");
        assert_eq!(
            error,
            WriteBoundaryError::WriteFailed("fan mode readback access denied".to_owned())
        );
        assert!(error.may_have_mutated());
    }

    #[test]
    fn readback_missing_reports_may_have_mutated() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.read_failure = Some(ReadFailure::Missing);
        let error = fixture
            .controlled(reads)
            .execute(&HardwareCommand::SetFanMode(fan("auto")))
            .expect_err("missing readback must fail closed");
        assert_eq!(
            error,
            WriteBoundaryError::WriteFailed("fan mode readback missing".to_owned())
        );
        assert!(error.may_have_mutated());
    }

    #[test]
    fn readback_parse_failure_reports_may_have_mutated() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.read_failure = Some(ReadFailure::Broken);
        let error = fixture
            .controlled(reads)
            .execute(&HardwareCommand::SetFanMode(fan("auto")))
            .expect_err("broken readback must fail");
        assert_eq!(
            error,
            WriteBoundaryError::WriteFailed("fan mode readback".to_owned())
        );
        assert_eq!(
            error.to_string(),
            "hardware write failed after target open: fan mode readback"
        );
        assert!(error.may_have_mutated());
    }

    #[test]
    fn fan_mode_readback_mismatch_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.string_override = Some("silent".to_owned());
        assert_eq!(
            fixture
                .controlled(reads)
                .execute(&HardwareCommand::SetFanMode(fan("auto"))),
            Err(WriteBoundaryError::VerificationFailed("fan mode"))
        );
    }

    #[test]
    fn shift_mode_readback_mismatch_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.ec_file("shift_mode", b"eco\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.string_override = Some("eco".to_owned());
        assert_eq!(
            fixture
                .controlled(reads)
                .execute(&HardwareCommand::SetShiftMode(shift("sport"))),
            Err(WriteBoundaryError::VerificationFailed("shift mode"))
        );
    }

    #[test]
    fn boolean_readback_mismatch_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.ec_file("cooler_boost", b"off\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.string_override = Some("on".to_owned());
        assert_eq!(
            fixture
                .controlled(reads)
                .execute(&HardwareCommand::SetCoolerBoost(false)),
            Err(WriteBoundaryError::VerificationFailed("cooler boost"))
        );
    }

    #[test]
    fn backlight_readback_mismatch_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.backlight_file(b"1\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.u8_override = Some(3);
        assert_eq!(
            fixture
                .controlled(reads)
                .execute(&HardwareCommand::SetKeyboardBacklight(2)),
            Err(WriteBoundaryError::VerificationFailed("keyboard backlight"))
        );
    }

    #[test]
    fn battery_start_mismatch_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.battery_entry("BAT0", b"70\n", b"60\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.start_override = Some(60);
        assert_eq!(
            fixture
                .controlled(reads)
                .execute(&HardwareCommand::SetBatteryThreshold(threshold())),
            Err(WriteBoundaryError::VerificationFailed("battery thresholds"))
        );
    }

    #[test]
    fn battery_end_mismatch_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.battery_entry("BAT0", b"70\n", b"60\n");
        let mut reads = ControlledReads::plain(LinuxSysfsReader);
        reads.end_override = Some(90);
        assert_eq!(
            fixture
                .controlled(reads)
                .execute(&HardwareCommand::SetBatteryThreshold(threshold())),
            Err(WriteBoundaryError::VerificationFailed("battery thresholds"))
        );
    }

    #[test]
    fn unrelated_sentinel_file_remains_untouched() {
        let fixture = Fixture::new();
        let sentinel = fixture.root.path().join("sentinel");
        fs::write(&sentinel, b"do-not-touch\n").unwrap();
        fixture.ec_file("cooler_boost", b"off\n");
        fixture.ec_file("fan_mode", b"silent\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetCoolerBoost(true))
                .is_ok()
        );
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetFanMode(fan("auto")))
                .is_ok()
        );
        assert_eq!(fixture.read_raw(&sentinel), b"do-not-touch\n");
    }

    #[test]
    fn no_command_can_select_an_arbitrary_path() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"auto\n");
        fixture.ec_file("shift_mode", b"comfort\n");
        fixture.ec_file("cooler_boost", b"on\n");
        fixture.ec_file("super_battery", b"off\n");
        fixture.ec_file("webcam", b"on\n");
        fixture.ec_file("webcam_block", b"off\n");
        fixture.backlight_file(b"2\n");
        fixture.battery_entry("BAT0", b"70\n", b"60\n");
        let sentinel = fixture.root.path().join("sentinel");
        fs::write(&sentinel, b"do-not-touch\n").unwrap();
        for command in [
            HardwareCommand::SetFanMode(fan("auto")),
            HardwareCommand::SetShiftMode(shift("comfort")),
            HardwareCommand::SetCoolerBoost(true),
            HardwareCommand::SetSuperBattery(false),
            HardwareCommand::SetWebcam(true),
            HardwareCommand::SetWebcamBlock(false),
            HardwareCommand::SetKeyboardBacklight(2),
            HardwareCommand::SetBatteryThreshold(threshold()),
        ] {
            assert!(fixture.writer().execute(&command).is_ok());
        }
        // Exactly the eight fixed targets plus the sentinel exist: no raw
        // EC/debug node and no caller-chosen path was touched or created.
        assert_eq!(
            fixture.collect_files(),
            vec![
                "sentinel",
                "sys/class/leds/msiacpi::kbd_backlight/brightness",
                "sys/class/power_supply/BAT0/charge_control_end_threshold",
                "sys/class/power_supply/BAT0/charge_control_start_threshold",
                "sys/devices/platform/msi-ec/cooler_boost",
                "sys/devices/platform/msi-ec/fan_mode",
                "sys/devices/platform/msi-ec/shift_mode",
                "sys/devices/platform/msi-ec/super_battery",
                "sys/devices/platform/msi-ec/webcam",
                "sys/devices/platform/msi-ec/webcam_block",
            ]
        );
        assert_eq!(fixture.read_raw(&sentinel), b"do-not-touch\n");
    }

    #[test]
    fn mode_text_with_separators_stays_file_content() {
        // Even a syntactically valid mode containing separators is payload
        // data, never a path: it is written literally into the fixed node.
        let fixture = Fixture::new();
        let target = fixture.ec_file("fan_mode", b"auto\n");
        let command = HardwareCommand::SetFanMode(fan("a/b"));
        assert!(fixture.writer().execute(&command).is_ok());
        assert_eq!(fixture.read_raw(&target), b"a/b\n");
        assert!(!fixture.root.path().join("a").exists());
    }

    #[test]
    fn production_boundary_performs_no_validation_itself() {
        // No DMI tree, no advertised modes: the transport still executes
        // the typed command untouched. Policy lives in the executor.
        let fixture = Fixture::new();
        let target = fixture.ec_file("fan_mode", b"auto\n");
        assert!(
            fixture
                .writer()
                .execute(&HardwareCommand::SetFanMode(fan("never-advertised")))
                .is_ok()
        );
        assert_eq!(fixture.read_raw(&target), b"never-advertised\n");
    }

    #[test]
    fn immediate_match_performs_one_verification_read() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let _guard = sleep_guard();
        let (reads, count) = ScriptedReads::sequenced();
        let result = fixture
            .settling(reads, counting_sleep)
            .execute(&HardwareCommand::SetFanMode(fan("auto")));
        assert!(result.is_ok());
        assert_eq!(count.get(), 1);
        assert_eq!(sleep_calls(), 0);
    }

    #[test]
    fn first_read_stale_second_read_correct_succeeds() {
        let fixture = Fixture::new();
        fixture.ec_file("shift_mode", b"eco\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads
            .strings
            .borrow_mut()
            .extend(["eco".to_owned(), "sport".to_owned()]);
        let _guard = sleep_guard();
        let result = fixture
            .settling(reads, counting_sleep)
            .execute(&HardwareCommand::SetShiftMode(shift("sport")));
        assert!(result.is_ok());
        assert_eq!(count.get(), 2);
        assert_eq!(sleep_calls(), 1);
    }

    #[test]
    fn multiple_stale_reads_then_correct_succeeds() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads.strings.borrow_mut().extend(
            ["silent", "silent", "silent", "silent", "auto"]
                .into_iter()
                .map(str::to_owned),
        );
        let _guard = sleep_guard();
        let result = fixture
            .settling(reads, counting_sleep)
            .execute(&HardwareCommand::SetFanMode(fan("auto")));
        assert!(result.is_ok());
        assert_eq!(count.get(), 5);
        assert_eq!(sleep_calls(), 4);
    }

    #[test]
    fn value_that_never_settles_is_verification_failed() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads
            .strings
            .borrow_mut()
            .extend((0..8).map(|_| "silent".to_owned()));
        let _guard = sleep_guard();
        let error = fixture
            .settling(reads, counting_sleep)
            .execute(&HardwareCommand::SetFanMode(fan("auto")))
            .expect_err("unsettled value must fail closed");
        // One immediate read plus five delayed polls, then failure.
        assert_eq!(count.get(), 6);
        assert_eq!(sleep_calls(), 5);
        assert_eq!(error, WriteBoundaryError::VerificationFailed("fan mode"));
        assert!(error.may_have_mutated());
    }

    #[test]
    fn settling_retries_do_not_repeat_the_write() {
        // The retry driver receives only a boolean poll closure: it holds
        // no target, no payload, and no write handle, so it cannot write.
        // The single `write_existing_text` per arm is the only mutation.
        let fixture = Fixture::new();
        let target = fixture.ec_file("fan_mode", b"silent\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads
            .strings
            .borrow_mut()
            .extend(["silent", "silent", "auto"].into_iter().map(str::to_owned));
        let _guard = sleep_guard();
        let result = fixture
            .settling(reads, counting_sleep)
            .execute(&HardwareCommand::SetFanMode(fan("auto")));
        assert!(result.is_ok());
        assert_eq!(count.get(), 3);
        assert_eq!(sleep_calls(), 2);
        // Exactly the single payload is on disk.
        assert_eq!(fixture.read_raw(&target), b"auto\n");
    }

    #[test]
    fn backlight_delayed_convergence_matches_physical_finding() {
        // GF63 Thin 11UC finding: after writing 2 over initial 3, the
        // first readback still shows stale 3 before settling to 2.
        let fixture = Fixture::new();
        let target = fixture.backlight_file(b"3\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads.levels.borrow_mut().extend([3, 2]);
        let _guard = sleep_guard();
        let result = fixture
            .settling(reads, counting_sleep)
            .execute(&HardwareCommand::SetKeyboardBacklight(2));
        assert!(result.is_ok());
        assert_eq!(count.get(), 2);
        assert_eq!(sleep_calls(), 1);
        assert_eq!(fixture.read_raw(&target), b"2\n");
    }

    #[test]
    fn boolean_delayed_convergence_is_accepted() {
        let fixture = Fixture::new();
        fixture.ec_file("cooler_boost", b"off\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads
            .strings
            .borrow_mut()
            .extend(["off".to_owned(), "on".to_owned()]);
        let result = fixture
            .settling(reads, no_op_sleep)
            .execute(&HardwareCommand::SetCoolerBoost(true));
        assert!(result.is_ok());
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn fan_mode_delayed_convergence_is_accepted() {
        let fixture = Fixture::new();
        fixture.ec_file("fan_mode", b"silent\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads
            .strings
            .borrow_mut()
            .extend(["silent".to_owned(), "auto".to_owned()]);
        let result = fixture
            .settling(reads, no_op_sleep)
            .execute(&HardwareCommand::SetFanMode(fan("auto")));
        assert!(result.is_ok());
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn shift_mode_delayed_convergence_is_accepted() {
        let fixture = Fixture::new();
        fixture.ec_file("shift_mode", b"eco\n");
        let (reads, count) = ScriptedReads::sequenced();
        reads
            .strings
            .borrow_mut()
            .extend(["eco".to_owned(), "sport".to_owned()]);
        let result = fixture
            .settling(reads, no_op_sleep)
            .execute(&HardwareCommand::SetShiftMode(shift("sport")));
        assert!(result.is_ok());
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn battery_pair_delayed_convergence_is_accepted() {
        let fixture = Fixture::new();
        let entry = fixture.battery_entry("BAT0", b"70\n", b"60\n");
        let (reads, count) = ScriptedReads::sequenced();
        // First poll sees the pre-write end limit; the second poll sees
        // the settled pair. No second write is ever issued.
        reads.starts.borrow_mut().extend([70]);
        reads.ends.borrow_mut().extend([60, 80]);
        let result = fixture
            .settling(reads, no_op_sleep)
            .execute(&HardwareCommand::SetBatteryThreshold(threshold()));
        assert!(result.is_ok());
        assert_eq!(count.get(), 4);
        assert_eq!(
            fixture.read_raw(&entry.join("charge_control_start_threshold")),
            b"70\n"
        );
        assert_eq!(
            fixture.read_raw(&entry.join("charge_control_end_threshold")),
            b"80\n"
        );
    }

    #[test]
    fn post_write_read_errors_are_not_retried() {
        for failure in [
            ReadFailure::Denied,
            ReadFailure::Missing,
            ReadFailure::Broken,
        ] {
            let fixture = Fixture::new();
            fixture.ec_file("fan_mode", b"silent\n");
            let _guard = sleep_guard();
            let (mut reads, count) = ScriptedReads::sequenced();
            reads.failure = Some(failure);
            let error = fixture
                .settling(reads, counting_sleep)
                .execute(&HardwareCommand::SetFanMode(fan("auto")))
                .expect_err("structural read errors must fail at once");
            assert!(
                matches!(error, WriteBoundaryError::WriteFailed(_)),
                "read errors must not become polling mismatches: {error:?}"
            );
            assert_eq!(count.get(), 1);
            assert_eq!(sleep_calls(), 0);
            assert!(error.may_have_mutated());
        }
    }
}
