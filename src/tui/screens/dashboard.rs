//! Read-only dashboard screen: current telemetry presentation.
//!
//! Renders [`LiveHardware::current_snapshot`] only, never stale history.
//! No sampling, no sysfs, no terminal lifecycle here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::LiveHardware;
use crate::hardware::{EcBackend, SupportMode};

use crate::tui::ui::{
    MIN_SCREEN_HEIGHT, MIN_SCREEN_WIDTH, battery_lines, device_lines, performance_lines,
    read_only_reason_text, render_compact, render_panel, support_mode_text, telemetry_state_text,
    thermals_lines,
};

/// Conservative fallback threshold: below this the dashboard cannot show
/// its panels honestly, so a compact message replaces it.
const MIN_DASHBOARD_WIDTH: u16 = MIN_SCREEN_WIDTH;
const MIN_DASHBOARD_HEIGHT: u16 = MIN_SCREEN_HEIGHT;

/// Renders the read-only dashboard into `area`.
///
/// Reads only already-sampled [`LiveHardware`] state and performs zero
/// backend calls. Absent values render `N/A`; a failed latest sample hides
/// older history values instead of presenting them as current.
pub fn render_dashboard<B: EcBackend>(frame: &mut Frame, area: Rect, live: &LiveHardware<B>) {
    if area.width < MIN_DASHBOARD_WIDTH || area.height < MIN_DASHBOARD_HEIGHT {
        render_compact(frame, area);
        return;
    }

    let outer = Block::default()
        .borders(Borders::ALL)
        .title(" MEC — MSI EC Control Center ");
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
    render_header(frame, rows[0], live);
    render_panels(frame, rows[1], live);

    frame.render_widget(Paragraph::new("1 Dashboard • ? Help • Q Quit"), rows[2]);
}

fn render_panels<B: EcBackend>(frame: &mut Frame, area: Rect, live: &LiveHardware<B>) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[0]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);
    let snapshot = live.current_snapshot();
    render_panel(frame, left[0], " THERMALS ", thermals_lines(snapshot));
    render_panel(frame, left[1], " BATTERY ", battery_lines(snapshot));
    render_panel(
        frame,
        right[0],
        " PERFORMANCE ",
        performance_lines(snapshot),
    );
    render_panel(frame, right[1], " DEVICE ", device_lines(snapshot));
}

fn render_header<B: EcBackend>(frame: &mut Frame, area: Rect, live: &LiveHardware<B>) {
    let mut mode_line = format!("Mode: {}", support_mode_text(live.mode()));
    if let SupportMode::ReadOnly(reason) = live.mode() {
        mode_line.push_str(&format!(" ({})", read_only_reason_text(reason)));
    }
    let mut lines = vec![
        Line::from(format!("Device: {}", live.device().product_name)),
        Line::from(mode_line),
        Line::from(format!(
            "Telemetry: {}",
            telemetry_state_text(live.is_degraded(), live.current_snapshot().is_some())
        )),
    ];
    if let Some(error) = live.snapshot_error() {
        lines.push(Line::from(error.to_string()));
    }
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
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

    use crate::app::LiveHardware;
    use crate::hardware::{
        BackendError, BatteryStatus, Capabilities, DeviceInfo, EcBackend, FanMode, FanPercent,
        HardwareSnapshot, ReadOnlyReason, ShiftMode, SupportMode, TemperatureCelsius,
    };
    use crate::monitoring::SnapshotHistory;

    use super::render_dashboard;

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

    fn dashboard_text<B: EcBackend>(live: &LiveHardware<B>, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_dashboard(frame, frame.area(), live))
            .expect("dashboard draws");
        buffer_text(terminal.backend().buffer())
    }

    fn healthy_live() -> (LiveHardware<CountingBackend>, Rc<Cell<usize>>) {
        let calls = Rc::new(Cell::new(0));
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::counted(vec![Ok(healthy_snapshot())], Rc::clone(&calls)),
            SnapshotHistory::default(),
        );
        live.refresh();
        (live, calls)
    }

    fn dashboard_in(area: Rect, live: &LiveHardware<CountingBackend>) -> String {
        let backend = TestBackend::new(area.width.max(1), area.height.max(1));
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_dashboard(frame, area, live))
            .expect("dashboard draws");
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn renders_mec_product_title() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("MEC"));
    }

    #[test]
    fn renders_device_product_name() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Dashboard Test Fixture"));
    }

    #[test]
    fn renders_ready_mode() {
        let (live, _) = healthy_live();
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("READY"));
        assert!(!text.contains("READ-ONLY"));
    }

    #[test]
    fn renders_cpu_temperature() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("63°C"));
    }

    #[test]
    fn renders_gpu_temperature() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("51°C"));
    }

    #[test]
    fn renders_cpu_fan() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("42%"));
    }

    #[test]
    fn renders_gpu_fan() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("31%"));
    }

    #[test]
    fn output_contains_no_rpm() {
        let (live, _) = healthy_live();
        assert!(!dashboard_text(&live, 100, 30).contains("RPM"));
    }

    #[test]
    fn renders_shift_mode() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("comfort"));
    }

    #[test]
    fn renders_fan_mode() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("auto"));
    }

    #[test]
    fn renders_cooler_boost_off() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Cooler Boost: Off"));
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
        assert!(dashboard_text(&live, 100, 30).contains("Cooler Boost: On"));
    }

    #[test]
    fn renders_super_battery_off() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Super Battery: Off"));
    }

    #[test]
    fn renders_battery_percentage() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("77%"));
    }

    #[test]
    fn renders_charging_state() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Charging"));
    }

    #[test]
    fn renders_ac_connected() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Connected"));
    }

    #[test]
    fn renders_start_threshold() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("50%"));
    }

    #[test]
    fn renders_end_threshold() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("80%"));
    }

    #[test]
    fn renders_webcam() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Webcam: On"));
    }

    #[test]
    fn renders_webcam_block() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Webcam Block: Off"));
    }

    #[test]
    fn renders_keyboard_backlight() {
        let (live, _) = healthy_live();
        assert!(dashboard_text(&live, 100, 30).contains("Keyboard Backlight: 2"));
    }

    #[test]
    fn absent_telemetry_renders_na() {
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(HardwareSnapshot::default())]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("N/A"));
        assert!(text.contains("CPU Temperature: N/A"));
    }

    #[test]
    fn partial_snapshot_remains_live_not_degraded() {
        let snapshot = HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(63).ok(),
            ..Default::default()
        };
        let mut live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![Ok(snapshot)]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("LIVE"));
        assert!(!text.contains("DEGRADED"));
        assert!(text.contains("63°C"));
        assert!(text.contains("N/A"));
    }

    #[test]
    fn waiting_state_renders_waiting() {
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![]),
            SnapshotHistory::default(),
        );
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("WAITING"));
        assert!(!text.contains("DEGRADED"));
    }

    #[test]
    fn waiting_fields_render_na() {
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            CountingBackend::scripted(vec![]),
            SnapshotHistory::default(),
        );
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("CPU Temperature: N/A"));
        assert!(text.contains("Charge: N/A"));
    }

    fn degraded_live() -> LiveHardware<CountingBackend> {
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
        live
    }

    #[test]
    fn degraded_renders_degraded_status() {
        let text = dashboard_text(&degraded_live(), 100, 30);
        assert!(text.contains("DEGRADED"));
        assert!(!text.contains("LIVE"));
    }

    #[test]
    fn degraded_renders_readable_backend_error() {
        let text = dashboard_text(&degraded_live(), 100, 30);
        assert!(text.contains("sensor offline"));
    }

    #[test]
    fn degraded_telemetry_renders_na() {
        let text = dashboard_text(&degraded_live(), 100, 30);
        assert!(text.contains("CPU Temperature: N/A"));
        assert!(text.contains("Charge: N/A"));
    }

    #[test]
    fn degraded_hides_stale_history_value() {
        let live = degraded_live();
        assert_eq!(live.history().len(), 1);
        let text = dashboard_text(&live, 100, 30);
        assert!(!text.contains("63°C"));
        assert!(!text.contains("77%"));
    }

    #[test]
    fn recovery_renders_live_again() {
        let mut snapshot = healthy_snapshot();
        snapshot.cpu_temperature = TemperatureCelsius::try_from(61).ok();
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
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("LIVE"));
        assert!(!text.contains("DEGRADED"));
    }

    #[test]
    fn recovery_displays_recovered_value() {
        let mut snapshot = healthy_snapshot();
        snapshot.cpu_temperature = TemperatureCelsius::try_from(61).ok();
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
        let text = dashboard_text(&live, 100, 30);
        assert!(text.contains("61°C"));
        assert!(!text.contains("63°C"));
    }

    #[test]
    fn read_only_renders_mode_and_reason() {
        let mut live = LiveHardware::new(
            device(),
            SupportMode::ReadOnly(ReadOnlyReason::InconsistentInterface),
            CountingBackend::scripted(vec![Ok(healthy_snapshot())]),
            SnapshotHistory::default(),
        );
        live.refresh();
        let text = dashboard_text(&live, 100, 30);
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
            let mut live = LiveHardware::new(
                device(),
                SupportMode::ReadOnly(reason),
                CountingBackend::scripted(vec![Ok(healthy_snapshot())]),
                SnapshotHistory::default(),
            );
            live.refresh();
            let text = dashboard_text(&live, 100, 30);
            assert!(text.contains("READ-ONLY"), "{expected}");
            assert!(text.contains(expected), "{expected}");
        }
    }

    #[test]
    fn rendering_performs_zero_backend_calls() {
        let (live, calls) = healthy_live();
        assert_eq!(calls.get(), 1);
        let _ = dashboard_text(&live, 100, 30);
        let _ = dashboard_text(&live, 100, 30);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn small_area_renders_fallback_without_panic() {
        let (live, _) = healthy_live();
        let text = dashboard_in(Rect::new(0, 0, 20, 8), &live);
        assert!(text.contains("Terminal too small"));
    }

    #[test]
    fn zero_area_renders_without_panic() {
        let (live, _) = healthy_live();
        let backend = TestBackend::new(10, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal constructs");
        terminal
            .draw(|frame| render_dashboard(frame, Rect::new(0, 0, 0, 0), &live))
            .expect("zero-area dashboard draws");
    }
}
