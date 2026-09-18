//! Safe execution coordinator: fresh policy before every boundary crossing.

pub mod control;
pub mod executor;
pub mod profile_transaction;

pub use control::execute_hardware_command;
pub use executor::{CommandExecutionError, HardwareCommandExecutor};
pub use profile_transaction::{
    ProfileApplyError, ProfileApplyFailure, ProfileApplyReport, ProfileTransactionExecutor,
    RollbackAttempt,
};
