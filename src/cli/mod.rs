//! CLI layer: argument types and report collectors. Frontends never touch
//! sysfs directly; controls parse intent, build a typed `HardwareCommand`,
//! and run it through the composed safe pipeline.

mod args;
pub mod monitor;
mod signal;
pub mod status;

pub use args::{BatteryCommand, BatteryLimitParseError, Cli, Command, FanCommand, OnOff};
pub use status::StatusReport;
