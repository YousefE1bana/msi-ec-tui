//! Human-readable hardware status: `mec status` presentation.
//!
//! Read-only. Composes existing hardware abstractions; formatting lives
//! here, policy and values live in `hardware`.

use std::fmt;

use serde::Serialize;

use crate::hardware::{
    BackendError, BatteryStatus, DeviceInfo, EcBackend, HardwareSnapshot, MsiEcBackend,
    SupportEvaluator, SupportMode, SysfsReader, SystemPaths,
};

/// Owned, deterministically renderable hardware status.
///
/// A report may carry a degraded presentation snapshot: when strict
/// snapshot collection fails, the snapshot is an all-`None` default and
/// the original error is retained for out-of-band warning output.
/// Device identity failure stays fatal and never produces a report.
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub device: DeviceInfo,
    pub mode: SupportMode,
    pub snapshot: HardwareSnapshot,
    snapshot_error: Option<BackendError>,
}

impl StatusReport {
    /// The snapshot collection error behind a degraded report, if any.
    pub fn snapshot_error(&self) -> Option<&BackendError> {
        self.snapshot_error.as_ref()
    }
}

/// Collects device identity, support mode, and snapshot through the
/// existing backend abstractions. Read-only verdicts do not block
/// monitoring: a coherent snapshot stays usable when read-only.
///
/// Identity failure is fatal. Snapshot failure degrades to an all-`None`
/// presentation snapshot with the original error retained; values are
/// never fabricated.
pub fn status<R>(paths: SystemPaths, reader: R) -> Result<StatusReport, BackendError>
where
    R: SysfsReader + Clone,
{
    let mode = SupportEvaluator::new(paths.clone(), reader.clone()).evaluate();
    let backend = MsiEcBackend::new(paths, reader);
    let device = backend.detect_device()?;
    match backend.snapshot() {
        Ok(snapshot) => Ok(StatusReport {
            device,
            mode,
            snapshot,
            snapshot_error: None,
        }),
        Err(error) => Ok(StatusReport {
            device,
            mode,
            snapshot: HardwareSnapshot::default(),
            snapshot_error: Some(error),
        }),
    }
}

fn on_off(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "On",
        Some(false) => "Off",
        None => "N/A",
    }
}

fn battery_state(status: Option<&BatteryStatus>) -> &'static str {
    match status {
        Some(BatteryStatus::Unknown) => "Unknown",
        Some(BatteryStatus::Charging) => "Charging",
        Some(BatteryStatus::Discharging) => "Discharging",
        Some(BatteryStatus::NotCharging) => "Not charging",
        Some(BatteryStatus::Full) => "Full",
        None => "N/A",
    }
}

/// Explicit machine-output DTOs. Hardware-domain structs are deliberately
/// not `Serialize`; the CLI schema evolves independently of the domain.
#[derive(Debug, Clone, Serialize)]
struct DeviceDto {
    manufacturer: String,
    product_name: String,
    board_name: Option<String>,
    bios_version: Option<String>,
    ec_firmware_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ThermalsDto {
    cpu_celsius: Option<u8>,
    gpu_celsius: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
struct FansDto {
    cpu_percent: Option<u16>,
    gpu_percent: Option<u16>,
    mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PerformanceDto {
    shift_mode: Option<String>,
    cooler_boost: Option<bool>,
    super_battery: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct BatteryDto {
    percentage: Option<u8>,
    status: Option<String>,
    ac_connected: Option<bool>,
    start_threshold_percent: Option<u8>,
    end_threshold_percent: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
struct DevicesDto {
    webcam: Option<bool>,
    webcam_block: Option<bool>,
    keyboard_backlight: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
struct StatusDto {
    device: DeviceDto,
    mode: &'static str,
    thermals: ThermalsDto,
    fans: FansDto,
    performance: PerformanceDto,
    battery: BatteryDto,
    devices: DevicesDto,
}

fn battery_status_text(status: Option<&BatteryStatus>) -> Option<String> {
    status.map(|value| battery_state(Some(value)).to_owned())
}

impl StatusReport {
    fn to_dto(&self) -> StatusDto {
        let snapshot = &self.snapshot;
        StatusDto {
            device: DeviceDto {
                manufacturer: self.device.manufacturer.clone(),
                product_name: self.device.product_name.clone(),
                board_name: self.device.board_name.clone(),
                bios_version: self.device.bios_version.clone(),
                ec_firmware_version: self.device.ec_firmware_version.clone(),
            },
            mode: match self.mode {
                SupportMode::Ready => "READY",
                SupportMode::ReadOnly(_) => "READ-ONLY",
            },
            thermals: ThermalsDto {
                cpu_celsius: snapshot.cpu_temperature.map(|value| value.get()),
                gpu_celsius: snapshot.gpu_temperature.map(|value| value.get()),
            },
            fans: FansDto {
                cpu_percent: snapshot.cpu_fan.map(|value| value.get()),
                gpu_percent: snapshot.gpu_fan.map(|value| value.get()),
                mode: snapshot
                    .fan_mode
                    .as_ref()
                    .map(|mode| mode.as_str().to_owned()),
            },
            performance: PerformanceDto {
                shift_mode: snapshot
                    .shift_mode
                    .as_ref()
                    .map(|mode| mode.as_str().to_owned()),
                cooler_boost: snapshot.cooler_boost,
                super_battery: snapshot.super_battery,
            },
            battery: BatteryDto {
                percentage: snapshot.battery_percentage,
                status: battery_status_text(snapshot.battery_status.as_ref()),
                ac_connected: snapshot.ac_connected,
                start_threshold_percent: snapshot.battery_start_threshold,
                end_threshold_percent: snapshot.battery_end_threshold,
            },
            devices: DevicesDto {
                webcam: snapshot.webcam,
                webcam_block: snapshot.webcam_block,
                keyboard_backlight: snapshot.keyboard_backlight,
            },
        }
    }

    /// Serializes the report as one compact JSON document (no trailing
    /// newline; the CLI adds exactly one when printing).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.to_dto())
    }
}

impl fmt::Display for StatusReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let snapshot = &self.snapshot;
        let mode = match self.mode {
            SupportMode::Ready => "READY",
            SupportMode::ReadOnly(_) => "READ-ONLY",
        };
        let cpu_temperature = snapshot
            .cpu_temperature
            .map(|value| format!("{}°C", value.get()))
            .unwrap_or_else(|| "N/A".to_owned());
        let gpu_temperature = snapshot
            .gpu_temperature
            .map(|value| format!("{}°C", value.get()))
            .unwrap_or_else(|| "N/A".to_owned());
        let cpu_fan = snapshot
            .cpu_fan
            .map(|value| format!("{}%", value.get()))
            .unwrap_or_else(|| "N/A".to_owned());
        let gpu_fan = snapshot
            .gpu_fan
            .map(|value| format!("{}%", value.get()))
            .unwrap_or_else(|| "N/A".to_owned());
        let fan_mode = snapshot
            .fan_mode
            .as_ref()
            .map(|mode| mode.as_str())
            .unwrap_or("N/A");
        let shift_mode = snapshot
            .shift_mode
            .as_ref()
            .map(|mode| mode.as_str())
            .unwrap_or("N/A");
        let backlight = snapshot
            .keyboard_backlight
            .map(|level| level.to_string())
            .unwrap_or_else(|| "N/A".to_owned());
        let charge = snapshot
            .battery_percentage
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "N/A".to_owned());
        let ac = match snapshot.ac_connected {
            Some(true) => "Connected",
            Some(false) => "Disconnected",
            None => "N/A",
        };
        let start_threshold = snapshot
            .battery_start_threshold
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "N/A".to_owned());
        let end_threshold = snapshot
            .battery_end_threshold
            .map(|value| format!("{value}%"))
            .unwrap_or_else(|| "N/A".to_owned());

        writeln!(f, "MEC Status")?;
        writeln!(f)?;
        writeln!(f, "Device: {}", self.device.product_name)?;
        writeln!(f, "Mode: {mode}")?;
        writeln!(f)?;
        writeln!(f, "Thermals")?;
        writeln!(f, "CPU Temperature: {cpu_temperature}")?;
        writeln!(f, "GPU Temperature: {gpu_temperature}")?;
        writeln!(f)?;
        writeln!(f, "Fans")?;
        writeln!(f, "CPU Fan: {cpu_fan}")?;
        writeln!(f, "GPU Fan: {gpu_fan}")?;
        writeln!(f, "Fan Mode: {fan_mode}")?;
        writeln!(f)?;
        writeln!(f, "Performance")?;
        writeln!(f, "Shift Mode: {shift_mode}")?;
        writeln!(f, "Cooler Boost: {}", on_off(snapshot.cooler_boost))?;
        writeln!(f, "Super Battery: {}", on_off(snapshot.super_battery))?;
        writeln!(f)?;
        writeln!(f, "Battery")?;
        writeln!(f, "Charge: {charge}")?;
        writeln!(
            f,
            "State: {}",
            battery_state(snapshot.battery_status.as_ref())
        )?;
        writeln!(f, "AC: {ac}")?;
        writeln!(f, "Start Threshold: {start_threshold}")?;
        writeln!(f, "End Threshold: {end_threshold}")?;
        writeln!(f)?;
        writeln!(f, "Devices")?;
        writeln!(f, "Webcam: {}", on_off(snapshot.webcam))?;
        writeln!(f, "Webcam Block: {}", on_off(snapshot.webcam_block))?;
        writeln!(f, "Keyboard Backlight: {backlight}")
    }
}
