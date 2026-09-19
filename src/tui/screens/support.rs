//! Shared TestBackend harnesses for secondary-screen rendering tests.
//!
//! Test-only: counting fake backend, healthy fixtures, and buffer helpers.
//! Rendering itself must perform zero backend calls.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::app::LiveHardware;
use crate::hardware::{
    BackendError, BacklightCapability, BatteryStatus, Capabilities, DeviceInfo, EcBackend, FanMode,
    FanPercent, HardwareSnapshot, ShiftMode, SupportMode, TemperatureCelsius,
};
use crate::monitoring::SnapshotHistory;

/// Fake backend that counts snapshots and panics on identity/capability
/// discovery, proving renderers never perform hardware transport.
pub(crate) struct CountingBackend {
    script: RefCell<VecDeque<Result<HardwareSnapshot, BackendError>>>,
    snapshot_calls: Rc<Cell<usize>>,
}

impl CountingBackend {
    pub(crate) fn scripted(script: Vec<Result<HardwareSnapshot, BackendError>>) -> Self {
        Self::counted(script, Rc::new(Cell::new(0)))
    }

    pub(crate) fn counted(
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
        panic!("screen rendering must not detect device identity");
    }

    fn capabilities(&self) -> Result<Capabilities, BackendError> {
        panic!("screen rendering must not discover capabilities");
    }

    fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
        self.snapshot_calls.set(self.snapshot_calls.get() + 1);
        self.script
            .borrow_mut()
            .pop_front()
            .expect("script exhausted")
    }
}

pub(crate) fn device() -> DeviceInfo {
    DeviceInfo {
        manufacturer: "MSI".to_owned(),
        product_name: "Secondary Screen Fixture".to_owned(),
        board_name: Some("MS-99XY".to_owned()),
        bios_version: Some("E99XYIMS.100".to_owned()),
        ec_firmware_version: Some("99XYEMS1.100".to_owned()),
    }
}

pub(crate) fn healthy_snapshot() -> HardwareSnapshot {
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

pub(crate) fn full_capabilities() -> Capabilities {
    Capabilities {
        cpu_temperature: true,
        gpu_temperature: false,
        cpu_fan: true,
        gpu_fan: true,
        fan_modes: ["auto", "silent", "future-mode"]
            .into_iter()
            .map(|name| FanMode::try_from(name).unwrap())
            .collect(),
        shift_modes: ["comfort", "sport"]
            .into_iter()
            .map(|name| ShiftMode::try_from(name).unwrap())
            .collect(),
        cooler_boost: true,
        super_battery: false,
        webcam: true,
        webcam_block: false,
        fn_key: true,
        win_key: false,
        keyboard_backlight: Some(BacklightCapability { max_brightness: 3 }),
        battery_thresholds: true,
    }
}

/// Builds refreshed live state over `script` in `mode`, returning the
/// state plus its backend call counter.
pub(crate) fn live_for(
    script: Vec<Result<HardwareSnapshot, BackendError>>,
    mode: SupportMode,
    refreshes: usize,
) -> (LiveHardware<CountingBackend>, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let mut live = LiveHardware::new(
        device(),
        mode,
        CountingBackend::counted(script, Rc::clone(&calls)),
        SnapshotHistory::default(),
    );
    for _ in 0..refreshes {
        live.refresh();
    }
    (live, calls)
}

pub(crate) fn buffer_text(buffer: &Buffer) -> String {
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

/// Renders `draw` into a deterministic `width`x`height` buffer and returns
/// its visible text.
pub(crate) fn screen_text(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    with_buffer(width, height, draw, buffer_text)
}

/// Style of the first cell where `needle` starts, if present. Compares
/// buffer symbols (not bytes) so Unicode borders never skew the lookup.
pub(crate) fn first_cell_style(
    width: u16,
    height: u16,
    needle: &str,
    draw: impl FnOnce(&mut Frame),
) -> Option<(Color, Modifier)> {
    with_buffer(width, height, draw, |buffer| {
        for y in 0..buffer.area.height {
            let mut text = String::new();
            let mut byte_cells = Vec::new();
            for x in 0..buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                for _ in 0..symbol.len() {
                    byte_cells.push(x);
                }
                text.push_str(symbol);
            }
            if let Some(byte) = text.find(needle) {
                let cell = &buffer[(byte_cells[byte], y)];
                return Some((cell.fg, cell.modifier));
            }
        }
        None
    })
}

/// Foreground and background of the first cell where `needle` starts, if
/// present. Proves themes actually paint rendered cells, not just structs.
pub(crate) fn first_cell_colors(
    width: u16,
    height: u16,
    needle: &str,
    draw: impl FnOnce(&mut Frame),
) -> Option<(Color, Color)> {
    with_buffer(width, height, draw, |buffer| {
        for y in 0..buffer.area.height {
            let mut text = String::new();
            let mut byte_cells = Vec::new();
            for x in 0..buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                for _ in 0..symbol.len() {
                    byte_cells.push(x);
                }
                text.push_str(symbol);
            }
            if let Some(byte) = text.find(needle) {
                let cell = &buffer[(byte_cells[byte], y)];
                return Some((cell.fg, cell.bg));
            }
        }
        None
    })
}

pub(crate) fn with_buffer<R>(
    width: u16,
    height: u16,
    draw: impl FnOnce(&mut Frame),
    inspect: impl FnOnce(&Buffer) -> R,
) -> R {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal constructs");
    terminal.draw(draw).expect("screen draws");
    inspect(terminal.backend().buffer())
}
