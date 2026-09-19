//! Shared read-only presentation helpers for TUI screens.
//!
//! Pure text mapping over already-sampled domain values, plus the common
//! secondary-screen shell (header, footer, small-area fallback). No
//! sampling, no terminal lifecycle here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::LiveHardware;
use crate::hardware::{
    BatteryStatus, EcBackend, FanMode, FanPercent, HardwareSnapshot, ReadOnlyReason, ShiftMode,
    SupportMode, TemperatureCelsius,
};

use super::theme::Theme;

/// Conservative fallback threshold shared by all screens: below this a
/// screen cannot show its content honestly, so a compact message replaces
/// it. Layout splits below never panic; only honest presentation matters.
pub(crate) const MIN_SCREEN_WIDTH: u16 = 60;
pub(crate) const MIN_SCREEN_HEIGHT: u16 = 14;

/// Truthful footer: every shortcut listed is implemented.
pub(crate) const SCREEN_FOOTER: &str = "Tab/arrows/hjkl Navigate • ? Help • Q Quit";

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

/// Capability existence label. "Supported"/"Unavailable" describe the
/// interface, never current state.
pub(crate) fn support_text(supported: bool) -> &'static str {
    if supported {
        "Supported"
    } else {
        "Unavailable"
    }
}

/// Style for the support verdict: healthy states succeed, read-only warns.
pub(crate) fn support_mode_style(mode: &SupportMode, theme: &Theme) -> Style {
    match mode {
        SupportMode::Ready => Style::default().fg(theme.success),
        SupportMode::ReadOnly(_) => Style::default().fg(theme.warning),
    }
}

/// Style for the telemetry word: failure endangers, silence mutes.
pub(crate) fn telemetry_style(degraded: bool, has_current: bool, theme: &Theme) -> Style {
    if degraded {
        Style::default().fg(theme.danger)
    } else if has_current {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.muted)
    }
}

/// Style for capability existence: supported succeeds, missing mutes.
pub(crate) fn capability_style(supported: bool, theme: &Theme) -> Style {
    if supported {
        Style::default().fg(theme.success)
    } else {
        Style::default().fg(theme.muted)
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

/// Upstream mode names in reported order, verbatim. Empty means the driver
/// reported no modes.
pub(crate) fn joined_modes<'a>(modes: impl Iterator<Item = &'a str>) -> String {
    let joined: Vec<&'a str> = modes.collect();
    if joined.is_empty() {
        "None reported".to_owned()
    } else {
        joined.join(", ")
    }
}

pub(crate) fn temperature_text(value: Option<TemperatureCelsius>) -> String {
    value
        .map(|reading| format!("{}°C", reading.get()))
        .unwrap_or_else(|| "N/A".to_owned())
}

pub(crate) fn fan_text(value: Option<FanPercent>) -> String {
    value
        .map(|reading| format!("{}%", reading.get()))
        .unwrap_or_else(|| "N/A".to_owned())
}

pub(crate) fn percent_text(value: Option<u8>) -> String {
    value
        .map(|level| format!("{level}%"))
        .unwrap_or_else(|| "N/A".to_owned())
}

pub(crate) fn fan_mode_text(mode: Option<&FanMode>) -> String {
    mode.map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "N/A".to_owned())
}

pub(crate) fn shift_mode_text(mode: Option<&ShiftMode>) -> String {
    mode.map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "N/A".to_owned())
}

pub(crate) fn backlight_text(level: Option<u8>) -> String {
    level
        .map(|value| value.to_string())
        .unwrap_or_else(|| "N/A".to_owned())
}

pub(crate) fn thermals_lines(snapshot: Option<&HardwareSnapshot>) -> Vec<Line<'static>> {
    let cpu = snapshot.and_then(|state| state.cpu_temperature);
    let gpu = snapshot.and_then(|state| state.gpu_temperature);
    let cpu_fan = snapshot.and_then(|state| state.cpu_fan);
    let gpu_fan = snapshot.and_then(|state| state.gpu_fan);
    vec![
        Line::from(format!("CPU Temperature: {}", temperature_text(cpu))),
        Line::from(format!("GPU Temperature: {}", temperature_text(gpu))),
        Line::from(format!("CPU Fan: {}", fan_text(cpu_fan))),
        Line::from(format!("GPU Fan: {}", fan_text(gpu_fan))),
    ]
}

pub(crate) fn performance_lines(snapshot: Option<&HardwareSnapshot>) -> Vec<Line<'static>> {
    let shift = snapshot.and_then(|state| state.shift_mode.as_ref());
    let fan = snapshot.and_then(|state| state.fan_mode.as_ref());
    let cooler = snapshot.and_then(|state| state.cooler_boost);
    let super_battery = snapshot.and_then(|state| state.super_battery);
    vec![
        Line::from(format!("Shift Mode: {}", shift_mode_text(shift))),
        Line::from(format!("Fan Mode: {}", fan_mode_text(fan))),
        Line::from(format!("Cooler Boost: {}", on_off_text(cooler))),
        Line::from(format!("Super Battery: {}", on_off_text(super_battery))),
    ]
}

pub(crate) fn battery_lines(snapshot: Option<&HardwareSnapshot>) -> Vec<Line<'static>> {
    let charge = snapshot.and_then(|state| state.battery_percentage);
    let state = snapshot.and_then(|state| state.battery_status.as_ref());
    let ac = snapshot.and_then(|state| state.ac_connected);
    let start = snapshot.and_then(|state| state.battery_start_threshold);
    let end = snapshot.and_then(|state| state.battery_end_threshold);
    vec![
        Line::from(format!("Charge: {}", percent_text(charge))),
        Line::from(format!("State: {}", battery_status_text(state))),
        Line::from(format!("AC: {}", ac_text(ac))),
        Line::from(format!("Start Threshold: {}", percent_text(start))),
        Line::from(format!("End Threshold: {}", percent_text(end))),
    ]
}

pub(crate) fn device_lines(snapshot: Option<&HardwareSnapshot>) -> Vec<Line<'static>> {
    let webcam = snapshot.and_then(|state| state.webcam);
    let block = snapshot.and_then(|state| state.webcam_block);
    let backlight = snapshot.and_then(|state| state.keyboard_backlight);
    vec![
        Line::from(format!("Webcam: {}", on_off_text(webcam))),
        Line::from(format!("Webcam Block: {}", on_off_text(block))),
        Line::from(format!("Keyboard Backlight: {}", backlight_text(backlight))),
    ]
}

/// Bordered panel with a styled title and content lines.
pub(crate) fn render_panel(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    lines: Vec<Line<'static>>,
    theme: &Theme,
) {
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            title,
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).style(theme.base_style()),
        inner,
    );
}

/// Compact fallback for areas too small for honest panels.
pub(crate) fn render_compact(frame: &mut Frame, area: Rect, theme: &Theme) {
    let text = Text::from(vec![
        Line::from("MEC"),
        Line::from("Terminal too small"),
        Line::from("Q Quit"),
    ]);
    frame.render_widget(Paragraph::new(text).style(theme.base_style()), area);
}

/// Shared secondary-screen shell: outer block, status header, truthful
/// footer. Returns the content area, or a zero area after rendering the
/// compact fallback on tiny terminals.
pub(crate) fn render_screen_shell<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    live: &LiveHardware<B>,
    theme: &Theme,
) -> Rect {
    if area.width < MIN_SCREEN_WIDTH || area.height < MIN_SCREEN_HEIGHT {
        render_compact(frame, area, theme);
        return Rect::new(0, 0, 0, 0);
    }
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            format!(" MEC — {title} "),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(Text::from(screen_header_lines(live, theme))).style(theme.base_style()),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(SCREEN_FOOTER).style(theme.base_style()),
        rows[2],
    );
    rows[1]
}

/// Status header shared by secondary screens: device, support verdict with
/// stable reason, telemetry state with readable error detail.
fn screen_header_lines<B: EcBackend>(live: &LiveHardware<B>, theme: &Theme) -> Vec<Line<'static>> {
    let mut mode_line = vec![
        Span::raw("Mode: "),
        Span::styled(
            support_mode_text(live.mode()).to_owned(),
            support_mode_style(live.mode(), theme),
        ),
    ];
    if let SupportMode::ReadOnly(reason) = live.mode() {
        mode_line.push(Span::raw(format!(" ({})", read_only_reason_text(reason))));
    }
    let mut header = vec![
        Line::from(format!("Device: {}", live.device().product_name)),
        Line::from(mode_line),
        Line::from(vec![
            Span::raw("Telemetry: "),
            Span::styled(
                telemetry_state_text(live.is_degraded(), live.current_snapshot().is_some())
                    .to_owned(),
                telemetry_style(live.is_degraded(), live.current_snapshot().is_some(), theme),
            ),
        ]),
    ];
    header.push(match live.snapshot_error() {
        Some(error) => Line::styled(error.to_string(), Style::default().fg(theme.danger)),
        None => Line::from(""),
    });
    header
}
