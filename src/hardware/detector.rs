//! Linux DMI device identity detection.

use std::path::PathBuf;

use thiserror::Error;

use super::{DeviceInfo, SysfsReader, SystemPaths, paths::PathValidationError, sysfs::SysfsError};

#[derive(Debug, Error)]
pub enum DetectionError {
    #[error("unsupported vendor: {detected:?}")]
    UnsupportedVendor { detected: String },
    #[error("required field {field:?} is missing")]
    MissingRequiredField { field: &'static str },
    #[error("field {field:?} contains invalid data")]
    InvalidField { field: &'static str },
    #[error("failed to read field {field}")]
    ReadFailure {
        field: &'static str,
        source: SysfsError,
    },
    #[error("failed to resolve path for {field}")]
    PathFailure {
        field: &'static str,
        source: PathValidationError,
    },
}

/// DMI identity fields read by the detector.
#[derive(Debug, Clone, Copy)]
enum DmiField {
    SysVendor,
    ProductName,
    BoardName,
    BiosVersion,
    EcFirmwareVersion,
}

impl DmiField {
    fn name(self) -> &'static str {
        match self {
            Self::SysVendor => "sys_vendor",
            Self::ProductName => "product_name",
            Self::BoardName => "board_name",
            Self::BiosVersion => "bios_version",
            Self::EcFirmwareVersion => "fw_version",
        }
    }
}

/// Accepted manufacturer identities, compared case-insensitively.
const MSI_VENDOR_FORMS: [&str; 3] = [
    "msi",
    "micro-star international",
    "micro-star international co., ltd.",
];

/// Collapses Unicode whitespace runs to single ASCII spaces and trims the ends.
fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_checked(value: &str, field: &'static str) -> Result<String, DetectionError> {
    let normalized = normalize(value);
    if normalized.is_empty() {
        Err(DetectionError::InvalidField { field })
    } else {
        Ok(normalized)
    }
}

fn is_msi_vendor(normalized_manufacturer: &str) -> bool {
    MSI_VENDOR_FORMS.contains(&normalized_manufacturer.to_lowercase().as_str())
}

/// Detects device identity from Linux DMI without deciding feature support.
pub struct DeviceDetector<R> {
    paths: SystemPaths,
    reader: R,
}

impl<R> DeviceDetector<R>
where
    R: SysfsReader,
{
    /// Creates a detector over `paths` using the supplied reader.
    pub fn new(paths: SystemPaths, reader: R) -> Self {
        Self { paths, reader }
    }

    /// Detects device identity from DMI plus the optional EC firmware
    /// version, without requiring the msi-ec driver.
    pub fn detect(&self) -> Result<DeviceInfo, DetectionError> {
        let mut device = self.detect_identity()?;
        device.ec_firmware_version = self.optional(DmiField::EcFirmwareVersion)?;
        Ok(device)
    }

    /// Detects device identity from DMI alone, without touching the
    /// `msi-ec` interface at all. Optional EC firmware enrichment must
    /// never determine whether identity can be established.
    pub fn detect_identity(&self) -> Result<DeviceInfo, DetectionError> {
        let manufacturer = self.required(DmiField::SysVendor)?;
        if !is_msi_vendor(&manufacturer) {
            return Err(DetectionError::UnsupportedVendor {
                detected: manufacturer,
            });
        }
        let product_name = self.required(DmiField::ProductName)?;
        let board_name = self.optional(DmiField::BoardName)?;
        let bios_version = self.optional(DmiField::BiosVersion)?;
        Ok(DeviceInfo {
            manufacturer,
            product_name,
            board_name,
            bios_version,
            ec_firmware_version: None,
        })
    }

    fn required(&self, field: DmiField) -> Result<String, DetectionError> {
        let path = self.path(field)?;
        let value = self
            .reader
            .read_string(&path)
            .map_err(|source| match source {
                SysfsError::NotFound(_) => DetectionError::MissingRequiredField {
                    field: field.name(),
                },
                source => DetectionError::ReadFailure {
                    field: field.name(),
                    source,
                },
            })?;
        normalize_checked(&value, field.name())
    }

    fn optional(&self, field: DmiField) -> Result<Option<String>, DetectionError> {
        let path = self.path(field)?;
        match self.reader.read_string(&path) {
            Ok(value) => normalize_checked(&value, field.name()).map(Some),
            Err(SysfsError::NotFound(_)) => Ok(None),
            Err(source) => Err(DetectionError::ReadFailure {
                field: field.name(),
                source,
            }),
        }
    }

    fn path(&self, field: DmiField) -> Result<PathBuf, DetectionError> {
        let resolved = match field {
            DmiField::EcFirmwareVersion => self.paths.msi_ec("fw_version"),
            _ => self.paths.dmi(field.name()),
        };
        resolved.map_err(|source| DetectionError::PathFailure {
            field: field.name(),
            source,
        })
    }
}
