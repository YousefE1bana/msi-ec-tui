//! CLI layer: argument types and report collectors. No hardware access here.

mod args;
pub mod status;

pub use args::{Cli, Command};
pub use status::StatusReport;
