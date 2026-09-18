//! First diagnostic surface: `mec doctor` compatibility reporting.
//!
//! Read-only. Composes existing hardware abstractions; the final verdict
//! always comes from [`SupportEvaluator`]. Formatting lives here, policy
//! lives in `hardware`.

use std::fmt;

use crate::hardware::{
    CapabilityDetector, CapabilityDiscoveryError, DetectionError, DeviceDetector, LinuxSysfsReader,
    ReadOnlyReason, SupportEvaluator, SupportMode, SysfsError, SysfsReader, SystemPaths,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
struct CheckLine {
    verdict: Verdict,
    message: String,
}

/// Owned, deterministically renderable result of a doctor evaluation.
#[derive(Debug, Clone)]
pub struct DoctorReport {
    mode: SupportMode,
    lines: Vec<CheckLine>,
}

impl DoctorReport {
    fn push(&mut self, verdict: Verdict, message: impl Into<String>) {
        self.lines.push(CheckLine {
            verdict,
            message: message.into(),
        });
    }
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "MEC Doctor")?;
        writeln!(f)?;
        for line in &self.lines {
            let tag = match line.verdict {
                Verdict::Pass => "PASS",
                Verdict::Warn => "WARN",
                Verdict::Fail => "FAIL",
            };
            writeln!(f, "[{tag}] {}", line.message)?;
        }
        writeln!(f)?;
        match &self.mode {
            SupportMode::Ready => writeln!(f, "Mode: READY"),
            SupportMode::ReadOnly(reason) => {
                writeln!(f, "Mode: READ-ONLY")?;
                writeln!(f, "Reason: {}", reason_text(reason))
            }
        }
    }
}

fn reason_text(reason: &ReadOnlyReason) -> &'static str {
    match reason {
        ReadOnlyReason::NonMsiHardware => "Non-MSI hardware",
        ReadOnlyReason::UnverifiedHardwareIdentity => "Hardware identity could not be verified",
        ReadOnlyReason::MsiEcUnavailable => "msi-ec interface unavailable",
        ReadOnlyReason::MsiEcUnreadable => "msi-ec interface unreadable",
        ReadOnlyReason::InconsistentInterface => "Inconsistent hardware interface",
    }
}

fn push_feature(report: &mut DoctorReport, present: bool, name: &str) {
    if present {
        report.push(Verdict::Pass, format!("{name} available"));
    } else {
        report.push(Verdict::Warn, format!("{name} unavailable"));
    }
}

/// Evaluates hardware compatibility and builds a renderable report.
///
/// Never fails on routine hardware conditions: a READ-ONLY diagnosis is a
/// successful evaluation, not an error.
pub fn doctor(paths: SystemPaths, reader: LinuxSysfsReader) -> DoctorReport {
    let mode = SupportEvaluator::new(paths.clone(), reader).evaluate();
    let mut report = DoctorReport {
        mode,
        lines: Vec::new(),
    };

    match DeviceDetector::new(paths.clone(), reader).detect_identity() {
        Ok(device) => report.push(
            Verdict::Pass,
            format!("MSI laptop detected: {}", device.product_name),
        ),
        Err(DetectionError::UnsupportedVendor { .. }) => {
            report.push(Verdict::Fail, "MSI laptop not detected");
        }
        Err(_) => report.push(Verdict::Fail, "Hardware identity could not be verified"),
    }

    if cfg!(target_os = "linux") {
        report.push(Verdict::Pass, "Linux environment supported");
    } else {
        report.push(Verdict::Warn, "Non-Linux environment: unsupported");
    }

    let root_enumerable = match reader.list_entries(&paths.msi_ec_root()) {
        Ok(_) => {
            report.push(Verdict::Pass, "msi-ec interface available");
            true
        }
        Err(SysfsError::NotFound(_)) => {
            report.push(Verdict::Fail, "msi-ec interface unavailable");
            false
        }
        Err(_) => {
            report.push(Verdict::Fail, "msi-ec interface unreadable");
            false
        }
    };

    // Coherence diagnostics require an enumerable root: without one there is
    // no interface to call coherent, so feature lines are omitted rather
    // than fabricated. The final mode still comes from `SupportEvaluator`.
    if root_enumerable {
        match CapabilityDetector::new(paths.clone(), reader).discover() {
            Ok(capabilities) => {
                report.push(Verdict::Pass, "EC interface coherent");
                push_feature(
                    &mut report,
                    !capabilities.fan_modes.is_empty(),
                    "Fan controls",
                );
                push_feature(
                    &mut report,
                    !capabilities.shift_modes.is_empty(),
                    "Shift controls",
                );
                push_feature(
                    &mut report,
                    capabilities.battery_thresholds,
                    "Battery thresholds",
                );
                push_feature(
                    &mut report,
                    capabilities.keyboard_backlight.is_some(),
                    "Keyboard backlight",
                );
                push_feature(&mut report, capabilities.cpu_temperature, "CPU temperature");
                push_feature(&mut report, capabilities.gpu_temperature, "GPU temperature");
            }
            Err(CapabilityDiscoveryError::Read { .. }) => {
                report.push(Verdict::Fail, "EC capabilities unreadable");
            }
            Err(_) => report.push(Verdict::Fail, "EC interface inconsistent"),
        }
    }

    report
}
