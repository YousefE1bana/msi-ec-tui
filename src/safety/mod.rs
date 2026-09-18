//! Safe execution coordinator: fresh policy before every boundary crossing.

pub mod control;
pub mod executor;

pub use control::execute_hardware_command;
pub use executor::{CommandExecutionError, HardwareCommandExecutor};
