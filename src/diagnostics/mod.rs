//! Diagnostics layer: human-readable hardware reports.

mod doctor;
mod report;

pub use doctor::{DoctorReport, doctor};
pub use report::{
    CompatibilityReport, CompatibilitySnapshot, compatibility_report, evaluate_compatibility,
};
