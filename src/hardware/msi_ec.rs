//! Strictly read-only `msi-ec` backend: identity, capabilities, snapshot.
//!
//! No write API exists here. Snapshot collection reads only the interfaces
//! advertised by capability discovery and never requires write eligibility.

use std::path::{Path, PathBuf};

use super::{
    BackendError, Capabilities, CapabilityDetector, CapabilityDiscoveryError, DetectionError,
    DeviceDetector, DeviceInfo, EcBackend, FanMode, FanPercent, HardwareSnapshot,
    ModeValidationError, ShiftMode, SysfsError, SysfsReader, SystemPaths, TemperatureCelsius,
};

/// Production-shaped read-only backend over rooted sysfs-like trees.
pub struct MsiEcBackend<R> {
    paths: SystemPaths,
    reader: R,
}

impl<R> MsiEcBackend<R>
where
    R: SysfsReader + Clone,
{
    /// Creates a backend over `paths` using the supplied reader.
    pub fn new(paths: SystemPaths, reader: R) -> Self {
        Self { paths, reader }
    }

    fn ec_path(&self, field: &'static str, relative: &str) -> Result<PathBuf, BackendError> {
        self.paths
            .msi_ec(relative)
            .map_err(|source| BackendError::InvalidData(format!("invalid {field} path: {source}")))
    }

    fn read_temperature(
        &self,
        field: &'static str,
        relative: &str,
    ) -> Result<TemperatureCelsius, BackendError> {
        let path = self.ec_path(field, relative)?;
        let raw = self
            .reader
            .read_u8(&path)
            .map_err(|error| map_sysfs(field, error))?;
        TemperatureCelsius::try_from(raw)
            .map_err(|error| BackendError::InvalidData(format!("invalid {field}: {error}")))
    }

    /// Raw percentage-style fan value exposed by `msi-ec` (`0..=150`).
    fn read_fan_percent(
        &self,
        field: &'static str,
        relative: &str,
    ) -> Result<FanPercent, BackendError> {
        let path = self.ec_path(field, relative)?;
        let raw = self
            .reader
            .read_u16(&path)
            .map_err(|error| map_sysfs(field, error))?;
        FanPercent::try_from(raw)
            .map_err(|error| BackendError::InvalidData(format!("invalid {field}: {error}")))
    }

    fn read_mode<M>(&self, field: &'static str, relative: &str) -> Result<M, BackendError>
    where
        M: for<'a> TryFrom<&'a str, Error = ModeValidationError>,
    {
        let path = self.ec_path(field, relative)?;
        let raw = self
            .reader
            .read_string(&path)
            .map_err(|error| map_sysfs(field, error))?;
        M::try_from(raw.as_str())
            .map_err(|error| BackendError::InvalidData(format!("invalid {field}: {error}")))
    }

    fn read_bool(&self, field: &'static str, relative: &str) -> Result<bool, BackendError> {
        let path = self.ec_path(field, relative)?;
        let raw = self
            .reader
            .read_string(&path)
            .map_err(|error| map_sysfs(field, error))?;
        parse_bool(field, &raw, &path)
    }

    fn read_backlight(&self, max_brightness: u8) -> Result<u8, BackendError> {
        const FIELD: &str = "keyboard_backlight";
        let path = self
            .paths
            .leds_root()
            .join("msiacpi::kbd_backlight")
            .join("brightness");
        let value = self
            .reader
            .read_u8(&path)
            .map_err(|error| map_sysfs(FIELD, error))?;
        if value > max_brightness {
            return Err(BackendError::InvalidData(format!(
                "{FIELD} brightness {value} exceeds discovered maximum {max_brightness}"
            )));
        }
        Ok(value)
    }

    fn read_battery_thresholds(&self) -> Result<(Option<u8>, Option<u8>), BackendError> {
        const FIELD: &str = "battery_thresholds";
        let entries = self
            .reader
            .list_entries(&self.paths.power_supply_root())
            .map_err(|error| map_sysfs(FIELD, error))?;
        // `list_entries` already enumerates in deterministic sorted order.
        // Existence probes are typed: a probe failure must surface instead
        // of hiding the entry, and a half-present pair mirrors discovery by
        // failing rather than pretending the entry is threshold-free.
        let mut selected: Option<&PathBuf> = None;
        for entry in &entries {
            let start_present = self
                .reader
                .exists(&entry.join("charge_control_start_threshold"))
                .map_err(|error| map_sysfs(FIELD, error))?;
            let end_present = self
                .reader
                .exists(&entry.join("charge_control_end_threshold"))
                .map_err(|error| map_sysfs(FIELD, error))?;
            match (start_present, end_present) {
                (true, true) => {
                    selected = Some(entry);
                    break;
                }
                (false, false) => {}
                _ => {
                    return Err(BackendError::InvalidData(format!(
                        "{FIELD}: threshold pair is half-present"
                    )));
                }
            }
        }
        let entry = selected.ok_or(BackendError::Unavailable)?;
        let start = self.read_threshold(entry, "charge_control_start_threshold")?;
        let end = self.read_threshold(entry, "charge_control_end_threshold")?;
        if start > 100 || end > 100 {
            return Err(BackendError::InvalidData(format!(
                "{FIELD} out of range: start {start}, end {end}"
            )));
        }
        if start > end {
            return Err(BackendError::InvalidData(format!(
                "{FIELD} inverted: start {start} exceeds end {end}"
            )));
        }
        Ok((Some(start), Some(end)))
    }

    fn read_threshold(&self, entry: &Path, name: &str) -> Result<u8, BackendError> {
        let path = entry.join(name);
        self.reader
            .read_u8(&path)
            .map_err(|error| map_sysfs("battery_thresholds", error))
    }
}

impl<R> EcBackend for MsiEcBackend<R>
where
    R: SysfsReader + Clone,
{
    fn detect_device(&self) -> Result<DeviceInfo, BackendError> {
        DeviceDetector::new(self.paths.clone(), self.reader.clone())
            .detect()
            .map_err(map_detection)
    }

    fn capabilities(&self) -> Result<Capabilities, BackendError> {
        CapabilityDetector::new(self.paths.clone(), self.reader.clone())
            .discover()
            .map_err(map_discovery)
    }

    fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
        let capabilities = self.capabilities()?;

        let cpu_temperature = capabilities
            .cpu_temperature
            .then(|| self.read_temperature("cpu_temperature", "cpu/realtime_temperature"))
            .transpose()?;
        let gpu_temperature = capabilities
            .gpu_temperature
            .then(|| self.read_temperature("gpu_temperature", "gpu/realtime_temperature"))
            .transpose()?;
        let cpu_fan = capabilities
            .cpu_fan
            .then(|| self.read_fan_percent("cpu_fan", "cpu/realtime_fan_speed"))
            .transpose()?;
        let gpu_fan = capabilities
            .gpu_fan
            .then(|| self.read_fan_percent("gpu_fan", "gpu/realtime_fan_speed"))
            .transpose()?;

        // A non-empty advertised list means current-mode control exists; the
        // reported value itself only needs to be syntactically valid, not a
        // member of the list, so future driver states remain representable.
        let fan_mode: Option<FanMode> = if capabilities.fan_modes.is_empty() {
            None
        } else {
            Some(self.read_mode("fan_mode", "fan_mode")?)
        };
        let shift_mode: Option<ShiftMode> = if capabilities.shift_modes.is_empty() {
            None
        } else {
            Some(self.read_mode("shift_mode", "shift_mode")?)
        };

        let cooler_boost = capabilities
            .cooler_boost
            .then(|| self.read_bool("cooler_boost", "cooler_boost"))
            .transpose()?;
        let super_battery = capabilities
            .super_battery
            .then(|| self.read_bool("super_battery", "super_battery"))
            .transpose()?;
        let webcam = capabilities
            .webcam
            .then(|| self.read_bool("webcam", "webcam"))
            .transpose()?;
        let webcam_block = capabilities
            .webcam_block
            .then(|| self.read_bool("webcam_block", "webcam_block"))
            .transpose()?;

        let keyboard_backlight = capabilities
            .keyboard_backlight
            .as_ref()
            .map(|backlight| self.read_backlight(backlight.max_brightness))
            .transpose()?;

        let (battery_start_threshold, battery_end_threshold) = if capabilities.battery_thresholds {
            self.read_battery_thresholds()?
        } else {
            (None, None)
        };

        Ok(HardwareSnapshot {
            cpu_temperature,
            gpu_temperature,
            cpu_fan,
            gpu_fan,
            fan_mode,
            shift_mode,
            cooler_boost,
            super_battery,
            webcam,
            webcam_block,
            keyboard_backlight,
            battery_start_threshold,
            battery_end_threshold,
        })
    }
}

/// Accepts only the exact upstream boolean vocabulary, case-sensitively.
fn parse_bool(field: &'static str, value: &str, path: &Path) -> Result<bool, BackendError> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(BackendError::InvalidData(format!(
            "invalid {field} at {}: expected \"on\" or \"off\"",
            path.display()
        ))),
    }
}

/// Maps filesystem failures conservatively: denied access stays denied, a
/// node that vanished after discovery is unavailable, and malformed content
/// is invalid data. Never panics.
fn map_sysfs(context: &str, error: SysfsError) -> BackendError {
    match error {
        SysfsError::PermissionDenied(_) => BackendError::AccessDenied,
        SysfsError::NotFound(_) => BackendError::Unavailable,
        SysfsError::InvalidValue { path, .. } => BackendError::InvalidData(format!(
            "{context}: invalid numeric value at {}",
            path.display()
        )),
        error => BackendError::InvalidData(format!("{context}: {error}")),
    }
}

fn map_detection(error: DetectionError) -> BackendError {
    match error {
        DetectionError::UnsupportedVendor { detected } => {
            BackendError::InvalidData(format!("unsupported vendor: {detected}"))
        }
        DetectionError::MissingRequiredField { .. } => BackendError::Unavailable,
        DetectionError::InvalidField { field } => {
            BackendError::InvalidData(format!("invalid DMI field: {field}"))
        }
        DetectionError::ReadFailure { field, source } => match source {
            SysfsError::PermissionDenied(_) => BackendError::AccessDenied,
            source => {
                BackendError::InvalidData(format!("failed to read DMI field {field}: {source}"))
            }
        },
        DetectionError::PathFailure { field, source } => {
            BackendError::InvalidData(format!("invalid DMI path for {field}: {source}"))
        }
    }
}

fn map_discovery(error: CapabilityDiscoveryError) -> BackendError {
    match error {
        CapabilityDiscoveryError::Read { capability, source } => map_sysfs(capability, source),
        CapabilityDiscoveryError::Path { capability, source } => {
            BackendError::InvalidData(format!("invalid {capability} path: {source}"))
        }
        CapabilityDiscoveryError::InvalidModes { capability, .. } => {
            BackendError::InvalidData(format!("invalid {capability} mode list"))
        }
        CapabilityDiscoveryError::InconsistentInterface { capability, detail } => {
            BackendError::InvalidData(format!("inconsistent {capability} interface: {detail}"))
        }
        CapabilityDiscoveryError::InvalidBacklight { detail } => {
            BackendError::InvalidData(format!("invalid keyboard backlight: {detail}"))
        }
    }
}
