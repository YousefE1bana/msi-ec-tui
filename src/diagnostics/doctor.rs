//! First diagnostic surface: `mec doctor` compatibility reporting.
//!
//! Read-only. Renders one shared [`CompatibilityEvaluation`]; the support
//! verdict always comes from [`SupportEvaluator`] via that evaluation.
//! Formatting lives here, policy lives in `hardware` plus the shared
//! evaluation composition.

use std::fmt;

use crate::hardware::{LinuxSysfsReader, ReadOnlyReason, SupportMode, SystemPaths};

use super::evaluation::{
    CapabilityStatus, CompatibilityEvaluation, IdentityOutcome, InterfaceStatus,
    evaluate_compatibility,
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
    DoctorReport::from_evaluation(&evaluate_compatibility(paths, reader))
}

impl DoctorReport {
    /// Renders one prepared compatibility evaluation. Pure formatting over
    /// shared data: no detection, no discovery, no second evaluation pass.
    pub fn from_evaluation(evaluation: &CompatibilityEvaluation) -> Self {
        let mut report = DoctorReport {
            mode: evaluation.mode.clone(),
            lines: Vec::new(),
        };

        match evaluation.identity {
            IdentityOutcome::Msi => report.push(
                Verdict::Pass,
                format!(
                    "MSI laptop detected: {}",
                    evaluation
                        .device
                        .as_ref()
                        .map(|device| device.product_name.as_str())
                        .unwrap_or("unknown product")
                ),
            ),
            IdentityOutcome::NotMsi => {
                report.push(Verdict::Fail, "MSI laptop not detected");
            }
            IdentityOutcome::Unverified => {
                report.push(Verdict::Fail, "Hardware identity could not be verified");
            }
        }

        if cfg!(target_os = "linux") {
            report.push(Verdict::Pass, "Linux environment supported");
        } else {
            report.push(Verdict::Warn, "Non-Linux environment: unsupported");
        }

        match evaluation.interface {
            InterfaceStatus::Available => {
                report.push(Verdict::Pass, "msi-ec interface available");
            }
            InterfaceStatus::Unavailable => {
                report.push(Verdict::Fail, "msi-ec interface unavailable");
                return report;
            }
            InterfaceStatus::Unreadable => {
                report.push(Verdict::Fail, "msi-ec interface unreadable");
                return report;
            }
        }

        // Coherence diagnostics require an enumerable root: without one
        // there is no interface to call coherent, so feature lines are
        // omitted rather than fabricated. The evaluation guarantees
        // `NotChecked` exactly in that case.
        match &evaluation.capabilities {
            CapabilityStatus::Available(capabilities) => {
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
            CapabilityStatus::Unreadable => {
                report.push(Verdict::Fail, "EC capabilities unreadable");
            }
            CapabilityStatus::Inconsistent => {
                report.push(Verdict::Fail, "EC interface inconsistent");
            }
            CapabilityStatus::NotChecked => {}
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_root(name: &str) -> SystemPaths {
        SystemPaths::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(name),
        )
    }

    #[test]
    fn gf63_output_preserves_established_lines_in_order() {
        let text = doctor(fixture_root("gf63"), LinuxSysfsReader).to_string();
        let expected = [
            "MEC Doctor",
            "[PASS] MSI laptop detected: GF63 Thin 11UC",
            "[PASS] Linux environment supported",
            "[PASS] msi-ec interface available",
            "[PASS] EC interface coherent",
            "[PASS] Fan controls available",
            "[PASS] Shift controls available",
            "[PASS] Battery thresholds available",
            "[PASS] Keyboard backlight available",
            "[PASS] CPU temperature available",
            "[PASS] GPU temperature available",
            "Mode: READY",
        ];
        let mut cursor = 0;
        for line in expected {
            let found = text[cursor..]
                .find(line)
                .unwrap_or_else(|| panic!("{line:?} missing from doctor output"));
            cursor += found + line.len();
        }
    }

    #[test]
    fn gf63_output_matches_historical_text_exactly() {
        let text = doctor(fixture_root("gf63"), LinuxSysfsReader).to_string();
        assert_eq!(
            text,
            "MEC Doctor\n\
             \n\
             [PASS] MSI laptop detected: GF63 Thin 11UC\n\
             [PASS] Linux environment supported\n\
             [PASS] msi-ec interface available\n\
             [PASS] EC interface coherent\n\
             [PASS] Fan controls available\n\
             [PASS] Shift controls available\n\
             [PASS] Battery thresholds available\n\
             [PASS] Keyboard backlight available\n\
             [PASS] CPU temperature available\n\
             [PASS] GPU temperature available\n\
             \n\
             Mode: READY\n"
        );
    }

    #[test]
    fn broken_fixture_output_matches_historical_text_exactly() {
        let text = doctor(fixture_root("broken-sysfs"), LinuxSysfsReader).to_string();
        assert_eq!(
            text,
            "MEC Doctor\n\
             \n\
             [PASS] MSI laptop detected: Broken Interface Test Fixture\n\
             [PASS] Linux environment supported\n\
             [PASS] msi-ec interface available\n\
             [FAIL] EC interface inconsistent\n\
             \n\
             Mode: READ-ONLY\n\
             Reason: Inconsistent hardware interface\n"
        );
    }

    #[test]
    fn from_evaluation_matches_direct_doctor() {
        use super::super::evaluation::evaluate_compatibility;
        for name in ["gf63", "partial-device", "unknown-device", "broken-sysfs"] {
            let paths = fixture_root(name);
            let direct = doctor(paths.clone(), LinuxSysfsReader).to_string();
            let shared =
                DoctorReport::from_evaluation(&evaluate_compatibility(paths, LinuxSysfsReader))
                    .to_string();
            assert_eq!(direct, shared, "{name}");
        }
        let missing = SystemPaths::new(PathBuf::from("/nonexistent-mec-fixture-root"));
        let direct = doctor(missing.clone(), LinuxSysfsReader).to_string();
        let shared =
            DoctorReport::from_evaluation(&evaluate_compatibility(missing, LinuxSysfsReader))
                .to_string();
        assert_eq!(direct, shared);
    }
}
