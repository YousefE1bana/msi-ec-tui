//! Dynamic discovery of `msi-ec` capabilities from exposed sysfs interfaces.
//!
//! Discovery is presence-driven: a capability exists only when the kernel
//! interface exposes it. It never infers support from laptop model names,
//! and it performs reads only — no values are interpreted here.

use std::collections::HashSet;

use thiserror::Error;

use super::{
    BacklightCapability, Capabilities, ModeValidationError, PathValidationError, SysfsError,
    SysfsReader, SystemPaths,
};

#[derive(Debug, Error)]
pub enum CapabilityDiscoveryError {
    #[error("failed to resolve path for {capability}")]
    Path {
        capability: &'static str,
        source: PathValidationError,
    },
    #[error("failed to read {capability}")]
    Read {
        capability: &'static str,
        source: SysfsError,
    },
    #[error("invalid {capability} mode list: {value:?}")]
    InvalidModes {
        capability: &'static str,
        value: String,
    },
    #[error("inconsistent {capability} interface: {detail}")]
    InconsistentInterface {
        capability: &'static str,
        detail: &'static str,
    },
    #[error("invalid keyboard backlight: {detail}")]
    InvalidBacklight { detail: &'static str },
}

/// Discovers capabilities from the interfaces actually exposed by the system.
pub struct CapabilityDetector<R> {
    paths: SystemPaths,
    reader: R,
}

impl<R> CapabilityDetector<R>
where
    R: SysfsReader,
{
    /// Creates a detector over `paths` using the supplied reader.
    pub fn new(paths: SystemPaths, reader: R) -> Self {
        Self { paths, reader }
    }

    /// Discovers capabilities. A missing `msi-ec` root yields absent
    /// EC-specific capabilities without an error; class discovery proceeds.
    pub fn discover(&self) -> Result<Capabilities, CapabilityDiscoveryError> {
        Ok(Capabilities {
            cpu_temperature: self.present("cpu_temperature", "cpu/realtime_temperature")?,
            gpu_temperature: self.present("gpu_temperature", "gpu/realtime_temperature")?,

            cpu_fan: self.present("cpu_fan", "cpu/realtime_fan_speed")?,
            gpu_fan: self.present("gpu_fan", "gpu/realtime_fan_speed")?,

            fan_modes: self.discover_modes("fan_mode", "fan_mode", "available_fan_modes")?,
            shift_modes: self.discover_modes(
                "shift_mode",
                "shift_mode",
                "available_shift_modes",
            )?,

            cooler_boost: self.present("cooler_boost", "cooler_boost")?,
            super_battery: self.present("super_battery", "super_battery")?,

            webcam: self.present("webcam", "webcam")?,
            webcam_block: self.present("webcam_block", "webcam_block")?,

            fn_key: self.present("fn_key", "fn_key")?,
            win_key: self.present("win_key", "win_key")?,

            keyboard_backlight: self.discover_backlight()?,
            battery_thresholds: self.discover_battery_thresholds()?,
        })
    }

    /// Presence-only probe of one `msi-ec` interface file. Existence is
    /// established successfully before answering; a probe failure is a
    /// read error, never a silent absence.
    fn present(
        &self,
        capability: &'static str,
        relative: &str,
    ) -> Result<bool, CapabilityDiscoveryError> {
        let path = self
            .paths
            .msi_ec(relative)
            .map_err(|source| CapabilityDiscoveryError::Path { capability, source })?;
        self.reader
            .exists(&path)
            .map_err(|source| CapabilityDiscoveryError::Read { capability, source })
    }

    /// Discovers one paired current/available mode interface.
    fn discover_modes<M>(
        &self,
        capability: &'static str,
        current: &str,
        available: &str,
    ) -> Result<Vec<M>, CapabilityDiscoveryError>
    where
        M: for<'a> TryFrom<&'a str, Error = ModeValidationError>,
    {
        let current_path = self
            .paths
            .msi_ec(current)
            .map_err(|source| CapabilityDiscoveryError::Path { capability, source })?;
        let available_path = self
            .paths
            .msi_ec(available)
            .map_err(|source| CapabilityDiscoveryError::Path { capability, source })?;
        let current = self
            .reader
            .exists(&current_path)
            .map_err(|source| CapabilityDiscoveryError::Read { capability, source })?;
        let available = self
            .reader
            .exists(&available_path)
            .map_err(|source| CapabilityDiscoveryError::Read { capability, source })?;
        match (current, available) {
            (false, false) => Ok(Vec::new()),
            (true, true) => {
                let raw = self
                    .reader
                    .read_string(&available_path)
                    .map_err(|source| CapabilityDiscoveryError::Read { capability, source })?;
                parse_modes(&raw, capability)
            }
            _ => Err(CapabilityDiscoveryError::InconsistentInterface {
                capability,
                detail: "current and available mode files must both exist or both be absent",
            }),
        }
    }

    /// Discovers battery charge thresholds via symlink-tolerant class entries.
    fn discover_battery_thresholds(&self) -> Result<bool, CapabilityDiscoveryError> {
        const CAPABILITY: &str = "battery_thresholds";
        let entries = match self.reader.list_entries(&self.paths.power_supply_root()) {
            Ok(entries) => entries,
            Err(SysfsError::NotFound(_)) => return Ok(false),
            Err(source) => {
                return Err(CapabilityDiscoveryError::Read {
                    capability: CAPABILITY,
                    source,
                });
            }
        };
        let mut supported = false;
        for entry in entries {
            let start = entry.join("charge_control_start_threshold");
            let end = entry.join("charge_control_end_threshold");
            let start_present =
                self.reader
                    .exists(&start)
                    .map_err(|source| CapabilityDiscoveryError::Read {
                        capability: CAPABILITY,
                        source,
                    })?;
            let end_present =
                self.reader
                    .exists(&end)
                    .map_err(|source| CapabilityDiscoveryError::Read {
                        capability: CAPABILITY,
                        source,
                    })?;
            match (start_present, end_present) {
                (true, true) => supported = true,
                (false, false) => {}
                _ => {
                    return Err(CapabilityDiscoveryError::InconsistentInterface {
                        capability: CAPABILITY,
                        detail: "charge threshold files must both exist or both be absent",
                    });
                }
            }
        }
        Ok(supported)
    }

    /// Discovers the exact upstream keyboard-backlight class entry, if any.
    fn discover_backlight(&self) -> Result<Option<BacklightCapability>, CapabilityDiscoveryError> {
        const CAPABILITY: &str = "keyboard_backlight";
        let entries = match self.reader.list_entries(&self.paths.leds_root()) {
            Ok(entries) => entries,
            Err(SysfsError::NotFound(_)) => return Ok(None),
            Err(source) => {
                return Err(CapabilityDiscoveryError::Read {
                    capability: CAPABILITY,
                    source,
                });
            }
        };
        let Some(entry) = entries.iter().find(|path| {
            path.file_name()
                .is_some_and(|name| name == "msiacpi::kbd_backlight")
        }) else {
            return Ok(None);
        };
        let brightness = entry.join("brightness");
        let max_brightness = entry.join("max_brightness");
        let brightness_present =
            self.reader
                .exists(&brightness)
                .map_err(|source| CapabilityDiscoveryError::Read {
                    capability: CAPABILITY,
                    source,
                })?;
        if !brightness_present {
            return Err(CapabilityDiscoveryError::InvalidBacklight {
                detail: "brightness is missing",
            });
        }
        let max_present = self.reader.exists(&max_brightness).map_err(|source| {
            CapabilityDiscoveryError::Read {
                capability: CAPABILITY,
                source,
            }
        })?;
        if !max_present {
            return Err(CapabilityDiscoveryError::InvalidBacklight {
                detail: "max_brightness is missing",
            });
        }
        let max = self.reader.read_u8(&max_brightness).map_err(|source| {
            CapabilityDiscoveryError::Read {
                capability: CAPABILITY,
                source,
            }
        })?;
        if max == 0 {
            return Err(CapabilityDiscoveryError::InvalidBacklight {
                detail: "max_brightness must be at least 1",
            });
        }
        Ok(Some(BacklightCapability {
            max_brightness: max,
        }))
    }
}

/// Parses one newline-separated available-mode list through the validated
/// mode newtypes, preserving upstream order. `read_string` already strips
/// trailing whitespace, so a final newline is accepted while any remaining
/// empty line is an internal blank and therefore invalid.
fn parse_modes<M>(raw: &str, capability: &'static str) -> Result<Vec<M>, CapabilityDiscoveryError>
where
    M: for<'a> TryFrom<&'a str, Error = ModeValidationError>,
{
    if raw.is_empty() {
        return Err(CapabilityDiscoveryError::InvalidModes {
            capability,
            value: raw.to_owned(),
        });
    }
    let mut seen = HashSet::new();
    let mut modes = Vec::new();
    for line in raw.split('\n') {
        if line.is_empty() || !seen.insert(line) {
            return Err(CapabilityDiscoveryError::InvalidModes {
                capability,
                value: raw.to_owned(),
            });
        }
        let mode = M::try_from(line).map_err(|_| CapabilityDiscoveryError::InvalidModes {
            capability,
            value: raw.to_owned(),
        })?;
        modes.push(mode);
    }
    Ok(modes)
}
