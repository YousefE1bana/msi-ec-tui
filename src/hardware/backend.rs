//! Read-only hardware backend contract.

use thiserror::Error;

use super::{Capabilities, DeviceInfo, HardwareSnapshot};

/// Domain-level failures independent of a concrete hardware transport.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BackendError {
    #[error("hardware backend is unavailable")]
    Unavailable,
    #[error("hardware access denied")]
    AccessDenied,
    #[error("invalid hardware data: {0}")]
    InvalidData(String),
}

pub trait EcBackend {
    fn detect_device(&self) -> Result<DeviceInfo, BackendError>;
    fn capabilities(&self) -> Result<Capabilities, BackendError>;
    fn snapshot(&self) -> Result<HardwareSnapshot, BackendError>;
}
