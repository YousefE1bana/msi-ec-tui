//! Privacy-conscious compatibility reporting for `mec doctor --export`.
//!
//! [`CompatibilityReport`] renders one shared [`CompatibilityEvaluation`]
//! as deterministic Markdown/text. It includes only the allowlist: MEC
//! version, OS/arch, hardware identity, support mode, and capability
//! presence. It never dumps sysfs files, environment, paths, usernames,
//! hostnames, serials, UUIDs, MACs, IPs, profile/config contents,
//! notifications, or live telemetry values.
//!
//! Output is deterministic: no timestamps, no random IDs.

use std::fmt;

use crate::hardware::{DeviceInfo, LinuxSysfsReader, ReadOnlyReason, SupportMode, SystemPaths};

use super::evaluation::{CapabilityStatus, CompatibilityEvaluation, evaluate_compatibility};

/// Maximum displayed characters per hardware text field.
const MAX_FIELD_CHARS: usize = 128;

/// Owned, deterministically renderable compatibility report.
#[derive(Debug, Clone)]
pub struct CompatibilityReport {
    evaluation: CompatibilityEvaluation,
}

impl CompatibilityReport {
    /// Renders one prepared compatibility evaluation. Pure formatting over
    /// shared data: no detection, no discovery, no second evaluation pass.
    pub fn from_evaluation(evaluation: &CompatibilityEvaluation) -> Self {
        Self {
            evaluation: evaluation.clone(),
        }
    }

    /// Evaluates hardware through the shared evaluator and builds the
    /// report. Never fails on routine hardware conditions.
    pub fn evaluate(paths: SystemPaths, reader: LinuxSysfsReader) -> Self {
        Self::from_evaluation(&evaluate_compatibility(paths, reader))
    }
}

/// Report-specific sanitizer for sysfs-sourced identity text: control
/// characters become spaces, whitespace runs collapse, output stays one
/// logical line, ordinary Unicode is preserved, and length is bounded.
/// [`DeviceInfo`] itself is never mutated for display.
pub(crate) fn sanitize_field(value: &str) -> String {
    let flat: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(MAX_FIELD_CHARS).collect()
}

fn display_field(value: Option<&str>) -> String {
    match value {
        Some(text) => {
            let clean = sanitize_field(text);
            if clean.is_empty() {
                "N/A".to_owned()
            } else {
                clean
            }
        }
        None => "N/A".to_owned(),
    }
}

fn presence(supported: Option<bool>) -> &'static str {
    match supported {
        Some(true) => "Supported",
        Some(false) => "Unavailable",
        None => "Unknown",
    }
}

fn mode_list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
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

impl fmt::Display for CompatibilityReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let evaluation = &self.evaluation;
        let device = evaluation.device.as_ref();
        // Unchecked, unreadable, or inconsistent discovery renders as
        // Unknown: the report never pretends capabilities were checked
        // when the shared evaluation says they were not.
        let capabilities = match &evaluation.capabilities {
            CapabilityStatus::Available(caps) => Some(caps),
            CapabilityStatus::Unreadable
            | CapabilityStatus::Inconsistent
            | CapabilityStatus::NotChecked => None,
        };
        let none_device = DeviceInfo {
            manufacturer: String::new(),
            product_name: String::new(),
            board_name: None,
            bios_version: None,
            ec_firmware_version: None,
        };
        let device = device.unwrap_or(&none_device);
        let has_device = evaluation.device.is_some();

        writeln!(f, "# MEC Compatibility Report")?;
        writeln!(f)?;
        writeln!(f, "MEC version: {}", env!("CARGO_PKG_VERSION"))?;
        writeln!(f, "OS: {}", std::env::consts::OS)?;
        writeln!(f, "Architecture: {}", std::env::consts::ARCH)?;
        writeln!(f)?;
        writeln!(f, "## Hardware")?;
        writeln!(
            f,
            "Manufacturer: {}",
            if has_device {
                display_field(Some(&device.manufacturer))
            } else {
                "N/A".to_owned()
            }
        )?;
        writeln!(
            f,
            "Product: {}",
            if has_device {
                display_field(Some(&device.product_name))
            } else {
                "N/A".to_owned()
            }
        )?;
        writeln!(f, "Board: {}", display_field(device.board_name.as_deref()))?;
        writeln!(f, "BIOS: {}", display_field(device.bios_version.as_deref()))?;
        writeln!(
            f,
            "EC firmware: {}",
            display_field(device.ec_firmware_version.as_deref())
        )?;
        writeln!(f)?;
        writeln!(f, "## Support")?;
        match &evaluation.mode {
            SupportMode::Ready => writeln!(f, "Mode: READY")?,
            SupportMode::ReadOnly(reason) => {
                writeln!(f, "Mode: READ-ONLY")?;
                writeln!(f, "Reason: {}", reason_text(reason))?;
            }
        }
        writeln!(f)?;
        writeln!(f, "## Capabilities")?;
        let flag = |value: Option<bool>| presence(value);
        writeln!(
            f,
            "CPU temperature: {}",
            flag(capabilities.map(|c| c.cpu_temperature))
        )?;
        writeln!(
            f,
            "GPU temperature: {}",
            flag(capabilities.map(|c| c.gpu_temperature))
        )?;
        writeln!(f, "CPU fan: {}", flag(capabilities.map(|c| c.cpu_fan)))?;
        writeln!(f, "GPU fan: {}", flag(capabilities.map(|c| c.gpu_fan)))?;
        writeln!(
            f,
            "Cooler boost: {}",
            flag(capabilities.map(|c| c.cooler_boost))
        )?;
        writeln!(
            f,
            "Super battery: {}",
            flag(capabilities.map(|c| c.super_battery))
        )?;
        writeln!(f, "Webcam: {}", flag(capabilities.map(|c| c.webcam)))?;
        writeln!(
            f,
            "Webcam block: {}",
            flag(capabilities.map(|c| c.webcam_block))
        )?;
        writeln!(f, "Fn key: {}", flag(capabilities.map(|c| c.fn_key)))?;
        writeln!(f, "Win key: {}", flag(capabilities.map(|c| c.win_key)))?;
        writeln!(
            f,
            "Keyboard backlight: {}",
            flag(capabilities.map(|c| c.keyboard_backlight.is_some()))
        )?;
        writeln!(
            f,
            "Battery thresholds: {}",
            flag(capabilities.map(|c| c.battery_thresholds))
        )?;
        match capabilities {
            Some(caps) => {
                let fans: Vec<String> = caps
                    .fan_modes
                    .iter()
                    .map(|mode| mode.as_str().to_owned())
                    .collect();
                let shifts: Vec<String> = caps
                    .shift_modes
                    .iter()
                    .map(|mode| mode.as_str().to_owned())
                    .collect();
                writeln!(f, "Fan modes: {}", mode_list(&fans))?;
                writeln!(f, "Shift modes: {}", mode_list(&shifts))?;
            }
            None => {
                writeln!(f, "Fan modes: Unknown")?;
                writeln!(f, "Shift modes: Unknown")?;
            }
        }
        writeln!(f)?;
        writeln!(f, "## Privacy")?;
        writeln!(f, "- No serial number collected")?;
        writeln!(f, "- No system UUID collected")?;
        writeln!(f, "- No hostname or username collected")?;
        writeln!(f, "- No network addresses collected")?;
        writeln!(f, "- No profile/config contents collected")?;
        writeln!(f, "- No live telemetry values included")?;
        Ok(())
    }
}

/// Builds the export report through the shared evaluators.
pub fn compatibility_report(paths: SystemPaths, reader: LinuxSysfsReader) -> CompatibilityReport {
    CompatibilityReport::evaluate(paths, reader)
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

    fn report_for(name: &str) -> String {
        compatibility_report(fixture_root(name), LinuxSysfsReader).to_string()
    }

    /// Sentinel values for every excluded category. None of these may ever
    /// appear in an exported report.
    const SENTINELS: [&str; 8] = [
        "SECRET-SERIAL-123",
        "SECRET-UUID-456",
        "secret-hostname",
        "secret-user",
        "10.99.88.77",
        "AA:BB:CC:DD:EE:FF",
        "/home/private-user",
        "secret-profile-content",
    ];

    /// Builds a TempDir sysfs root seeded with sentinel files the report
    /// must never read, plus a hostile product name the sanitizer must
    /// keep on one line.
    fn sentinel_root() -> (tempfile::TempDir, SystemPaths) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let dmi = root.join("sys/class/dmi/id");
        std::fs::create_dir_all(&dmi).unwrap();
        std::fs::write(dmi.join("sys_vendor"), b"MSI\n").unwrap();
        // Hostile product name: newline markdown injection plus a control
        // character plus an overlong tail.
        let mut product = b"Evil Product\n# Injected Heading\x01 ".to_vec();
        product.extend(std::iter::repeat_n(b'w', 300));
        std::fs::write(dmi.join("product_name"), &product).unwrap();
        // Excluded-category files the report must never touch.
        std::fs::write(dmi.join("product_serial"), b"SECRET-SERIAL-123\n").unwrap();
        std::fs::write(dmi.join("product_uuid"), b"SECRET-UUID-456\n").unwrap();
        let ec = root.join("sys/devices/platform/msi-ec");
        std::fs::create_dir_all(&ec).unwrap();
        std::fs::write(ec.join("secret_host"), b"secret-hostname secret-user\n").unwrap();
        std::fs::write(ec.join("secret_net"), b"10.99.88.77 AA:BB:CC:DD:EE:FF\n").unwrap();
        std::fs::write(ec.join("secret_home"), b"/home/private-user\n").unwrap();
        std::fs::write(ec.join("secret_profile"), b"secret-profile-content\n").unwrap();
        // Transient telemetry the report must omit.
        std::fs::create_dir_all(ec.join("cpu")).unwrap();
        std::fs::write(ec.join("cpu/realtime_temperature"), b"73\n").unwrap();
        std::fs::write(ec.join("cpu/realtime_fan_speed"), b"88\n").unwrap();
        (dir, SystemPaths::new(root.to_path_buf()))
    }

    #[test]
    fn ready_fixture_report_is_deterministic() {
        let first = report_for("gf63");
        let second = report_for("gf63");
        assert_eq!(first, second);
    }

    #[test]
    fn read_only_fixture_includes_stable_reason() {
        let text = report_for("broken-sysfs");
        assert!(text.contains("Mode: READ-ONLY"));
        assert!(text.contains("Reason: Inconsistent hardware interface"));
    }

    #[test]
    fn report_includes_version_os_arch() {
        let text = report_for("gf63");
        assert!(text.contains(&format!("MEC version: {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains(&format!("OS: {}", std::env::consts::OS)));
        assert!(text.contains(&format!("Architecture: {}", std::env::consts::ARCH)));
    }

    #[test]
    fn report_includes_identity_and_capabilities() {
        let text = report_for("gf63");
        assert!(text.contains("Manufacturer: Micro-Star International Co., Ltd."));
        assert!(text.contains("Product: GF63 Thin 11UC"));
        assert!(text.contains("CPU temperature: Supported"));
        assert!(text.contains("Battery thresholds: Supported"));
        assert!(text.contains("Fan modes: auto, silent, basic, advanced"));
        assert!(text.contains("Shift modes: eco, comfort, sport, turbo"));
    }

    #[test]
    fn missing_optional_identity_renders_na() {
        let text = report_for("partial-device");
        assert!(text.contains("Product: Modern 15 Test Fixture"));
        assert!(text.contains("Board: N/A") || text.contains("BIOS: N/A"));
    }

    #[test]
    fn fan_shift_modes_follow_driver_order_and_survive() {
        use super::super::evaluation::{CapabilityStatus, evaluate_compatibility};
        let paths = fixture_root("gf63");
        let evaluation = evaluate_compatibility(paths, LinuxSysfsReader);
        let available = match &evaluation.capabilities {
            CapabilityStatus::Available(caps) => caps.clone(),
            other => panic!("gf63 capabilities must be available, got {other:?}"),
        };
        let fans: Vec<String> = available
            .fan_modes
            .iter()
            .map(|mode| mode.as_str().to_owned())
            .collect();
        let text = report_for("gf63");
        assert!(text.contains(&format!("Fan modes: {}", fans.join(", "))));
        // Future valid mode names survive verbatim.
        let unknown = report_for("unknown-device");
        assert!(unknown.contains("Future MSI Model XYZ-999"));
    }

    #[test]
    fn report_states_no_physical_fan_count() {
        let text = report_for("gf63");
        assert!(!text.to_lowercase().contains("fan count"));
        assert!(!text.contains("Number of fans"));
    }

    #[test]
    fn report_never_labels_fans_rpm() {
        for name in ["gf63", "broken-sysfs", "unknown-device"] {
            assert!(!report_for(name).contains("RPM"));
        }
    }

    #[test]
    fn report_omits_transient_telemetry() {
        let text = report_for("gf63");
        assert!(!text.contains("°C"));
        assert!(!text.contains("Charging"));
        assert!(!text.contains("Connected"));
        assert!(!text.contains("realtime"));
    }

    #[test]
    fn privacy_declaration_appears() {
        let text = report_for("gf63");
        for line in [
            "No serial number collected",
            "No system UUID collected",
            "No hostname or username collected",
            "No network addresses collected",
            "No profile/config contents collected",
            "No live telemetry values included",
        ] {
            assert!(text.contains(line), "{line:?} missing");
        }
    }

    #[test]
    fn sentinel_values_never_appear() {
        let (_dir, paths) = sentinel_root();
        let text = compatibility_report(paths, LinuxSysfsReader).to_string();
        for sentinel in SENTINELS {
            assert!(!text.contains(sentinel), "{sentinel:?} leaked into report");
        }
        // Transient values from the same root are omitted too.
        assert!(!text.contains("73\n"));
        // Allowed identity still renders (sanitized, single line).
        assert!(text.contains("Manufacturer: MSI"));
        assert!(text.lines().any(|line| line.contains("Evil Product")));
    }

    #[test]
    fn hostile_product_name_stays_one_logical_field() {
        let (_dir, paths) = sentinel_root();
        let text = compatibility_report(paths, LinuxSysfsReader).to_string();
        assert!(!text.lines().any(|line| line.starts_with("# Injected")));
        assert!(!text.contains('\u{1}'));
        assert!(
            text.lines()
                .any(|line| line.contains("Evil Product # Injected"))
        );
    }

    #[test]
    fn sanitizer_collapses_controls_and_bounds_length() {
        assert_eq!(sanitize_field("MSI\nThin\t11UC"), "MSI Thin 11UC");
        assert_eq!(sanitize_field("a\x07b"), "a b");
        assert_eq!(sanitize_field("  spaced   out  "), "spaced out");
        assert_eq!(sanitize_field("Ünïcödé ✓ laptop"), "Ünïcödé ✓ laptop");
        let long = "w".repeat(300);
        assert_eq!(sanitize_field(&long).chars().count(), MAX_FIELD_CHARS);
        assert_eq!(sanitize_field(""), "");
    }

    #[test]
    fn export_capability_values_come_from_shared_evaluation() {
        use super::super::evaluation::{CapabilityStatus, evaluate_compatibility};
        let paths = fixture_root("gf63");
        let evaluation = evaluate_compatibility(paths, LinuxSysfsReader);
        let available = match &evaluation.capabilities {
            CapabilityStatus::Available(caps) => caps.clone(),
            other => panic!("gf63 capabilities must be available, got {other:?}"),
        };
        let text = CompatibilityReport::from_evaluation(&evaluation).to_string();
        let fans: Vec<String> = available
            .fan_modes
            .iter()
            .map(|mode| mode.as_str().to_owned())
            .collect();
        assert!(text.contains(&format!("Fan modes: {}", fans.join(", "))));
        assert!(text.contains("CPU temperature: Supported"));
    }

    #[test]
    fn unchecked_capabilities_render_as_unknown() {
        use super::super::evaluation::{CapabilityStatus, evaluate_compatibility};
        let missing = SystemPaths::new(PathBuf::from("/nonexistent-mec-fixture-root"));
        let evaluation = evaluate_compatibility(missing, LinuxSysfsReader);
        assert_eq!(evaluation.capabilities, CapabilityStatus::NotChecked);
        let text = CompatibilityReport::from_evaluation(&evaluation).to_string();
        assert!(text.contains("CPU temperature: Unknown"));
        assert!(text.contains("Fan modes: Unknown"));
        assert!(text.contains("Mode: READ-ONLY"));
    }

    #[test]
    fn from_evaluation_matches_direct_export() {
        use super::super::evaluation::evaluate_compatibility;
        for name in ["gf63", "partial-device", "unknown-device", "broken-sysfs"] {
            let paths = fixture_root(name);
            let direct = compatibility_report(paths.clone(), LinuxSysfsReader).to_string();
            let shared = CompatibilityReport::from_evaluation(&evaluate_compatibility(
                paths,
                LinuxSysfsReader,
            ))
            .to_string();
            assert_eq!(direct, shared, "{name}");
        }
    }
}
