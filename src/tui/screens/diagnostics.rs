//! Read-only diagnostics screen: startup metadata summary.
//!
//! Descriptive only: identity, compatibility verdict, telemetry state, and
//! a capability matrix over already-supplied startup data. Never probes
//! hardware, never renders PASS/WARN/FAIL verdicts.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;

use crate::app::LiveHardware;
use crate::hardware::{Capabilities, DeviceInfo, EcBackend, SupportMode};

use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, read_only_reason_text, render_panel, render_screen_shell, support_mode_style,
    support_mode_text, support_text, telemetry_state_text, telemetry_style,
};

/// Renders the startup-metadata summary: identity, compatibility verdict,
/// telemetry state, and capability matrix. Descriptive only: no probing,
/// no verdict engine, no PASS/WARN/FAIL labels.
pub fn render_diagnostics<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    render_diagnostics_with_theme(frame, area, live, capabilities, &Theme::default());
}

/// Theme-aware diagnostics renderer behind the Task-5 API.
pub(crate) fn render_diagnostics_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    let content = render_screen_shell(frame, area, "Diagnostics", live, theme);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(content);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(0)])
        .split(columns[0]);
    render_panel(
        frame,
        left[0],
        " IDENTITY ",
        identity_lines(live.device()),
        theme,
    );
    render_panel(
        frame,
        left[1],
        " TELEMETRY ",
        telemetry_lines(live, theme),
        theme,
    );
    render_panel(
        frame,
        columns[1],
        " CAPABILITIES ",
        matrix_lines(capabilities, theme),
        theme,
    );
}

fn optional_text(value: Option<&String>) -> &str {
    value.map(String::as_str).unwrap_or("N/A")
}

pub(crate) fn identity_lines(device: &DeviceInfo) -> Vec<Line<'static>> {
    vec![
        Line::from(format!("Manufacturer: {}", device.manufacturer)),
        Line::from(format!("Product: {}", device.product_name)),
        Line::from(format!(
            "Board: {}",
            optional_text(device.board_name.as_ref())
        )),
        Line::from(format!(
            "BIOS: {}",
            optional_text(device.bios_version.as_ref())
        )),
        Line::from(format!(
            "EC Firmware: {}",
            optional_text(device.ec_firmware_version.as_ref())
        )),
    ]
}

pub(crate) fn telemetry_lines<B: EcBackend>(
    live: &LiveHardware<B>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    use ratatui::text::Span;

    let mut lines = vec![
        Line::from(vec![
            Span::raw("Mode: "),
            Span::styled(
                support_mode_text(live.mode()).to_owned(),
                support_mode_style(live.mode(), theme),
            ),
        ]),
        Line::from(vec![
            Span::raw("Telemetry: "),
            Span::styled(
                telemetry_state_text(live.is_degraded(), live.current_snapshot().is_some())
                    .to_owned(),
                telemetry_style(live.is_degraded(), live.current_snapshot().is_some(), theme),
            ),
        ]),
    ];
    if let SupportMode::ReadOnly(reason) = live.mode() {
        lines.push(Line::from(format!(
            "Reason: {}",
            read_only_reason_text(reason)
        )));
    }
    if let Some(error) = live.snapshot_error() {
        lines.push(Line::styled(
            error.to_string(),
            ratatui::style::Style::default().fg(theme.danger),
        ));
    }
    lines
}

fn matrix_row(label: &'static str, supported: bool, theme: &Theme) -> Line<'static> {
    Line::styled(
        format!("{label}: {}", support_text(supported)),
        capability_style(supported, theme),
    )
}

pub(crate) fn matrix_lines(capabilities: &Capabilities, theme: &Theme) -> Vec<Line<'static>> {
    vec![
        matrix_row("CPU Temperature", capabilities.cpu_temperature, theme),
        matrix_row("GPU Temperature", capabilities.gpu_temperature, theme),
        matrix_row("CPU Fan", capabilities.cpu_fan, theme),
        matrix_row("GPU Fan", capabilities.gpu_fan, theme),
        matrix_row("Fan Modes", !capabilities.fan_modes.is_empty(), theme),
        matrix_row("Shift Modes", !capabilities.shift_modes.is_empty(), theme),
        matrix_row("Cooler Boost", capabilities.cooler_boost, theme),
        matrix_row("Super Battery", capabilities.super_battery, theme),
        matrix_row("Webcam", capabilities.webcam, theme),
        matrix_row("Webcam Block", capabilities.webcam_block, theme),
        matrix_row("Fn Key", capabilities.fn_key, theme),
        matrix_row("Win Key", capabilities.win_key, theme),
        matrix_row(
            "Keyboard Backlight",
            capabilities.keyboard_backlight.is_some(),
            theme,
        ),
        matrix_row("Battery Thresholds", capabilities.battery_thresholds, theme),
    ]
}

#[cfg(test)]
mod tests {
    use crate::hardware::{BackendError, ReadOnlyReason, SupportMode};

    use super::super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::render_diagnostics;

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        })
    }

    fn text_in(mode: SupportMode) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], mode, 1);
        let capabilities = full_capabilities();
        screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        })
    }

    #[test]
    fn renders_manufacturer() {
        assert!(text().contains("MSI"));
    }

    #[test]
    fn renders_product_name() {
        assert!(text().contains("Secondary Screen Fixture"));
    }

    #[test]
    fn renders_board() {
        assert!(text().contains("MS-99XY"));
    }

    #[test]
    fn renders_bios_version() {
        assert!(text().contains("E99XYIMS.100"));
    }

    #[test]
    fn renders_ec_firmware_version() {
        assert!(text().contains("99XYEMS1.100"));
    }

    #[test]
    fn missing_optional_identity_renders_na() {
        use crate::app::LiveHardware;
        use crate::hardware::DeviceInfo;
        use crate::monitoring::SnapshotHistory;

        use super::super::support::CountingBackend;

        let minimal = DeviceInfo {
            manufacturer: "MSI".to_owned(),
            product_name: "Minimal Fixture".to_owned(),
            board_name: None,
            bios_version: None,
            ec_firmware_version: None,
        };
        let mut live = LiveHardware::new(
            minimal,
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(healthy_snapshot())]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Board: N/A"));
        assert!(text.contains("BIOS: N/A"));
        assert!(text.contains("EC Firmware: N/A"));
    }

    #[test]
    fn ready_shown_correctly() {
        let text = text();
        assert!(text.contains("READY"));
        assert!(!text.contains("READ-ONLY"));
    }

    #[test]
    fn read_only_shown_correctly() {
        let text = text_in(SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable));
        assert!(text.contains("READ-ONLY"));
        assert!(text.contains("msi-ec unavailable"));
    }

    #[test]
    fn read_only_reason_uses_stable_text() {
        let text = text_in(SupportMode::ReadOnly(ReadOnlyReason::NonMsiHardware));
        assert!(text.contains("Non-MSI hardware"));
    }

    #[test]
    fn live_telemetry_shown() {
        let text = text();
        assert!(text.contains("LIVE"));
        assert!(!text.contains("DEGRADED"));
    }

    #[test]
    fn waiting_telemetry_shown() {
        let (live, _) = live_for(vec![], SupportMode::Ready, 0);
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("WAITING"));
    }

    #[test]
    fn degraded_telemetry_shown() {
        let (live, _) = live_for(
            vec![
                Ok(healthy_snapshot()),
                Err(BackendError::InvalidData("ec busy".to_owned())),
            ],
            SupportMode::Ready,
            2,
        );
        let capabilities = full_capabilities();
        let text = screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("DEGRADED"));
        assert!(text.contains("ec busy"));
    }

    #[test]
    fn capability_matrix_includes_supported_items() {
        let text = text();
        for row in [
            "CPU Temperature: Supported",
            "CPU Fan: Supported",
            "Fan Modes: Supported",
            "Shift Modes: Supported",
            "Cooler Boost: Supported",
            "Webcam: Supported",
            "Fn Key: Supported",
            "Keyboard Backlight: Supported",
            "Battery Thresholds: Supported",
        ] {
            assert!(text.contains(row), "{row:?} missing");
        }
    }

    #[test]
    fn capability_matrix_includes_unavailable_items() {
        let text = text();
        for row in [
            "GPU Temperature: Unavailable",
            "Super Battery: Unavailable",
            "Webcam Block: Unavailable",
            "Win Key: Unavailable",
        ] {
            assert!(text.contains(row), "{row:?} missing");
        }
    }

    #[test]
    fn unavailable_capability_is_not_labeled_fail() {
        let text = text();
        assert!(!text.contains("FAIL"));
        assert!(!text.contains("PASS"));
        assert!(!text.contains("WARN"));
    }
}
