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

    /// Evaluates support deterministically: interface readability, then
    /// identity, then capability coherence. Uncertainty means read-only.
    ///
    /// The root readability probe runs before identity on purpose: identity
    /// detection itself reads the optional `msi-ec/fw_version`, so a
    /// present-but-corrupt driver root would otherwise surface as an
    /// unverifiable identity instead of the hard interface failure it is.
    /// A merely absent root defers to identity first, so non-MSI machines
    /// are still reported as non-MSI rather than as missing-driver machines.
    pub fn evaluate(&self) -> SupportMode {
        let root_readable = match self.reader.list_entries(&self.paths.msi_ec_root()) {
            Ok(_) => true,
            Err(SysfsError::NotFound(_)) => false,
            Err(_) => return SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnreadable),
        };

        if let Err(error) = DeviceDetector::new(self.paths.clone(), self.reader.clone()).detect() {
            return match error {
                DetectionError::UnsupportedVendor { .. } => {
                    SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware)
                }
                _ => SupportMode::ReadOnly(ReadOnlyReason::UnverifiedHardwareIdentity),
            };
        }

        if !root_readable {
            return SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable);
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
