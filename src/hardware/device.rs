//! Device identity and optional hardware readings.

use super::values::{FanMode, FanPercent, ShiftMode, TemperatureCelsius};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub product_name: String,
    pub board_name: Option<String>,
    pub bios_version: Option<String>,
    pub ec_firmware_version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HardwareSnapshot {
    pub cpu_temperature: Option<TemperatureCelsius>,
    pub gpu_temperature: Option<TemperatureCelsius>,

    pub cpu_fan: Option<FanPercent>,
    pub gpu_fan: Option<FanPercent>,

    pub fan_mode: Option<FanMode>,
    pub shift_mode: Option<ShiftMode>,

    pub cooler_boost: Option<bool>,
    pub super_battery: Option<bool>,
    pub webcam: Option<bool>,
    pub webcam_block: Option<bool>,

    pub keyboard_backlight: Option<u8>,

    pub battery_start_threshold: Option<u8>,
    pub battery_end_threshold: Option<u8>,
}
