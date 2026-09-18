//! Declarative hardware profiles: strict TOML parsing into validated data.

pub mod profile;

pub use profile::{
    BatteryProfile, DeviceProfile, PerformanceProfile, Profile, ProfileName, ProfileNameError,
    ProfileParseError, ProfileValidationError,
};
