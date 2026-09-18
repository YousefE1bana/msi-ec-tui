//! Shared read-only presentation helpers for TUI screens.
//!
//! Pure text mapping over already-sampled domain values. No sampling, no
//! layout, no terminal lifecycle here.

use crate::hardware::{BatteryStatus, ReadOnlyReason, SupportMode};

/// "READY" or "READ-ONLY". A support verdict never implies transport
/// connectivity, so no "connected" language lives here.
pub(crate) fn support_mode_text(mode: &SupportMode) -> &'static str {
    match mode {
        SupportMode::Ready => "READY",
        SupportMode::ReadOnly(_) => "READ-ONLY",
    }
}

/// Stable human-readable read-only reason. Never `Debug` formatting.
pub(crate) fn read_only_reason_text(reason: &ReadOnlyReason) -> &'static str {
    match reason {
        ReadOnlyReason::NonMsiHardware => "Non-MSI hardware",
        ReadOnlyReason::UnverifiedHardwareIdentity => "Unverified hardware identity",
        ReadOnlyReason::MsiEcUnavailable => "msi-ec unavailable",
        ReadOnlyReason::MsiEcUnreadable => "msi-ec unreadable",
        ReadOnlyReason::InconsistentInterface => "Inconsistent hardware interface",
    }
}

/// Header telemetry word: a fresh failure is degraded, a current sample is
/// live, and no sample yet is waiting (never degraded).
pub(crate) fn telemetry_state_text(degraded: bool, has_current: bool) -> &'static str {
    if degraded {
        "DEGRADED"
    } else if has_current {
        "LIVE"
    } else {
        "WAITING"
    }
}

/// On/Off/N/A for boolean capabilities.
pub(crate) fn on_off_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "On",
        Some(false) => "Off",
        None => "N/A",
    }
}

/// Connected/Disconnected/N/A for AC presence.
pub(crate) fn ac_text(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "Connected",
        Some(false) => "Disconnected",
        None => "N/A",
    }
}

/// Charging-state vocabulary matching the existing CLI terminology.
pub(crate) fn battery_status_text(status: Option<&BatteryStatus>) -> &'static str {
    match status {
        Some(BatteryStatus::Unknown) => "Unknown",
        Some(BatteryStatus::Charging) => "Charging",
        Some(BatteryStatus::Discharging) => "Discharging",
        Some(BatteryStatus::NotCharging) => "Not charging",
        Some(BatteryStatus::Full) => "Full",
        None => "N/A",
    }
}
