//! Terminal-independent application state foundation.
//!
//! Navigation and help visibility only. No hardware data, no filesystem
//! paths, no terminal types.

mod action;
mod interaction;
mod live;
mod state;

pub use action::AppAction;
pub use interaction::ProfileSelection;
pub use live::LiveHardware;
pub use state::{AppState, Screen};
