//! Declarative hardware profiles: strict TOML parsing into validated data.

pub mod planner;
pub mod presets;
pub mod profile;
pub mod storage;
pub mod transaction;

pub use planner::{ProfilePlanner, ProfilePreview, ProfilePreviewEntry, ProfilePreviewStatus};
pub use presets::{BuiltinPreset, BuiltinPresetError};
pub use profile::{
    BatteryProfile, DeviceProfile, PerformanceProfile, Profile, ProfileName, ProfileNameError,
    ProfileParseError, ProfileValidationError,
};
pub use storage::{
    CustomProfileSlug, CustomProfileSlugError, MAX_PROFILE_SIZE, ProfileStorageError, ProfileStore,
};
pub use transaction::{
    ProfileTransactionPlan, ProfileTransactionPlanner, TransactionPlanError, TransactionStep,
};
