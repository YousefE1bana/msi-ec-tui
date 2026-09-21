//! Dashboard screen: approved six-card grid fed by real production state.
//!
//! Current values render [`LiveHardware::current_snapshot`] only, never
//! stale history. Labeled history rows read the bounded
//! [`SnapshotHistory`]; min/max come from that history, never invented.
//! Absent values render `N/A`. No sampling, no sysfs, no terminal
//! lifecycle, no prototype state: menu identity, card focus, and
//! navigation all ride the production [`AppState`].

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{AppState, LiveHardware};
use crate::hardware::{EcBackend, SupportMode};

use crate::tui::history;
use crate::tui::mouse;
use crate::tui::theme::Theme;
use crate::tui::ui::{
    MIN_SCREEN_HEIGHT, MIN_SCREEN_WIDTH, ac_text, backlight_text, battery_status_text,
    fan_mode_text, fan_text, on_off_text, percent_text, read_only_reason_text, render_compact,
    shift_mode_text, support_mode_style, support_mode_text, telemetry_state_text, telemetry_style,
    temperature_text,
};

/// Conservative fallback threshold: below this the dashboard cannot show
/// its panels honestly, so a compact message replaces it.
const MIN_DASHBOARD_WIDTH: u16 = MIN_SCREEN_WIDTH;
const MIN_DASHBOARD_HEIGHT: u16 = MIN_SCREEN_HEIGHT;

/// Menu rows 1-7 mirror production screens; row 8 is the approved
/// Settings entry and row 9 exits. Kept in sync with
/// [`mouse::MENU_LABELS`] by construction below.
const MENU_ORDER: [MenuRow; 9] = [
    MenuRow::Screen(crate::app::Screen::Dashboard),
    MenuRow::Screen(crate::app::Screen::Performance),
    MenuRow::Screen(crate::app::Screen::Fans),
    MenuRow::Screen(crate::app::Screen::Battery),
    MenuRow::Screen(crate::app::Screen::Devices),
    MenuRow::Screen(crate::app::Screen::Profiles),
    MenuRow::Screen(crate::app::Screen::Diagnostics),
    MenuRow::Settings,
    MenuRow::Exit,
];

/// One approved menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuRow {
    Screen(crate::app::Screen),
    Settings,
    Exit,
}

impl MenuRow {
    fn label(self) -> &'static str {
        match self {
            MenuRow::Screen(screen) => screen.title(),
            MenuRow::Settings => "Settings",
            MenuRow::Exit => "Exit",
        }
    }
}

/// Renders the dashboard into `area` with the default theme.
///
/// Reads only already-sampled [`LiveHardware`] state plus navigation
/// state, and performs zero backend calls.
pub fn render_dashboard<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
) {
    render_dashboard_with_theme(frame, area, app, live, &Theme::default());
}

/// Theme-aware dashboard renderer. [`render_dashboard`] stays
/// source-compatible while future themes gain an injection seam.
pub(crate) fn render_dashboard_with_theme<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    if area.width < MIN_DASHBOARD_WIDTH || area.height < MIN_DASHBOARD_HEIGHT {
        render_compact(frame, area, theme);
        return;
    }
    frame.render_widget(Block::default().style(theme.base_style()), area);
    let (top, workspace, footer) = shell_rows(area);
    render_top_strip(frame, top, live, theme);
    render_grid(frame, workspace, app, live, theme);
    render_bottom_strip(frame, footer, live, theme);
}

/// Shell rows: one-row status strip, flexible workspace, one-row footer.
/// Mirrors [`mouse::dashboard_shell`] over the same area.
fn shell_rows(area: Rect) -> (Rect, Rect, Rect) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);
    (rows[0], rows[1], rows[2])
}

/// Thin full-width top strip:
/// `MEC | device | READY/READ-ONLY | EC firmware | LIVE/... | clock`.
fn render_top_strip<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    let firmware = live
        .device()
        .ec_firmware_version
        .as_deref()
        .unwrap_or("N/A");
    let line = Line::from(vec![
        Span::styled(
            " MEC ",
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("│ ", Style::default().fg(theme.muted)),
        Span::styled(
            live.device().product_name.clone(),
            Style::default().fg(theme.foreground),
        ),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(
            support_mode_text(live.mode()).to_owned(),
            support_mode_style(live.mode(), theme).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(format!("EC {firmware}"), Style::default().fg(theme.muted)),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(
            telemetry_state_text(live.is_degraded(), live.current_snapshot().is_some()).to_owned(),
            telemetry_style(live.is_degraded(), live.current_snapshot().is_some(), theme)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(theme.muted)),
        Span::styled(clock(), Style::default().fg(theme.muted)),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme.base_style()), area);
}

/// UTC clock for the top strip, std only.
fn clock() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!(
        "{:02}:{:02}:{:02}",
        (secs / 3600) % 24,
        (secs / 60) % 60,
        secs % 60
    )
}

/// Approved footer. Span order and widths match [`mouse::footer_regions`]
/// exactly; the status segment follows the real support verdict.
fn render_bottom_strip<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    let ready = !matches!(live.mode(), SupportMode::ReadOnly(_));
    let status = if ready {
        " ✓ READY  "
    } else {
        " ✓ READ-ONLY  "
    };
    let status_style = if ready {
        Style::default()
            .fg(theme.success)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.warning)
            .add_modifier(Modifier::BOLD)
    };
    let key = |k: &str| Span::styled(format!(" [{k}] "), Style::default().fg(theme.accent));
    let what = |w: &str| Span::styled(format!("{w} "), Style::default().fg(theme.muted));
    let line = Line::from(vec![
        Span::styled(status.to_owned(), status_style),
        Span::styled("│ ", Style::default().fg(theme.muted)),
        key("1-9"),
        what("Select"),
        key("↑↓"),
        what("Navigate"),
        key("Enter"),
        what("Open"),
        key("?"),
        what("Help"),
        key("Q"),
        what("Quit"),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme.base_style()), area);
}

/// Approved six-card grid. Geometry comes from the shared hit-test
/// module so clicks and wheel events map onto exactly these rects.
fn render_grid<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    theme: &Theme,
) {
    if area.is_empty() {
        return;
    }
    let cards = mouse::grid_cards(area);
    if cards.len() != 6 {
        return;
    }
    let focus = app.dashboard_focus();
    render_control_card(frame, cards[0], app, theme);
    render_thermals_card(frame, cards[1], live, focus == 1, theme);
    render_cooling_card(frame, cards[2], live, focus == 2, theme);
    render_power_card(frame, cards[3], live, focus == 3, theme);
    render_performance_card(frame, cards[4], live, focus == 4, theme);
    render_device_card(frame, cards[5], live, focus == 5, theme);
}

/// Card shell: thin border (accent when focused), charcoal surface, cyan
/// title. Returns the inner content area.
fn card(frame: &mut Frame, area: Rect, title: &str, focused: bool, theme: &Theme) -> Rect {
    let border = if focused {
        Style::default().fg(theme.accent).bg(theme.surface)
    } else {
        Style::default().fg(theme.border).bg(theme.surface)
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border)
        .style(Style::default().bg(theme.surface))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

/// Cool telemetry meter: approved fill on a dark track.
fn bar_line(frac: f64, width: usize, theme: &Theme) -> Line<'static> {
    let frac = frac.clamp(0.0, 1.0);
    let n = (frac * width as f64).round() as usize;
    Line::from(vec![
        Span::styled("█".repeat(n), Style::default().fg(theme.meter_fill)),
        Span::styled(
            "░".repeat(width.saturating_sub(n)),
            Style::default().fg(theme.meter_track),
        ),
    ])
}

/// Charge-window range track over 0-100 with the cool meter fill.
fn window_bar(start: u8, end: u8, width: usize, theme: &Theme) -> Line<'static> {
    let a = (f64::from(start) / 100.0 * width as f64).round() as usize;
    let b = (f64::from(end) / 100.0 * width as f64).round() as usize;
    let mut spans = Vec::new();
    for i in 0..width {
        let (ch, color) = if i >= a && i < b {
            ("█", theme.meter_fill)
        } else {
            ("─", theme.meter_track)
        };
        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
    }
    Line::from(spans)
}

fn sep(width: usize, theme: &Theme) -> Line<'static> {
    Line::styled("─".repeat(width.max(4)), Style::default().fg(theme.muted))
}

/// Original-project identity plus the approved numeric menu. The active
/// row follows the production screen; Settings and Exit complete the
/// nine approved entries.
fn render_control_card(frame: &mut Frame, area: Rect, app: &AppState, theme: &Theme) {
    let inner = card(
        frame,
        area,
        "MEC CONTROL",
        app.dashboard_focus() == 0,
        theme,
    );
    let w = inner.width as usize;
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "[*] ",
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Yousef Osama",
                Style::default()
                    .fg(theme.foreground)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::styled(
            "    github.com/YousefE1bana",
            Style::default().fg(theme.muted),
        ),
        sep(w, theme),
        Line::from(vec![
            Span::styled("[+] ", Style::default().fg(theme.accent)),
            Span::styled("Available Options:", Style::default().fg(theme.foreground)),
        ]),
    ];
    for (index, row) in MENU_ORDER.iter().enumerate() {
        let active = matches!(row, MenuRow::Screen(screen) if *screen == app.current_screen());
        let name_style = if active {
            Style::default()
                .fg(theme.foreground)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        lines.push(Line::from(vec![
            Span::styled(
                if active { "▸ " } else { "  " },
                Style::default().fg(theme.accent),
            ),
            Span::styled(
                format!("[{}] ", index + 1),
                Style::default().fg(theme.accent),
            ),
            Span::styled(format!(" {label:<12}", label = row.label()), name_style),
        ]));
    }
    // Debug anchor: keep every menu label identical to the shared map.
    debug_assert!(MENU_ORDER.map(MenuRow::label) == mouse::MENU_LABELS);
    lines.push(sep(w, theme));
    lines.push(Line::from(vec![
        Span::styled("[+] Select Option > ", Style::default().fg(theme.accent)),
        Span::styled("1-9", Style::default().fg(theme.accent)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Current: ", Style::default().fg(theme.muted)),
        Span::styled(
            app.current_screen().title().to_owned(),
            Style::default().fg(theme.success),
        ),
    ]));
    let style = Style::default().fg(theme.foreground).bg(theme.surface);
    frame.render_widget(Paragraph::new(Text::from(lines)).style(style), inner);
}

/// Real CPU/GPU temperatures with cool meters plus labeled history rows
/// (latest/min/max from the bounded history, never invented).
fn render_thermals_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    // Focus state is read by the shared shell; keep the call local.
    let inner = card(frame, area, "THERMALS", focused, theme);
    let snapshot = live.current_snapshot();
    let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 56);
    let mut lines = vec![
        Line::styled("CPU Temperature", Style::default().fg(theme.accent)),
        Line::from(format!(
            "CPU Temperature: {}",
            temperature_text(snapshot.and_then(|s| s.cpu_temperature))
        )),
        bar_line(
            temp_frac(snapshot.and_then(|s| s.cpu_temperature)),
            bar_w,
            theme,
        ),
        Line::styled("GPU Temperature", Style::default().fg(theme.accent)),
        Line::from(format!(
            "GPU Temperature: {}",
            temperature_text(snapshot.and_then(|s| s.gpu_temperature))
        )),
        bar_line(
            temp_frac(snapshot.and_then(|s| s.gpu_temperature)),
            bar_w,
            theme,
        ),
    ];
    let budget = usize::from(inner.width.saturating_sub(2));
    lines.extend(
        history::temperature_history_lines(live.history(), budget)
            .into_iter()
            .map(Line::from),
    );
    let style = Style::default().fg(theme.foreground).bg(theme.surface);
    frame.render_widget(Paragraph::new(Text::from(lines)).style(style), inner);
}

/// Real fan percentages (never RPM) with cool meters and history rows.
fn render_cooling_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = card(frame, area, "COOLING", focused, theme);
    let snapshot = live.current_snapshot();
    let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 48);
    let mut lines = vec![
        Line::styled("CPU Fan", Style::default().fg(theme.accent)),
        Line::from(format!(
            "CPU Fan: {}",
            fan_text(snapshot.and_then(|s| s.cpu_fan))
        )),
        bar_line(fan_frac(snapshot.and_then(|s| s.cpu_fan)), bar_w, theme),
        Line::styled("GPU Fan", Style::default().fg(theme.accent)),
        Line::from(format!(
            "GPU Fan: {}",
            fan_text(snapshot.and_then(|s| s.gpu_fan))
        )),
        bar_line(fan_frac(snapshot.and_then(|s| s.gpu_fan)), bar_w, theme),
    ];
    let budget = usize::from(inner.width.saturating_sub(2));
    lines.extend(
        history::fan_history_lines(live.history(), budget)
            .into_iter()
            .map(Line::from),
    );
    let style = Style::default().fg(theme.foreground).bg(theme.surface);
    frame.render_widget(Paragraph::new(Text::from(lines)).style(style), inner);
}

/// Real battery, AC, and threshold state with meters.
fn render_power_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = card(frame, area, "POWER", focused, theme);
    let snapshot = live.current_snapshot();
    let bar_w = (inner.width as usize).saturating_sub(2).clamp(8, 48);
    let charge = snapshot.and_then(|s| s.battery_percentage);
    let start = snapshot.and_then(|s| s.battery_start_threshold);
    let end = snapshot.and_then(|s| s.battery_end_threshold);
    let mut lines = vec![
        Line::styled("Battery", Style::default().fg(theme.accent)),
        Line::from(format!("Charge: {}", percent_text(charge))),
        bar_line(charge_frac(charge), bar_w, theme),
        Line::from(format!(
            "State: {}",
            battery_status_text(snapshot.and_then(|s| s.battery_status.as_ref()))
        )),
        Line::from(format!(
            "AC: {}",
            ac_text(snapshot.and_then(|s| s.ac_connected))
        )),
        Line::styled("Charge Window", Style::default().fg(theme.accent)),
        Line::from(format!(
            "Start Threshold: {}    End Threshold: {}",
            percent_text(start),
            percent_text(end)
        )),
    ];
    if let (Some(a), Some(b)) = (start, end) {
        lines.push(window_bar(a, b, bar_w.min(40), theme));
    }
    let style = Style::default().fg(theme.foreground).bg(theme.surface);
    frame.render_widget(Paragraph::new(Text::from(lines)).style(style), inner);
}

/// Real shift/fan/boost/super-battery state.
fn render_performance_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = card(frame, area, "PERFORMANCE", focused, theme);
    let snapshot = live.current_snapshot();
    let shift = snapshot.and_then(|s| s.shift_mode.as_ref());
    let fan = snapshot.and_then(|s| s.fan_mode.as_ref());
    let lines = vec![
        Line::from(format!("Shift Mode: {}", shift_mode_text(shift))),
        Line::from(format!("Fan Mode: {}", fan_mode_text(fan))),
        Line::from(format!(
            "Cooler Boost: {}",
            on_off_text(snapshot.and_then(|s| s.cooler_boost))
        )),
        Line::from(format!(
            "Super Battery: {}",
            on_off_text(snapshot.and_then(|s| s.super_battery))
        )),
        Line::styled(
            format!(
                "Telemetry: {}",
                telemetry_state_text(live.is_degraded(), snapshot.is_some())
            ),
            Style::default().fg(theme.muted),
        ),
    ];
    let style = Style::default().fg(theme.foreground).bg(theme.surface);
    frame.render_widget(Paragraph::new(Text::from(lines)).style(style), inner);
}

/// Real device identity, peripherals, mode verdict, and readable errors.
fn render_device_card<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    live: &LiveHardware<B>,
    focused: bool,
    theme: &Theme,
) {
    let inner = card(frame, area, "DEVICE / SYSTEM", focused, theme);
    let snapshot = live.current_snapshot();
    let firmware = live
        .device()
        .ec_firmware_version
        .as_deref()
        .unwrap_or("N/A");
    let mut lines = vec![
        Line::from(format!("Device: {}", live.device().product_name)),
        Line::from(format!("EC Firmware: {firmware}")),
        Line::from(format!(
            "Webcam: {}",
            on_off_text(snapshot.and_then(|s| s.webcam))
        )),
        Line::from(format!(
            "Webcam Block: {}",
            on_off_text(snapshot.and_then(|s| s.webcam_block))
        )),
        Line::from(format!(
            "Keyboard Backlight: {}",
            backlight_text(snapshot.and_then(|s| s.keyboard_backlight))
        )),
    ];
    let mut mode_line = vec![
        Span::styled("Mode: ", Style::default().fg(theme.muted)),
        Span::styled(
            support_mode_text(live.mode()).to_owned(),
            support_mode_style(live.mode(), theme),
        ),
    ];
    if let SupportMode::ReadOnly(reason) = live.mode() {
        mode_line.push(Span::raw(format!(" ({})", read_only_reason_text(reason))));
    }
    lines.push(Line::from(mode_line));
    if let Some(error) = live.snapshot_error() {
        lines.push(Line::styled(
            error.to_string(),
            Style::default().fg(theme.danger),
        ));
    }
    let style = Style::default().fg(theme.foreground).bg(theme.surface);
    frame.render_widget(Paragraph::new(Text::from(lines)).style(style), inner);
}

fn temp_frac(value: Option<crate::hardware::TemperatureCelsius>) -> f64 {
    value.map(|v| f64::from(v.get()) / 100.0).unwrap_or(0.0)
}

fn fan_frac(value: Option<crate::hardware::FanPercent>) -> f64 {
    value.map(|v| f64::from(v.get()) / 100.0).unwrap_or(0.0)
}

fn charge_frac(value: Option<u8>) -> f64 {
    value.map(|v| f64::from(v) / 100.0).unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    use crate::app::{AppAction, AppState, LiveHardware, Screen};
    use crate::hardware::{
        BackendError, BatteryStatus, Capabilities, DeviceInfo, EcBackend, FanMode, FanPercent,
        HardwareSnapshot, ReadOnlyReason, ShiftMode, SupportMode, TemperatureCelsius,
    };
    use crate::monitoring::SnapshotHistory;

    use super::{render_dashboard, render_dashboard_with_theme};

    struct CountingBackend {
        script: RefCell<VecDeque<Result<HardwareSnapshot, BackendError>>>,
        snapshot_calls: Rc<Cell<usize>>,
    }

    impl CountingBackend {
        fn scripted(script: Vec<Result<HardwareSnapshot, BackendError>>) -> Self {
            Self::counted(script, Rc::new(Cell::new(0)))
        }

        fn counted(
            script: Vec<Result<HardwareSnapshot, BackendError>>,
            snapshot_calls: Rc<Cell<usize>>,
        ) -> Self {
            Self {
                script: RefCell::new(script.into()),
                snapshot_calls,
            }
        }
    }

    impl EcBackend for CountingBackend {
        fn detect_device(&self) -> Result<DeviceInfo, BackendError> {
            panic!("dashboard rendering must not detect device identity");
        }

        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            panic!("dashboard rendering must not discover capabilities");
        }

        fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
            self.snapshot_calls.set(self.snapshot_calls.get() + 1);
            self.script
                .borrow_mut()
                .pop_front()
                .expect("script exhausted")
        }
    }

    fn device() -> DeviceInfo {
        DeviceInfo {
            manufacturer: "MSI".to_owned(),
            product_name: "Dashboard Test Fixture".to_owned(),
            board_name: None,
            bios_version: None,
            ec_firmware_version: None,
        }
    }

    fn healthy_snapshot() -> HardwareSnapshot {
        HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(63).ok(),
            gpu_temperature: TemperatureCelsius::try_from(51).ok(),
            cpu_fan: FanPercent::try_from(42).ok(),
            gpu_fan: FanPercent::try_from(31).ok(),
            fan_mode: FanMode::try_from("auto").ok(),
            shift_mode: ShiftMode::try_from("comfort").ok(),
            cooler_boost: Some(false),
            super_battery: Some(false),
            webcam: Some(true),
            webcam_block: Some(false),
            keyboard_backlight: Some(2),
            battery_start_threshold: Some(50),
            battery_end_threshold: Some(80),
            battery_percentage: Some(77),
            battery_status: Some(BatteryStatus::Charging),
            ac_connected: Some(true),
        }
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let mut lines = Vec::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }

    fn dashboard_text<B: EcBackend>(
        app: &AppState,
        live: &LiveHardware<B>,
        width: u16,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_dashboard(frame, frame.area(), app, live))
            .expect("dashboard draws");
        buffer_text(terminal.backend().buffer())
    }

    fn healthy_live() -> (AppState, LiveHardware<CountingBackend>, Rc<Cell<usize>>) {
        let calls = Rc::new(Cell::new(0));
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::counted(vec![Ok(healthy_snapshot())], Rc::clone(&calls)),
            SnapshotHistory::default(),
        );
        live.refresh();
        (AppState::default(), live, calls)
    }

    fn dashboard_in(area: Rect, app: &AppState, live: &LiveHardware<CountingBackend>) -> String {
        let backend = TestBackend::new(area.width.max(1), area.height.max(1));
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_dashboard(frame, area, app, live))
            .expect("dashboard draws");
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn renders_six_approved_card_headings() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        for heading in [
            "MEC CONTROL",
            "THERMALS",
            "COOLING",
            "POWER",
            "PERFORMANCE",
            "DEVICE / SYSTEM",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
    }

    #[test]
    fn renders_mec_product_title() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("MEC"));
    }

    #[test]
    fn renders_menu_identity_and_all_nine_rows() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("Yousef Osama"));
        assert!(text.contains("github.com/YousefE1bana"));
        for (index, label) in [
            "Dashboard",
            "Performance",
            "Fans",
            "Battery",
            "Devices",
            "Profiles",
            "Diagnostics",
            "Settings",
            "Exit",
        ]
        .iter()
        .enumerate()
        {
            assert!(text.contains(label), "{label:?} missing");
            assert!(text.contains(&format!("[{}]", index + 1)), "digit missing");
        }
    }

    #[test]
    fn renders_top_strip_with_real_state() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("Dashboard Test Fixture"));
        assert!(text.contains("READY"));
        assert!(!text.contains("READ-ONLY"));
        assert!(text.contains("LIVE"));
        assert!(text.contains("EC N/A"));
    }

    #[test]
    fn renders_ec_firmware_when_known() {
        let (app, _, calls) = healthy_live();
        let mut info = device();
        info.ec_firmware_version = Some("16R6EMS1.107".to_owned());
        let mut live = LiveHardware::new(
            info,
            SupportMode::Ready,
            CountingBackend::counted(vec![Ok(healthy_snapshot())], Rc::clone(&calls)),
            SnapshotHistory::default(),
        );
        live.refresh();
        assert!(dashboard_text(&app, &live, 160, 50).contains("EC 16R6EMS1.107"));
    }

    #[test]
    fn renders_footer_shortcuts() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("[1-9]"));
        assert!(text.contains("Help"));
        assert!(text.contains("Quit"));
    }

    #[test]
    fn renders_device_product_name() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Dashboard Test Fixture"));
    }

    #[test]
    fn renders_ready_mode() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("READY"));
        assert!(!text.contains("READ-ONLY"));
    }

    #[test]
    fn renders_cpu_temperature() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("63°C"));
    }

    #[test]
    fn renders_gpu_temperature() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("51°C"));
    }

    #[test]
    fn renders_cpu_fan() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("42%"));
    }

    #[test]
    fn renders_gpu_fan() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("31%"));
    }

    #[test]
    fn output_contains_no_rpm() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(!text.contains("RPM"));
        assert!(!text.contains("rpm"));
    }

    #[test]
    fn renders_shift_mode() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("comfort"));
    }

    #[test]
    fn renders_fan_mode() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("auto"));
    }

    #[test]
    fn renders_cooler_boost_off() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Cooler Boost: Off"));
    }

    #[test]
    fn renders_cooler_boost_on() {
        let calls = Rc::new(Cell::new(0));
        let mut snapshot = healthy_snapshot();
        snapshot.cooler_boost = Some(true);
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::counted(vec![Ok(snapshot)], Rc::clone(&calls)),
            SnapshotHistory::default(),
        );
        live.refresh();
        assert!(dashboard_text(&AppState::default(), &live, 160, 50).contains("Cooler Boost: On"));
    }

    #[test]
    fn renders_super_battery_off() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Super Battery: Off"));
    }

    #[test]
    fn renders_battery_percentage() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("77%"));
    }

    #[test]
    fn renders_charging_state() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Charging"));
    }

    #[test]
    fn renders_ac_connected() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Connected"));
    }

    #[test]
    fn renders_start_threshold() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("50%"));
    }

    #[test]
    fn renders_end_threshold() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("80%"));
    }

    #[test]
    fn renders_webcam() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Webcam: On"));
    }

    #[test]
    fn renders_webcam_block() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Webcam Block: Off"));
    }

    #[test]
    fn renders_keyboard_backlight() {
        let (app, live, _) = healthy_live();
        assert!(dashboard_text(&app, &live, 160, 50).contains("Keyboard Backlight: 2"));
    }

    #[test]
    fn absent_telemetry_renders_na() {
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(HardwareSnapshot::default())]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("N/A"));
        assert!(text.contains("CPU Temperature: N/A"));
    }

    #[test]
    fn partial_snapshot_remains_live_not_degraded() {
        let snapshot = HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(63).ok(),
            ..Default::default()
        };
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(snapshot)]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("LIVE"));
        assert!(!text.contains("DEGRADED"));
        assert!(text.contains("63°C"));
        assert!(text.contains("N/A"));
    }

    #[test]
    fn waiting_state_renders_waiting() {
        let app = AppState::default();
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![]),
            SnapshotHistory::default(),
        );
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("WAITING"));
        assert!(!text.contains("DEGRADED"));
    }

    #[test]
    fn waiting_fields_render_na() {
        let app = AppState::default();
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![]),
            SnapshotHistory::default(),
        );
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("CPU Temperature: N/A"));
        assert!(text.contains("Charge: N/A"));
    }

    fn degraded_live() -> (AppState, LiveHardware<CountingBackend>) {
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![
                Ok(healthy_snapshot()),
                Err(BackendError::InvalidData("sensor offline".to_owned())),
            ]),
            SnapshotHistory::default(),
        );
        live.refresh();
        live.refresh();
        (AppState::default(), live)
    }

    #[test]
    fn degraded_renders_degraded_status() {
        let (app, live) = degraded_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("DEGRADED"));
        assert!(!text.contains("LIVE"));
    }

    #[test]
    fn degraded_renders_readable_backend_error() {
        let (app, live) = degraded_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("sensor offline"));
    }

    #[test]
    fn degraded_telemetry_renders_na() {
        let (app, live) = degraded_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("CPU Temperature: N/A"));
        assert!(text.contains("Charge: N/A"));
    }

    #[test]
    fn degraded_hides_stale_history_value() {
        let (app, live) = degraded_live();
        assert_eq!(live.history().len(), 1);
        let text = dashboard_text(&app, &live, 160, 50);
        for line in text.lines() {
            if line.contains("63°C") {
                assert!(line.contains("History"), "{line:?}");
            }
        }
        assert!(text.lines().any(|line| line.contains("63°C")));
        assert!(!text.contains("77%"));
    }

    #[test]
    fn recovery_renders_live_again() {
        let mut snapshot = healthy_snapshot();
        snapshot.cpu_temperature = TemperatureCelsius::try_from(61).ok();
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![
                Ok(healthy_snapshot()),
                Err(BackendError::Unavailable),
                Ok(snapshot),
            ]),
            SnapshotHistory::default(),
        );
        live.refresh();
        live.refresh();
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("LIVE"));
        assert!(!text.contains("DEGRADED"));
    }

    #[test]
    fn recovery_displays_recovered_value() {
        let mut snapshot = healthy_snapshot();
        snapshot.cpu_temperature = TemperatureCelsius::try_from(61).ok();
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![
                Ok(healthy_snapshot()),
                Err(BackendError::Unavailable),
                Ok(snapshot),
            ]),
            SnapshotHistory::default(),
        );
        live.refresh();
        live.refresh();
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("61°C"));
        assert!(!text.contains("63°C"));
    }

    #[test]
    fn read_only_renders_mode_and_reason() {
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface),
            CountingBackend::scripted(vec![Ok(healthy_snapshot())]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("READ-ONLY"));
        assert!(text.contains("Inconsistent hardware interface"));
        assert!(text.contains("63°C"));
    }

    #[test]
    fn read_only_reasons_map_to_stable_text() {
        let cases = [
            (ReadOnlyReason::NonMsiHardware, "Non-MSI hardware"),
            (
                ReadOnlyReason::UnverifiedHardwareIdentity,
                "Unverified hardware identity",
            ),
            (ReadOnlyReason::MsiEcUnavailable, "msi-ec unavailable"),
            (ReadOnlyReason::MsiEcUnreadable, "msi-ec unreadable"),
            (
                ReadOnlyReason::InconsistentInterface,
                "Inconsistent hardware interface",
            ),
        ];
        for (reason, expected) in cases {
            let app = AppState::default();
            let mut live = LiveHardware::new(
                device(),
                SupportMode::ReadOnly(reason),
                CountingBackend::scripted(vec![Ok(healthy_snapshot())]),
                SnapshotHistory::default(),
            );
            live.refresh();
            let text = dashboard_text(&app, &live, 160, 50);
            assert!(text.contains("READ-ONLY"), "{expected}");
            assert!(text.contains(expected), "{expected}");
        }
    }

    #[test]
    fn rendering_performs_zero_backend_calls() {
        let (app, live, calls) = healthy_live();
        assert_eq!(calls.get(), 1);
        let _ = dashboard_text(&app, &live, 160, 50);
        let _ = dashboard_text(&app, &live, 160, 50);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn small_area_renders_fallback_without_panic() {
        let (app, live, _) = healthy_live();
        let text = dashboard_in(Rect::new(0, 0, 20, 8), &app, &live);
        assert!(text.contains("Terminal too small"));
    }

    #[test]
    fn zero_area_renders_without_panic() {
        let (app, live, _) = healthy_live();
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_dashboard(frame, Rect::new(0, 0, 0, 0), &app, &live))
            .expect("zero-area dashboard draws");
    }

    #[test]
    fn compact_width_stacks_cards_without_panic() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 100, 30);
        assert!(text.contains("MEC CONTROL"));
        assert!(text.contains("63°C"));
    }

    #[test]
    fn tiny_width_renders_without_panic() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 80, 24);
        assert!(text.contains("MEC"));
    }

    #[test]
    fn full_shows_temperature_history() {
        let (app, live, _) = healthy_live();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("CPU Temp History: 63°C"));
        assert!(text.contains("GPU Temp History: 51°C"));
        assert!(text.contains("min 63 / max 63"));
        assert!(text.contains("min 51 / max 51"));
    }

    #[test]
    fn cpu_only_sensor_does_not_invent_gpu_graph() {
        let snapshot = HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(60).ok(),
            ..Default::default()
        };
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(snapshot)]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("CPU Temp History: 60°C"));
        assert!(text.contains("GPU Temp History: No history"));
    }

    #[test]
    fn gpu_only_sensor_does_not_invent_cpu_graph() {
        let snapshot = HardwareSnapshot {
            gpu_temperature: TemperatureCelsius::try_from(49).ok(),
            ..Default::default()
        };
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(snapshot)]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("GPU Temp History: 49°C"));
        assert!(text.contains("CPU Temp History: No history"));
    }

    #[test]
    fn empty_history_shows_no_history_state() {
        let app = AppState::default();
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![]),
            SnapshotHistory::default(),
        );
        let text = dashboard_text(&app, &live, 160, 50);
        assert!(text.contains("CPU Temp History: No history"));
        assert!(text.contains("GPU Temp History: No history"));
    }

    #[test]
    fn history_rendering_performs_zero_backend_calls() {
        let calls = Rc::new(Cell::new(0));
        let app = AppState::default();
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::counted(
                vec![
                    Ok(healthy_snapshot()),
                    Ok(healthy_snapshot()),
                    Ok(healthy_snapshot()),
                ],
                Rc::clone(&calls),
            ),
            SnapshotHistory::default(),
        );
        live.refresh();
        live.refresh();
        live.refresh();
        assert_eq!(calls.get(), 3);
        assert_eq!(live.history().len(), 3);
        let _ = dashboard_text(&app, &live, 160, 50);
        let _ = dashboard_text(&app, &live, 160, 50);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn active_menu_row_follows_current_screen() {
        use super::super::support::first_cell_style;
        use crate::tui::theme::Theme;
        let (_, live, _) = healthy_live();
        let mut app = AppState::default();
        app.apply(AppAction::GoTo(Screen::Fans));
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(160, 50, "Fans", |frame| {
            render_dashboard_with_theme(frame, frame.area(), &app, &live, &theme);
        })
        .expect("Fans row present");
        assert_eq!(foreground, theme.foreground);
    }

    #[test]
    fn inactive_menu_rows_stay_muted() {
        use super::super::support::first_cell_style;
        use crate::tui::theme::Theme;
        let (app, live, _) = healthy_live();
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(160, 50, "Profiles", |frame| {
            render_dashboard_with_theme(frame, frame.area(), &app, &live, &theme);
        })
        .expect("Profiles row present");
        assert_eq!(foreground, theme.muted);
    }

    #[test]
    fn card_titles_use_accent_role() {
        use super::super::support::first_cell_style;
        use crate::tui::theme::Theme;
        let (app, live, _) = healthy_live();
        let theme = Theme::default();
        for title in ["THERMALS", "COOLING", "POWER", "PERFORMANCE"] {
            let (foreground, _) = first_cell_style(160, 50, title, |frame| {
                render_dashboard_with_theme(frame, frame.area(), &app, &live, &theme);
            })
            .expect("card title present");
            assert_eq!(foreground, theme.accent, "{title}");
        }
    }

    #[test]
    fn meters_use_approved_cool_fill() {
        use super::super::support::first_cell_style;
        use crate::tui::theme::Theme;
        use ratatui::style::Color;
        let (app, live, _) = healthy_live();
        let theme = Theme::default();
        let (foreground, _) = first_cell_style(160, 50, "█", |frame| {
            render_dashboard_with_theme(frame, frame.area(), &app, &live, &theme);
        })
        .expect("meter fill present");
        assert_eq!(foreground, Color::Rgb(79, 159, 173));
    }

    #[test]
    fn clock_renders_eight_char_time() {
        let clock = super::clock();
        assert_eq!(clock.len(), 8);
        assert_eq!(clock.chars().filter(|c| *c == ':').count(), 2);
    }

    #[test]
    fn gf63_fixture_dashboard_uses_real_identity_without_rpm() {
        use std::path::PathBuf;

        use crate::hardware::{LinuxSysfsReader, SystemPaths};
        let paths = SystemPaths::new(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("gf63"),
        );
        let mut app = crate::tui::prepare_tui(paths, LinuxSysfsReader)
            .expect("gf63 fixture prepares without hardware");
        app.refresh();
        let backend = TestBackend::new(160, 50);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| {
                render_dashboard_with_theme(
                    frame,
                    frame.area(),
                    app.state(),
                    app.live(),
                    &app.theme(),
                );
            })
            .expect("fixture dashboard draws");
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("GF63 Thin 11UC"), "real device identity");
        assert!(text.contains("READY"), "real support verdict");
        for heading in [
            "MEC CONTROL",
            "THERMALS",
            "COOLING",
            "POWER",
            "PERFORMANCE",
            "DEVICE / SYSTEM",
        ] {
            assert!(text.contains(heading), "{heading:?} missing");
        }
        assert!(!text.contains("RPM"), "fan values never read as RPM");
        assert!(!text.contains("rpm"), "fan values never read as RPM");
        if std::env::var("MEC_DUMP_DASHBOARD").is_ok() {
            println!("{text}");
        }
    }
}
