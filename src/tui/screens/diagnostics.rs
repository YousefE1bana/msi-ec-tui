//! Diagnostics screen in the approved v1.1 card system.
//!
//! Descriptive only: identity, compatibility verdict, telemetry state, and
//! a capability matrix over already-supplied startup data. Never probes
//! hardware. Status words (PASS/WARN/FAIL/Unavailable) map directly from
//! real support, telemetry, and capability state; nothing is hard-coded.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::app::{AppState, LiveHardware};
use crate::hardware::{Capabilities, DeviceInfo, EcBackend, SupportMode};

use crate::tui::shell;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    capability_style, read_only_reason_text, support_mode_style, support_mode_text, support_text,
    telemetry_state_text, telemetry_style,
};

/// Renders the startup-metadata summary: identity, compatibility verdict,
/// telemetry state, and capability matrix. Descriptive only: no probing.
pub fn render_diagnostics<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    render_diagnostics_with_theme(
        frame,
        area,
        &AppState::default(),
        live,
        capabilities,
        &Theme::default(),
    );
}

/// Theme-aware diagnostics renderer: approved shell plus real state.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_diagnostics_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme.base_style()),
        area,
    );
    let (top, workspace, footer) = shell::shell_split(area);
    shell::render_top_strip(frame, top, live, theme);
    shell::render_bottom_strip(frame, footer, live, theme);
    let regions = hit_regions(workspace);
    if regions.cards.len() != 5 {
        return;
    }
    let focus = app.focused_card();
    render_system_card(frame, regions.cards[0], live, focus == 0, theme);
    render_controls_card(frame, regions.cards[1], capabilities, focus == 1, theme);
    render_telemetry_card(frame, regions.cards[2], live, focus == 2, theme);
    render_device_card(frame, regions.cards[3], live, focus == 3, theme);
    render_export_card(frame, regions.cards[4], focus == 4, theme);
}

/// Card layout in focus order: system, controls, telemetry, device,
/// export. Diagnostics carries no interactive rows.
pub(crate) fn hit_regions(workspace: Rect) -> shell::ScreenRegions {
    let (top, mid, bottom) = shell::vsplit3(workspace, 34, 42);
    let (tl, tr) = shell::hpair(top, 50);
    let (ml, mr) = shell::hpair(mid, 50);
    shell::ScreenRegions {
        cards: vec![tl, tr, ml, mr, bottom],
        rows: Vec::new(),
    }
}

/// Human verdict for a live telemetry reading: present values pass while
/// the sample is fresh, degraded samples fail, and absent samples stay
/// unavailable. Waiting (no sample yet, no failure) warns.
fn telemetry_verdict<B: EcBackend>(
    present: bool,
    live: &LiveHardware<B>,
    theme: &Theme,
) -> ratatui::text::Span<'static> {
    if present && !live.is_degraded() {
        ratatui::text::Span::styled(
            "PASS".to_owned(),
            Style::default()
                .fg(theme.success)
                .add_modifier(Modifier::BOLD),
        )
    } else if live.is_degraded() {
        ratatui::text::Span::styled(
            "FAIL".to_owned(),
            Style::default()
                .fg(theme.danger)
                .add_modifier(Modifier::BOLD),
        )
    } else if present {
        ratatui::text::Span::styled("WARN".to_owned(), Style::default().fg(theme.warning))
    } else {
        ratatui::text::Span::styled("Unavailable".to_owned(), Style::default().fg(theme.muted))
    }
}

fn matrix_row(label: &'static str, supported: bool, theme: &Theme) -> Line<'static> {
    Line::styled(
        format!("{label}: {}", support_text(supported)),
        capability_style(supported, theme),
    )
}

fn render_system_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "SYSTEM", focused, theme);
    let on_linux = std::env::consts::OS == "linux";
    let msi = live.device().manufacturer == "MSI";
    let fw_known = live.device().ec_firmware_version.is_some();
    let mut lines = vec![
        verdict_line("MSI hardware", msi, theme),
        verdict_line("Linux", on_linux, theme),
        verdict_line("msi-ec interface", !live.is_degraded(), theme),
        verdict_line("EC firmware", fw_known, theme),
    ];
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        ratatui::text::Span::styled("Mode: ", Style::default().fg(theme.muted)),
        ratatui::text::Span::styled(
            support_mode_text(live.mode()).to_owned(),
            support_mode_style(live.mode(), theme),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

/// Support verdict mapped to PASS/WARN: supported passes, missing stays
/// unavailable (absence is not failure).
fn verdict_line(label: &str, supported: bool, theme: &Theme) -> Line<'static> {
    if supported {
        Line::from(vec![
            ratatui::text::Span::styled(format!("{label}: "), Style::default().fg(theme.muted)),
            ratatui::text::Span::styled(
                "PASS".to_owned(),
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            ratatui::text::Span::styled(format!("{label}: "), Style::default().fg(theme.muted)),
            ratatui::text::Span::styled("Unavailable".to_owned(), Style::default().fg(theme.muted)),
        ])
    }
}

#[allow(clippy::too_many_arguments)]
fn render_controls_card(
    frame: &mut Frame,
    area: Rect,
    capabilities: &Capabilities,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "CONTROLS", focused, theme);
    let lines = matrix_lines(capabilities, theme);
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_telemetry_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "TELEMETRY", focused, theme);
    let snapshot = live.current_snapshot();
    let mut lines = vec![
        Line::from(vec![
            ratatui::text::Span::styled("CPU Temp: ", Style::default().fg(theme.muted)),
            telemetry_verdict(
                snapshot.and_then(|s| s.cpu_temperature).is_some(),
                live,
                theme,
            ),
        ]),
        Line::from(vec![
            ratatui::text::Span::styled("GPU Temp: ", Style::default().fg(theme.muted)),
            telemetry_verdict(
                snapshot.and_then(|s| s.gpu_temperature).is_some(),
                live,
                theme,
            ),
        ]),
        Line::from(vec![
            ratatui::text::Span::styled("Fans: ", Style::default().fg(theme.muted)),
            telemetry_verdict(
                snapshot.and_then(|s| s.cpu_fan).is_some()
                    || snapshot.and_then(|s| s.gpu_fan).is_some(),
                live,
                theme,
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            ratatui::text::Span::styled("State: ", Style::default().fg(theme.muted)),
            ratatui::text::Span::styled(
                telemetry_state_text(live.is_degraded(), snapshot.is_some()).to_owned(),
                telemetry_style(live.is_degraded(), snapshot.is_some(), theme),
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
            Style::default().fg(theme.danger),
        ));
    }
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_device_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = shell::card(frame, area, "DEVICE INFO", focused, theme);
    let lines = identity_lines(live.device());
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

fn render_export_card(frame: &mut Frame, area: Rect, focused: bool, theme: &Theme) {
    let inner = shell::card(frame, area, "EXPORT / SUPPORT", focused, theme);
    let lines = export_lines();
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(lines)).style(shell::card_style(theme)),
        inner,
    );
}

/// Privacy-conscious export guidance. The renderer never executes doctor
/// or touches the filesystem: export is explicit and user-controlled via
/// `mec doctor --export`, whose report omits serials, hostnames, network
/// data, profile/config contents, and live telemetry.
pub(crate) fn export_lines() -> Vec<Line<'static>> {
    vec![
        Line::from("Export: mec doctor --export"),
        Line::from("Privacy: explicit, user-controlled; paste into an issue"),
        Line::from("Omits serials, hostnames, network, configs, telemetry"),
    ]
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
    use super::{hit_regions, render_diagnostics};

    fn text() -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        screen_text(160, 50, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        })
    }

    fn text_in(mode: SupportMode) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], mode, 1);
        let capabilities = full_capabilities();
        screen_text(160, 50, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        })
    }

    #[test]
    fn renders_shell_and_card_headings() {
        let text = text();
        for heading in [
            "SYSTEM",
            "CONTROLS",
            "TELEMETRY",
            "DEVICE INFO",
            "EXPORT / SUPPORT",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(text.contains(" MEC "));
    }

    #[test]
    fn system_checks_derive_from_real_state() {
        let text = text();
        assert!(text.contains("MSI hardware"));
        assert!(text.contains("PASS"));
        assert!(text.contains("Linux"));
    }

    #[test]
    fn controls_matrix_uses_supported_labels() {
        let text = text();
        assert!(text.contains("Fan Modes: Supported"));
        assert!(text.contains("Shift Modes: Supported"));
    }

    #[test]
    fn unsupported_capability_stays_unavailable() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let mut capabilities = full_capabilities();
        capabilities.cooler_boost = false;
        let text = screen_text(160, 50, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("Cooler Boost: Unavailable"));
    }

    #[test]
    fn telemetry_passes_when_live() {
        let text = text();
        assert!(text.contains("CPU Temp:"));
        assert!(text.contains("PASS"));
    }

    #[test]
    fn degraded_telemetry_fails_honestly() {
        let (live, _) = live_for(
            vec![Ok(healthy_snapshot()), Err(BackendError::Unavailable)],
            SupportMode::Ready,
            2,
        );
        let capabilities = full_capabilities();
        let text = screen_text(160, 50, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
        assert!(text.contains("DEGRADED"));
        assert!(text.contains("FAIL"));
    }

    #[test]
    fn identity_shows_real_device() {
        let text = text();
        assert!(text.contains("Manufacturer:"));
        assert!(text.contains("EC Firmware:"));
    }

    #[test]
    fn export_keeps_privacy_rules() {
        let text = text();
        assert!(text.contains("mec doctor --export"));
        assert!(text.contains("Omits serials"));
    }

    #[test]
    fn read_only_shows_reason() {
        let text = text_in(SupportMode::ReadOnly(ReadOnlyReason::MsiEcUnavailable));
        assert!(text.contains("READ-ONLY"));
        assert!(text.contains("msi-ec unavailable"));
    }

    #[test]
    fn hit_regions_carry_five_cards_without_rows() {
        let regions = hit_regions(ratatui::layout::Rect::new(0, 0, 160, 48));
        assert_eq!(regions.cards.len(), 5);
        assert!(regions.rows.is_empty());
    }

    #[test]
    fn zero_area_does_not_panic() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::layout::Rect;
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let caps = full_capabilities();
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_diagnostics(frame, Rect::new(0, 0, 0, 0), &live, &caps);
            })
            .expect("zero-area diagnostics draws");
    }
}
