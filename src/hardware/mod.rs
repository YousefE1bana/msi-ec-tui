//! Hardware domain types and the read-only backend interface.

pub mod backend;
pub mod capabilities;
pub mod capability_detector;
pub mod command;
pub mod detector;
pub mod device;
pub mod msi_ec;
pub(crate) mod msi_ec_write;
pub mod paths;
pub mod support;
pub mod sysfs;
pub mod values;
pub mod write_boundary;

pub use backend::{BackendError, EcBackend};
pub use capabilities::{BacklightCapability, Capabilities};
pub use capability_detector::{CapabilityDetector, CapabilityDiscoveryError};
pub use command::{
    BatteryThreshold, BatteryThresholdError, CommandValidationError, HardwareCommand,
};
pub use detector::{DetectionError, DeviceDetector};
pub use device::{BatteryStatus, DeviceInfo, HardwareSnapshot};
pub use msi_ec::MsiEcBackend;
pub use paths::{PathValidationError, SystemPaths};
pub use support::{ReadOnlyReason, SupportEvaluator, SupportMode};
pub use sysfs::{LinuxSysfsReader, SysfsError, SysfsReader};
pub use values::{
    FanMode, FanPercent, ModeValidationError, SensorValueError, ShiftMode, TemperatureCelsius,
};
pub use write_boundary::{HardwareWriteBoundary, WriteBoundaryError};
