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

use super::signal::subscribe_stop;

/// Failures that terminate monitoring with a non-zero exit.
#[derive(Debug, Error)]
pub enum MonitorError {
    /// Hardware collection failed (fail-fast; Task 6 owns degradation).
    #[error(transparent)]
    Collection(#[from] BackendError),
    /// The one-time process Ctrl+C handler could not be installed.
    #[error("failed to install Ctrl+C handler: {0}")]
    Signal(String),
    /// Monitor output could not be written.
    #[error("failed to write monitor output: {0}")]
    Output(#[from] std::io::Error),
}

/// Runs the monitor loop until Ctrl+C.
///
/// Prints the header, renders the first sample immediately, then samples
/// every interval. A failed sample enters a degraded state (one stderr
/// warning, no telemetry, nothing stored) and retries next interval; a
/// later success recovers with one stderr notice. Ctrl+C is a clean
/// shutdown returning `Ok`, even while degraded.
pub fn run_monitor(
    paths: SystemPaths,
    interval: PollInterval,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<(), MonitorError> {
    let reader = LinuxSysfsReader;
    let mode = SupportEvaluator::new(paths.clone(), reader).evaluate();
    let backend = MsiEcBackend::new(paths, reader);
    let device = backend.detect_device()?;
    let mut engine = MonitorEngine::new(backend, SnapshotHistory::default());

    // One process-global handler fans out to this session's fresh
    // receiver; repeated in-process sessions stay safe. The handler only
    // wakes the loop: no I/O, no collection there.
    let stop_rx = subscribe_stop()?;

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

    let mut degraded = false;
    sample_once(&mut engine, &mut degraded, out, err)?;

    // `recv_timeout` wakes promptly on Ctrl+C instead of sleeping through
    // the whole interval. Any non-timeout means stop was requested.
    while let Err(mpsc::RecvTimeoutError::Timeout) = stop_rx.recv_timeout(interval.as_duration()) {
        sample_once(&mut engine, &mut degraded, out, err)?;
    }

    writeln!(out, "Monitoring stopped.")?;
    out.flush()?;
    Ok(())
}

/// Attempts one sample: renders successes, tracks degraded transitions
/// with single-shot stderr notices, and never stores failed samples.
/// Only I/O failures propagate; collection errors become state.
fn sample_once<B>(
    engine: &mut MonitorEngine<B>,
    degraded: &mut bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<(), MonitorError>
where
    B: EcBackend,
{
    match engine.sample() {
        Ok(snapshot) => {
            if *degraded {
                writeln!(err, "MEC monitor recovered.")?;
                err.flush()?;
                *degraded = false;
            }
            render_sample(snapshot, out)?;
            out.flush()?;
        }
        Err(error) => {
            if !*degraded {
                writeln!(err, "MEC monitor degraded: {error}")?;
                err.flush()?;
                *degraded = true;
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{Capabilities, DeviceInfo, HardwareSnapshot, TemperatureCelsius};
    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct ScriptedBackend {
        script: RefCell<VecDeque<Result<HardwareSnapshot, BackendError>>>,
    }

    impl ScriptedBackend {
        fn new(script: Vec<Result<HardwareSnapshot, BackendError>>) -> Self {
            Self {
                script: RefCell::new(script.into()),
            }
        }
    }

    impl EcBackend for ScriptedBackend {
        fn detect_device(&self) -> Result<DeviceInfo, BackendError> {
            panic!("transition tests must not detect device identity");
        }

        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            panic!("transition tests must not discover capabilities");
        }

        fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
            self.script
                .borrow_mut()
                .pop_front()
                .expect("script exhausted")
        }
    }

    fn temperature_snapshot(celsius: u8) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(celsius).ok(),
            ..Default::default()
        }
    }

    fn harness(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
    ) -> (MonitorEngine<ScriptedBackend>, bool, Vec<u8>, Vec<u8>) {
        let engine = MonitorEngine::new(ScriptedBackend::new(script), SnapshotHistory::default());
        (engine, false, Vec::new(), Vec::new())
    }

    #[test]
    fn healthy_sample_renders_normally() {
        let (mut engine, mut degraded, mut out, mut err) =
            harness(vec![Ok(temperature_snapshot(60))]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        assert!(!degraded);
        assert!(String::from_utf8(out).unwrap().contains("CPU: 60°C"));
        assert!(err.is_empty());
    }

    #[test]
    fn first_error_enters_degraded_state() {
        let (mut engine, mut degraded, mut out, mut err) =
            harness(vec![Err(BackendError::Unavailable)]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        assert!(degraded);
    }

    #[test]
    fn first_error_writes_exactly_one_warning() {
        let (mut engine, mut degraded, mut out, mut err) =
            harness(vec![Err(BackendError::Unavailable)]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.matches("MEC monitor degraded:").count(), 1);
    }

    #[test]
    fn first_error_adds_no_history() {
        let (mut engine, mut degraded, mut out, mut err) =
            harness(vec![Err(BackendError::Unavailable)]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        assert!(out.is_empty());
        assert_eq!(engine.history().len(), 0);
    }

    #[test]
    fn repeated_error_does_not_duplicate_warning() {
        let (mut engine, mut degraded, mut out, mut err) = harness(vec![
            Err(BackendError::Unavailable),
            Err(BackendError::Unavailable),
        ]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.matches("MEC monitor degraded:").count(), 1);
        assert!(out.is_empty());
    }

    #[test]
    fn recovery_writes_recovered_message() {
        let (mut engine, mut degraded, mut out, mut err) = harness(vec![
            Err(BackendError::Unavailable),
            Ok(temperature_snapshot(61)),
        ]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        assert!(!degraded);
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.matches("MEC monitor recovered.").count(), 1);
    }

    #[test]
    fn recovery_sample_is_rendered_and_stored() {
        let (mut engine, mut degraded, mut out, mut err) = harness(vec![
            Err(BackendError::Unavailable),
            Ok(temperature_snapshot(61)),
        ]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("CPU: 61°C"));
        assert_eq!(engine.history().len(), 1);
        assert_eq!(
            engine
                .history()
                .latest()
                .unwrap()
                .cpu_temperature
                .unwrap()
                .get(),
            61
        );
    }

    #[test]
    fn second_healthy_sample_repeats_no_recovery_message() {
        let (mut engine, mut degraded, mut out, mut err) = harness(vec![
            Err(BackendError::Unavailable),
            Ok(temperature_snapshot(61)),
            Ok(temperature_snapshot(62)),
        ]);
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.matches("MEC monitor recovered.").count(), 1);
        assert!(String::from_utf8(out).unwrap().contains("CPU: 62°C"));
    }

    #[test]
    fn later_failure_after_recovery_warns_once_again() {
        let (mut engine, mut degraded, mut out, mut err) = harness(vec![
            Err(BackendError::Unavailable),
            Ok(temperature_snapshot(61)),
            Err(BackendError::Unavailable),
            Err(BackendError::Unavailable),
        ]);
        for _ in 0..4 {
            sample_once(&mut engine, &mut degraded, &mut out, &mut err).unwrap();
        }
        let err = String::from_utf8(err).unwrap();
        assert_eq!(err.matches("MEC monitor degraded:").count(), 2);
        assert!(degraded);
    }
}
