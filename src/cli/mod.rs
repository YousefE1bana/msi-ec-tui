//! CLI layer: argument types and report collectors. No hardware access here.

mod args;
pub mod monitor;
mod signal;
pub mod status;

pub use args::{Cli, Command};
pub use status::StatusReport;
