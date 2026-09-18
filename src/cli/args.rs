//! Command-line argument model for the `mec` executable.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// MEC command-line interface.
#[derive(Debug, Parser)]
#[command(name = "mec", about = "MSI EC Control Center for Linux")]
pub struct Cli {
    /// Alternate system root for hardware inspection (tests and fixtures).
    #[arg(long, global = true, value_name = "PATH", default_value = "/")]
    pub sys_root: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Available `mec` subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect hardware compatibility and report diagnostics.
    Doctor,
}
