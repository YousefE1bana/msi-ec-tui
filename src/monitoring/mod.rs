//! Reusable monitoring primitives: validated poll intervals and bounded
//! in-memory snapshot history. No I/O, no timers, no CLI coupling.

mod engine;
mod history;
mod interval;

pub use engine::MonitorEngine;
pub use history::{DEFAULT_HISTORY_CAPACITY, SnapshotHistory};
pub use interval::{PollInterval, PollIntervalError};
