//! Capability descriptions, independent of discovery or hardware access.

use super::values::{FanMode, ShiftMode};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub cpu_temperature: bool,
    pub gpu_temperature: bool,

    pub cpu_fan: bool,
    pub gpu_fan: bool,

    pub fan_modes: Vec<FanMode>,
    pub shift_modes: Vec<ShiftMode>,

    pub cooler_boost: bool,
    pub super_battery: bool,

    pub webcam: bool,
    pub webcam_block: bool,

    pub fn_key: bool,
    pub win_key: bool,

    pub keyboard_backlight: Option<BacklightCapability>,
    pub battery_thresholds: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklightCapability {
    pub max_brightness: u8,
}
