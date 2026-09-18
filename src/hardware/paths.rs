//! Lexical hardware paths rooted at a production or caller-supplied root.
//!
//! This module does not access the filesystem or resolve symbolic links.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PathValidationError {
    #[error("path component must not be empty")]
    EmptyComponent,
    #[error("path component must not contain a separator or NUL")]
    ForbiddenCharacter,
    #[error("expected exactly one normal relative path component")]
    InvalidComponent,
}

fn validate_component(value: &str) -> Result<(), PathValidationError> {
    if value.is_empty() {
        return Err(PathValidationError::EmptyComponent);
    }
    if value.contains(['/', '\\', '\0']) {
        return Err(PathValidationError::ForbiddenCharacter);
    }
    let mut components = Path::new(value).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(PathValidationError::InvalidComponent);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPaths {
    root: PathBuf,
}

impl SystemPaths {
    pub fn linux() -> Self {
        Self::new("/")
    }

    /// Uses the supplied root as-is, without requiring it to exist.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves exactly one normal DMI attribute filename.
    pub fn dmi(&self, attribute: &str) -> Result<PathBuf, PathValidationError> {
        validate_component(attribute)?;
        Ok(self
            .root
            .join("sys")
            .join("class")
            .join("dmi")
            .join("id")
            .join(attribute))
    }

    /// Resolves normal relative components beneath the MSI EC base path.
    pub fn msi_ec(&self, relative: &str) -> Result<PathBuf, PathValidationError> {
        // Validate before Path::components can normalize interior dots or empty components.
        for component in relative.split('/') {
            validate_component(component)?;
        }
        Ok(self
            .root
            .join("sys")
            .join("devices")
            .join("platform")
            .join("msi-ec")
            .join(relative))
    }

    pub fn power_supply_root(&self) -> PathBuf {
        self.root.join("sys").join("class").join("power_supply")
    }

    /// The base directory of the `msi-ec` platform driver interface.
    pub fn msi_ec_root(&self) -> PathBuf {
        self.root
            .join("sys")
            .join("devices")
            .join("platform")
            .join("msi-ec")
    }

    pub fn leds_root(&self) -> PathBuf {
        self.root.join("sys").join("class").join("leds")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn linux_dmi_path() {
        assert_eq!(
            SystemPaths::linux().dmi("product_name").unwrap(),
            PathBuf::from("/sys/class/dmi/id/product_name")
        );
    }

    #[test]
    fn linux_ec_firmware_path() {
        assert_eq!(
            SystemPaths::linux().msi_ec("fw_version").unwrap(),
            PathBuf::from("/sys/devices/platform/msi-ec/fw_version")
        );
    }

    #[test]
    fn linux_nested_ec_path() {
        assert_eq!(
            SystemPaths::linux()
                .msi_ec("cpu/realtime_temperature")
                .unwrap(),
            PathBuf::from("/sys/devices/platform/msi-ec/cpu/realtime_temperature")
        );
    }

    #[test]
    fn linux_power_supply_root() {
        assert_eq!(
            SystemPaths::linux().power_supply_root(),
            PathBuf::from("/sys/class/power_supply")
        );
    }

    #[test]
    fn linux_msi_ec_root() {
        assert_eq!(
            SystemPaths::linux().msi_ec_root(),
            PathBuf::from("/sys/devices/platform/msi-ec")
        );
    }

    #[test]
    fn custom_msi_ec_root() {
        assert_eq!(
            SystemPaths::new("/tmp/fake-root").msi_ec_root(),
            PathBuf::from("/tmp/fake-root/sys/devices/platform/msi-ec")
        );
    }

    #[test]
    fn linux_leds_root() {
        assert_eq!(
            SystemPaths::linux().leds_root(),
            PathBuf::from("/sys/class/leds")
        );
    }

    #[test]
    fn custom_dmi_path() {
        assert_eq!(
            SystemPaths::new("/tmp/fake-root")
                .dmi("product_name")
                .unwrap(),
            PathBuf::from("/tmp/fake-root/sys/class/dmi/id/product_name")
        );
    }

    #[test]
    fn custom_nested_ec_path() {
        assert_eq!(
            SystemPaths::new("/tmp/fake-root")
                .msi_ec("cpu/realtime_temperature")
                .unwrap(),
            PathBuf::from("/tmp/fake-root/sys/devices/platform/msi-ec/cpu/realtime_temperature")
        );
    }

    #[test]
    fn custom_collection_roots() {
        let paths = SystemPaths::new("/tmp/fake-root");
        assert_eq!(
            paths.power_supply_root(),
            PathBuf::from("/tmp/fake-root/sys/class/power_supply")
        );
        assert_eq!(
            paths.leds_root(),
            PathBuf::from("/tmp/fake-root/sys/class/leds")
        );
    }

    macro_rules! rejection_test {
        ($name:ident, $method:ident, $($input:expr),+ $(,)?) => {
            #[test]
            fn $name() {
                let paths = SystemPaths::new("/tmp/fake-root");
                $(assert!(paths.$method($input).is_err(), "input: {:?}", $input);)+
            }
        };
    }

    rejection_test!(dmi_rejects_empty, dmi, "");
    rejection_test!(dmi_rejects_current_directory, dmi, ".");
    rejection_test!(dmi_rejects_parent, dmi, "..");
    rejection_test!(dmi_rejects_slash, dmi, "/", "foo/bar", "foo/", "foo//bar");
    rejection_test!(dmi_rejects_backslash, dmi, "\\", "foo\\bar");
    rejection_test!(dmi_rejects_nul, dmi, "foo\0bar");
    rejection_test!(dmi_rejects_absolute, dmi, "/product_name");
    rejection_test!(ec_rejects_empty, msi_ec, "");
    rejection_test!(
        ec_rejects_absolute,
        msi_ec,
        "/",
        "/fw_version",
        "//fw_version"
    );
    rejection_test!(ec_rejects_parent, msi_ec, "..", "../foo");
    rejection_test!(ec_rejects_middle_parent, msi_ec, "foo/../bar", "foo/..");
    rejection_test!(
        ec_rejects_current_directory,
        msi_ec,
        ".",
        "./foo",
        "foo/./bar",
        "foo/."
    );
    rejection_test!(ec_rejects_backslash, msi_ec, "foo\\bar", "cpu/\\value");
    rejection_test!(ec_rejects_nul, msi_ec, "cpu/value\0");
    rejection_test!(
        ec_rejects_empty_components,
        msi_ec,
        "cpu//value",
        "cpu/value/"
    );

    #[test]
    fn legitimate_nested_ec_path() {
        let paths = SystemPaths::new("/tmp/fake-root");
        let result = paths.msi_ec("gpu/realtime_fan_speed").unwrap();
        assert_eq!(
            result,
            PathBuf::from("/tmp/fake-root/sys/devices/platform/msi-ec/gpu/realtime_fan_speed")
        );
        assert!(result.starts_with("/tmp/fake-root/sys/devices/platform/msi-ec"));
    }

    #[test]
    fn relative_root_requires_no_existing_files() {
        let paths = SystemPaths::new(PathBuf::from("fixtures/nonexistent"));
        assert_eq!(
            paths.dmi("product_name").unwrap(),
            PathBuf::from("fixtures/nonexistent/sys/class/dmi/id/product_name")
        );
    }
}
