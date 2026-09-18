//! Safe execution coordinator: fresh policy before every boundary crossing.

pub mod executor;

pub use executor::{CommandExecutionError, HardwareCommandExecutor};
