//! Human-readable hardware status: `mec status` presentation.
//!
//! Read-only. Composes existing hardware abstractions; formatting lives
//! here, policy and values live in `hardware`.

use std::fmt;

use crate::hardware::{
    BackendError, BatteryStatus, DeviceInfo, EcBackend, HardwareSnapshot, MsiEcBackend,
    SupportEvaluator, SupportMode, SysfsReader, SystemPaths,
};

/// Owned, deterministically renderable hardware status.
#[derive(Debug, Clone)]
pub struct StatusReport {
    pub device: DeviceInfo,
    pub mode: SupportMode,
    pub snapshot: HardwareSnapshot,
}

/// Collects device identity, support mode, and snapshot through the
/// existing backend abstractions. Read-only verdicts do not block
/// monitoring: a coherent snapshot stays usable when read-only.
pub fn status<R>(paths: SystemPaths, reader: R) -> Result<StatusReport, BackendError>
where
    R: SysfsReader + Clone,
{
    let mode = SupportEvaluator::new(paths.clone(), reader.clone()).evaluate();
    let backend = MsiEcBackend::new(paths, reader);
    let device = backend.detect_device()?;
    let snapshot = backend.snapshot()?;
    Ok(StatusReport {
        device,
        mode,
        snapshot,
    })
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
