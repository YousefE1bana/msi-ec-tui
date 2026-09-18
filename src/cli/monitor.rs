//! Read-only continuous monitoring runtime: `mec monitor` orchestration.
//!
//! Identity and support mode are collected once; only snapshots repeat.
//! Output goes through a caller-supplied writer for testability.

use std::io::Write;
use std::sync::mpsc;

use thiserror::Error;

use crate::hardware::{
    BackendError, EcBackend, LinuxSysfsReader, MsiEcBackend, SupportEvaluator, SupportMode,
    SystemPaths,
};
use crate::monitoring::{MonitorEngine, PollInterval, SnapshotHistory};

/// Failures that terminate monitoring with a non-zero exit.
#[derive(Debug, Error)]
pub enum MonitorError {
    /// Hardware collection failed (fail-fast; Task 6 owns degradation).
    #[error(transparent)]
    Collection(#[from] BackendError),
    /// The Ctrl+C handler could not be installed.
    #[error("failed to install Ctrl+C handler: {0}")]
    Signal(ctrlc::Error),
    /// Monitor output could not be written.
    #[error("failed to write monitor output: {0}")]
    Output(#[from] std::io::Error),
}

/// Runs the monitor loop until Ctrl+C or a collection error.
///
/// Prints the header, renders the first sample immediately, then samples
/// every interval. Ctrl+C is a clean shutdown returning `Ok`.
pub fn run_monitor(
    paths: SystemPaths,
    interval: PollInterval,
    out: &mut impl Write,
) -> Result<(), MonitorError> {
    let reader = LinuxSysfsReader;
    let mode = SupportEvaluator::new(paths.clone(), reader).evaluate();
    let backend = MsiEcBackend::new(paths, reader);
    let device = backend.detect_device()?;
    let mut engine = MonitorEngine::new(backend, SnapshotHistory::default());

    // Minimal handler: only wakes the loop. No I/O, no collection here.
    let (stop_tx, stop_rx) = mpsc::channel();
    ctrlc::set_handler(move || {
        let _ = stop_tx.send(());
    })
    .map_err(MonitorError::Signal)?;

    writeln!(out, "MEC Monitor")?;
    writeln!(out)?;
    writeln!(out, "Device: {}", device.product_name)?;
    writeln!(
        out,
        "Mode: {}",
        match mode {
            SupportMode::Ready => "READY",
            SupportMode::ReadOnly(_) => "READ-ONLY",
        }
    )?;
    writeln!(out, "Interval: {interval}")?;
    writeln!(out)?;
    out.flush()?;

    render_sample(engine.sample()?, out)?;
    out.flush()?;

    // `recv_timeout` wakes promptly on Ctrl+C instead of sleeping through
    // the whole interval. Any non-timeout means stop was requested.
    while let Err(mpsc::RecvTimeoutError::Timeout) = stop_rx.recv_timeout(interval.as_duration()) {
        render_sample(engine.sample()?, out)?;
        out.flush()?;
    }

    writeln!(out, "Monitoring stopped.")?;
    out.flush()?;
    Ok(())
}

fn render_sample(
    snapshot: &crate::hardware::HardwareSnapshot,
    out: &mut impl Write,
) -> Result<(), std::io::Error> {
    let cpu = snapshot
        .cpu_temperature
        .map(|value| format!("{}°C", value.get()))
        .unwrap_or_else(|| "N/A".to_owned());
    let gpu = snapshot
        .gpu_temperature
        .map(|value| format!("{}°C", value.get()))
        .unwrap_or_else(|| "N/A".to_owned());
    let cpu_fan = snapshot
        .cpu_fan
        .map(|value| format!("{}%", value.get()))
        .unwrap_or_else(|| "N/A".to_owned());
    let gpu_fan = snapshot
        .gpu_fan
        .map(|value| format!("{}%", value.get()))
        .unwrap_or_else(|| "N/A".to_owned());
    let battery = snapshot
        .battery_percentage
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "N/A".to_owned());
    let ac = match snapshot.ac_connected {
        Some(true) => "Connected",
        Some(false) => "Disconnected",
        None => "N/A",
    };
    writeln!(
        out,
        "CPU: {cpu} | GPU: {gpu} | CPU Fan: {cpu_fan} | GPU Fan: {gpu_fan} | Battery: {battery} | AC: {ac}"
    )
}
