//! Single diagnostics compatibility evaluation shared by `mec doctor`
//! and `mec doctor --export`.
//!
//! [`evaluate_compatibility`] is the only diagnostics production location
//! that orchestrates [`SupportEvaluator`], [`DeviceDetector`],
//! [`CapabilityDetector`], and `msi-ec` root enumeration. Both surfaces
//! consume the resulting [`CompatibilityEvaluation`] as pure data;
//! formatting stays surface-specific, so no strings pass between them.
//! Hardware support rules live in `hardware` and are unchanged here.

use crate::hardware::{
    Capabilities, CapabilityDetector, CapabilityDiscoveryError, DetectionError, DeviceDetector,
    DeviceInfo, SupportEvaluator, SupportMode, SysfsError, SysfsReader, SystemPaths,
};

/// Whether DMI identifies an MSI laptop. Established independently of
/// `msi-ec` availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityOutcome {
    /// An MSI vendor with a readable product name.
    Msi,
    /// A readable vendor that is not MSI.
    NotMsi,
    /// Identity could not be established.
    Unverified,
}

/// `msi-ec` root enumeration outcome. `NotFound` and other list errors
/// stay distinct because `doctor` reports them differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceStatus {
    /// The root enumerates; capability checks may proceed.
    Available,
    /// The root is missing.
    Unavailable,
    /// The root exists but cannot be enumerated.
    Unreadable,
}

/// Capability discovery outcome. `NotChecked` marks the gated case where
/// the interface itself could not be enumerated: capabilities must then
/// render as unchecked, never as probed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityStatus {
    /// Discovered capabilities, in driver-reported order.
    Available(Capabilities),
    /// A capability node failed to read.
    Unreadable,
    /// Discovery failed any other way (paths, modes, coherence).
    Inconsistent,
    /// Skipped because the interface gate failed.
    NotChecked,
}

/// Owned compatibility evaluation: support verdict, identity
/// classification, best-effort enriched device info, interface outcome,
/// and capability outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityEvaluation {
    /// Final support verdict from [`SupportEvaluator`].
    pub mode: SupportMode,
    /// DMI identity classification.
    pub identity: IdentityOutcome,
    /// Best-effort identity: EC-firmware enrichment never erases an
    /// established MSI identity, and never decides MSI-ness by itself.
    pub device: Option<DeviceInfo>,
    /// `msi-ec` root enumeration outcome.
    pub interface: InterfaceStatus,
    /// Capability discovery outcome (gated on the interface).
    pub capabilities: CapabilityStatus,
}

/// Evaluates compatibility once through the production hardware
/// abstractions. Generic over the sysfs reader so policy stays testable
/// without physical hardware; production wrappers pass
/// [`LinuxSysfsReader`](crate::hardware::LinuxSysfsReader).
pub fn evaluate_compatibility<R: SysfsReader + Clone>(
    paths: SystemPaths,
    reader: R,
) -> CompatibilityEvaluation {
    let mode = SupportEvaluator::new(paths.clone(), reader.clone()).evaluate();

    let detector = DeviceDetector::new(paths.clone(), reader.clone());
    let (identity, device) = match detector.detect_identity() {
        Ok(base) => {
            // Best-effort EC-firmware enrichment: a failed enrichment
            // keeps the established identity instead of erasing it.
            let enriched = detector.detect().unwrap_or(base);
            (IdentityOutcome::Msi, Some(enriched))
        }
        Err(DetectionError::UnsupportedVendor { .. }) => (IdentityOutcome::NotMsi, None),
        Err(_) => (IdentityOutcome::Unverified, None),
    };

    let interface = match reader.list_entries(&paths.msi_ec_root()) {
        Ok(_) => InterfaceStatus::Available,
        Err(SysfsError::NotFound(_)) => InterfaceStatus::Unavailable,
        Err(_) => InterfaceStatus::Unreadable,
    };

    let capabilities = match interface {
        InterfaceStatus::Available => match CapabilityDetector::new(paths, reader).discover() {
            Ok(found) => CapabilityStatus::Available(found),
            Err(CapabilityDiscoveryError::Read { .. }) => CapabilityStatus::Unreadable,
            Err(_) => CapabilityStatus::Inconsistent,
        },
        InterfaceStatus::Unavailable | InterfaceStatus::Unreadable => CapabilityStatus::NotChecked,
    };

    CompatibilityEvaluation {
        mode,
        identity,
        device,
        interface,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::LinuxSysfsReader;
    use std::path::PathBuf;

    fn fixture_root(name: &str) -> SystemPaths {
        SystemPaths::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(name),
        )
    }

    fn evaluate_fixture(name: &str) -> CompatibilityEvaluation {
        evaluate_compatibility(fixture_root(name), LinuxSysfsReader)
    }

    #[test]
    fn ready_fixture_evaluates_fully_available() {
        let evaluation = evaluate_fixture("gf63");
        assert_eq!(evaluation.mode, SupportMode::Ready);
        assert_eq!(evaluation.identity, IdentityOutcome::Msi);
        assert_eq!(evaluation.interface, InterfaceStatus::Available);
        assert!(matches!(
            evaluation.capabilities,
            CapabilityStatus::Available(_)
        ));
        let device = evaluation.device.expect("gf63 has identity");
        assert_eq!(device.product_name, "GF63 Thin 11UC");
    }

    #[test]
    fn missing_root_marks_interface_unavailable_with_unchecked_capabilities() {
        let evaluation = evaluate_compatibility(
            SystemPaths::new(PathBuf::from("/nonexistent-mec-fixture-root")),
            LinuxSysfsReader,
        );
        assert!(matches!(evaluation.mode, SupportMode::ReadOnly(_)));
        assert_eq!(evaluation.interface, InterfaceStatus::Unavailable);
        assert_eq!(evaluation.capabilities, CapabilityStatus::NotChecked);
        assert_eq!(evaluation.identity, IdentityOutcome::Unverified);
        assert_eq!(evaluation.device, None);
    }

    #[test]
    fn unreadable_root_marks_interface_unreadable_with_unchecked_capabilities() {
        let dir = tempfile::tempdir().unwrap();
        let dmi = dir.path().join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
        std::fs::write(dmi.join("product_name"), b"Evaluation Fixture\n").unwrap();
        // Regular file where the driver directory belongs.
        let ec = dir.path().join("sys/devices/platform/msi-ec");
        std::fs::create_dir_all(ec.parent().unwrap()).unwrap();
        std::fs::write(&ec, b"not a directory\n").unwrap();
        let evaluation =
            evaluate_compatibility(SystemPaths::new(dir.path().to_path_buf()), LinuxSysfsReader);
        assert_eq!(evaluation.interface, InterfaceStatus::Unreadable);
        assert_eq!(evaluation.capabilities, CapabilityStatus::NotChecked);
        // DMI identity is independent of interface readability.
        assert_eq!(evaluation.identity, IdentityOutcome::Msi);
        assert!(evaluation.device.is_some());
    }

    #[test]
    fn enumerable_but_broken_interface_marks_capabilities_inconsistent() {
        let evaluation = evaluate_fixture("broken-sysfs");
        assert_eq!(evaluation.interface, InterfaceStatus::Available);
        assert_eq!(evaluation.capabilities, CapabilityStatus::Inconsistent);
    }

    #[test]
    fn valid_identity_survives_missing_ec_firmware() {
        let dir = tempfile::tempdir().unwrap();
        let dmi = dir.path().join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
        std::fs::write(dmi.join("product_name"), b"Firmwareless Fixture\n").unwrap();
        // No msi-ec tree at all: no enrichment source exists.
        let evaluation =
            evaluate_compatibility(SystemPaths::new(dir.path().to_path_buf()), LinuxSysfsReader);
        assert_eq!(evaluation.identity, IdentityOutcome::Msi);
        let device = evaluation.device.expect("identity survives");
        assert_eq!(device.product_name, "Firmwareless Fixture");
        assert_eq!(device.ec_firmware_version, None);
        assert_eq!(evaluation.interface, InterfaceStatus::Unavailable);
        assert_eq!(evaluation.capabilities, CapabilityStatus::NotChecked);
    }

    #[test]
    fn valid_identity_survives_enrichment_failure() {
        let dir = tempfile::tempdir().unwrap();
        let dmi = dir.path().join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
        std::fs::write(dmi.join("product_name"), b"Enrichment Fixture\n").unwrap();
        // An msi-ec node exists, but the firmware file is a directory, so
        // enrichment fails while identity must stand.
        let ec = dir.path().join("sys/devices/platform/msi-ec");
        std::fs::create_dir_all(ec.join("fw_version")).unwrap();
        let evaluation =
            evaluate_compatibility(SystemPaths::new(dir.path().to_path_buf()), LinuxSysfsReader);
        assert_eq!(evaluation.identity, IdentityOutcome::Msi);
        let device = evaluation.device.expect("identity survives");
        assert_eq!(device.product_name, "Enrichment Fixture");
        assert_eq!(device.ec_firmware_version, None);
    }

    #[test]
    fn non_msi_vendor_classifies_not_msi_without_device() {
        let dir = tempfile::tempdir().unwrap();
        let dmi = dir.path().join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), b"OtherVendor\n").unwrap();
        std::fs::write(dmi.join("product_name"), b"Not A Laptop\n").unwrap();
        let evaluation =
            evaluate_compatibility(SystemPaths::new(dir.path().to_path_buf()), LinuxSysfsReader);
        assert_eq!(evaluation.identity, IdentityOutcome::NotMsi);
        assert_eq!(evaluation.device, None);
    }

    #[test]
    fn doctor_and_export_consume_one_shared_evaluation() {
        use crate::diagnostics::{CompatibilityReport, DoctorReport};
        let evaluation = evaluate_fixture("gf63");
        let doctor_text = DoctorReport::from_evaluation(&evaluation).to_string();
        let export_text = CompatibilityReport::from_evaluation(&evaluation).to_string();
        assert!(doctor_text.contains("MSI laptop detected: GF63 Thin 11UC"));
        assert!(doctor_text.contains("Mode: READY"));
        assert!(export_text.contains("Product: GF63 Thin 11UC"));
        assert!(export_text.contains("Mode: READY"));
    }

    #[test]
    fn rendering_from_evaluation_performs_zero_hardware_reads() {
        use std::cell::Cell;
        use std::path::Path;
        use std::rc::Rc;

        use crate::diagnostics::{CompatibilityReport, DoctorReport};
        use crate::hardware::{SysfsError, SysfsReader};

        struct CountingReader {
            inner: LinuxSysfsReader,
            reads: Rc<Cell<usize>>,
        }

        impl Clone for CountingReader {
            fn clone(&self) -> Self {
                Self {
                    inner: self.inner,
                    reads: Rc::clone(&self.reads),
                }
            }
        }

        impl SysfsReader for CountingReader {
            fn exists(&self, path: &Path) -> Result<bool, SysfsError> {
                self.reads.set(self.reads.get() + 1);
                self.inner.exists(path)
            }
            fn read_string(&self, path: &Path) -> Result<String, SysfsError> {
                self.reads.set(self.reads.get() + 1);
                self.inner.read_string(path)
            }
            fn read_u8(&self, path: &Path) -> Result<u8, SysfsError> {
                self.reads.set(self.reads.get() + 1);
                self.inner.read_u8(path)
            }
            fn read_u16(&self, path: &Path) -> Result<u16, SysfsError> {
                self.reads.set(self.reads.get() + 1);
                self.inner.read_u16(path)
            }
            fn list_dirs(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
                self.reads.set(self.reads.get() + 1);
                self.inner.list_dirs(path)
            }
            fn list_entries(&self, path: &Path) -> Result<Vec<PathBuf>, SysfsError> {
                self.reads.set(self.reads.get() + 1);
                self.inner.list_entries(path)
            }
        }

        let reads = Rc::new(Cell::new(0));
        let reader = CountingReader {
            inner: LinuxSysfsReader,
            reads: Rc::clone(&reads),
        };
        let evaluation = evaluate_compatibility(fixture_root("gf63"), reader);
        assert!(reads.get() > 0);
        reads.set(0);
        let _ = DoctorReport::from_evaluation(&evaluation).to_string();
        let _ = CompatibilityReport::from_evaluation(&evaluation).to_string();
        assert_eq!(reads.get(), 0);
    }

    #[test]
    fn evaluation_is_deterministic() {
        assert_eq!(evaluate_fixture("gf63"), evaluate_fixture("gf63"));
        assert_eq!(
            evaluate_fixture("broken-sysfs"),
            evaluate_fixture("broken-sysfs")
        );
    }
}
