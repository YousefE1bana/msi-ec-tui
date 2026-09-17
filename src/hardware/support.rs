//! Conservative compatibility policy: READY or READ-ONLY with a reason.
//!
//! This layer decides only whether MEC may eventually permit hardware
//! mutation. It performs no writes and interprets no live values.

use super::{
    CapabilityDetector, CapabilityDiscoveryError, DetectionError, DeviceDetector, SysfsError,
    SysfsReader, SystemPaths,
};

/// Whether MEC may eventually permit hardware mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupportMode {
    Ready,
    ReadOnly(ReadOnlyReason),
}

/// Why MEC must remain read-only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOnlyReason {
    /// The machine is positively identified as non-MSI hardware.
    NonMsiHardware,
    /// The machine identity could not be established at all.
    UnverifiedHardwareIdentity,
    /// MSI identity is established but the `msi-ec` interface is absent.
    MsiEcUnavailable,
    /// MSI identity is established but the `msi-ec` interface cannot be read.
    MsiEcUnreadable,
    /// MSI identity is established but the interface is internally inconsistent.
    InconsistentInterface,
}

/// Classifies the current machine/interface as ready or read-only.
pub struct SupportEvaluator<R> {
    paths: SystemPaths,
    reader: R,
}

impl<R> SupportEvaluator<R>
where
    R: SysfsReader + Clone,
{
    /// Creates an evaluator over `paths` using the supplied reader.
    pub fn new(paths: SystemPaths, reader: R) -> Self {
        Self { paths, reader }
    }

    /// Evaluates support deterministically: MSI DMI identity, then
    /// interface readability, then capability coherence. Uncertainty means
    /// read-only.
    ///
    /// Identity uses [`DeviceDetector::detect_identity`], which never touches
    /// the `msi-ec` interface: a malformed driver path can therefore never
    /// mask the vendor verdict. Only after MSI identity succeeds does the
    /// evaluator inspect the driver root.
    pub fn evaluate(&self) -> SupportMode {
        if let Err(error) =
            DeviceDetector::new(self.paths.clone(), self.reader.clone()).detect_identity()
        {
            return match error {
                DetectionError::UnsupportedVendor { .. } => {
                    SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware)
                }
                _ => SupportMode::ReadOnly(ReadOnlyReason::UnverifiedHardwareIdentity),
            };
        }

        if let Err(error) = self
            .reader
            .list_entries(&self.paths.msi_ec_root())
            .map(|_| ())
        {
            return match error {
                SysfsError::NotFound(_) => SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable),
                _ => SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnreadable),
            };
        }

        match CapabilityDetector::new(self.paths.clone(), self.reader.clone()).discover() {
            Ok(_) => SupportMode::Ready,
            Err(CapabilityDiscoveryError::Read { .. }) => {
                SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnreadable)
            }
            Err(_) => SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface),
        }
    }
}
