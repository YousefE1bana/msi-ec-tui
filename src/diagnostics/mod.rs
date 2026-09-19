//! Diagnostics layer: human-readable hardware reports.

mod doctor;
mod evaluation;
mod report;

pub use doctor::{DoctorReport, doctor};
pub use evaluation::{
    CapabilityStatus, CompatibilityEvaluation, IdentityOutcome, InterfaceStatus,
    evaluate_compatibility,
};
pub use report::{CompatibilityReport, compatibility_report};
