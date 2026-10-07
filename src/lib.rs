//! MEC — MSI EC Control Center.

pub mod app;
pub mod cli;
pub mod config;
pub mod diagnostics;
pub mod hardware;
pub mod monitoring;
pub mod profiles;
pub mod safety;
pub mod tui;

/// Explicit, read-only stable release checks.
pub mod updates;
