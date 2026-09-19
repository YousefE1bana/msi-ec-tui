//! Read-only TUI runtime: terminal lifecycle, events, and input mapping.
//!
//! Task 2 establishes the Crossterm/Ratatui runtime foundation. Hardware
//! access, rendering, and binary orchestration belong to later PLAN-003
//! tasks. This module never reads sysfs directly.

mod app;
mod confirmation;
mod controls;
pub mod editing;
mod event;
pub mod executor;
mod help;
mod history;
mod input;
mod notifications;
mod palette;
mod profile_catalog;
mod responsive;
mod runtime;
pub mod screens;
mod terminal;
pub mod theme;
pub mod ui;

pub use app::{
    TuiApp, prepare_tui, prepare_tui_with_profile_store, run_tui, run_tui_loop, should_launch_tui,
};
pub use event::{
    CrosstermEventSource, EventSource, TuiEvent, event_to_tui_event,
    event_to_tui_event_with_options,
};
pub use input::{action_for_key, action_for_key_with_options};
pub use profile_catalog::{CustomProfileEntry, ProfileCatalog};
pub use runtime::{run_event_loop, run_event_loop_with_ticks};
pub use screens::{
    render_battery, render_dashboard, render_devices, render_diagnostics, render_fans,
    render_performance, render_screen, render_screen_with_theme,
};
pub use terminal::TerminalSession;
pub use theme::Theme;
