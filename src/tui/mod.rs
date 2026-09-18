//! Read-only TUI runtime: terminal lifecycle, events, and input mapping.
//!
//! Task 2 establishes the Crossterm/Ratatui runtime foundation. Hardware
//! access, rendering, and binary orchestration belong to later PLAN-003
//! tasks. This module never reads sysfs directly.

mod event;
mod input;
mod runtime;
mod terminal;

pub use event::{CrosstermEventSource, EventSource, TuiEvent, event_to_tui_event};
pub use input::action_for_key;
pub use runtime::run_event_loop;
pub use terminal::TerminalSession;
