//! Reusable polling engine: collect snapshots through [`EcBackend`] into a
//! bounded [`SnapshotHistory`]. No timers, no signals, no presentation.

use crate::hardware::{BackendError, EcBackend, HardwareSnapshot};

use super::SnapshotHistory;

/// Collects owned snapshots into bounded history through an [`EcBackend`].
/// Knows nothing about backends, terminals, or signals.
pub struct MonitorEngine<B> {
    backend: B,
    history: SnapshotHistory,
}

impl<B> MonitorEngine<B>
where
    B: EcBackend,
{
    /// Creates an engine collecting through `backend` into `history`.
    pub fn new(backend: B, history: SnapshotHistory) -> Self {
        Self { backend, history }
    }

    /// Collects one snapshot, stores it, and returns the newest stored
    /// snapshot. On backend failure nothing is pushed and history is
    /// untouched.
    pub fn sample(&mut self) -> Result<&HardwareSnapshot, BackendError> {
        let snapshot = self.backend.snapshot()?;
        self.history.push(snapshot);
        Ok(self
            .history
            .latest()
            .expect("just-pushed snapshot is present"))
    }

    /// Collected samples, oldest first.
    pub fn history(&self) -> &SnapshotHistory {
        &self.history
    }

    /// Consumes the engine, returning the collected history.
    pub fn into_history(self) -> SnapshotHistory {
        self.history
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::num::NonZeroUsize;
    use std::rc::Rc;

    use super::*;
    use crate::hardware::{Capabilities, DeviceInfo};

    struct ScriptedBackend {
        script: RefCell<VecDeque<Result<HardwareSnapshot, BackendError>>>,
        snapshot_calls: Rc<Cell<usize>>,
    }

    impl ScriptedBackend {
        fn new(script: Vec<Result<HardwareSnapshot, BackendError>>) -> Self {
            Self::with_counter(script, Rc::new(Cell::new(0)))
        }

        fn with_counter(
            script: Vec<Result<HardwareSnapshot, BackendError>>,
            snapshot_calls: Rc<Cell<usize>>,
        ) -> Self {
            Self {
                script: RefCell::new(script.into()),
                snapshot_calls,
            }
        }
    }

    impl EcBackend for ScriptedBackend {
        fn detect_device(&self) -> Result<DeviceInfo, BackendError> {
            panic!("engine sampling must not detect device identity");
        }

        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            panic!("engine sampling must not discover capabilities");
        }

        fn snapshot(&self) -> Result<HardwareSnapshot, BackendError> {
            self.snapshot_calls.set(self.snapshot_calls.get() + 1);
            self.script
                .borrow_mut()
                .pop_front()
                .expect("script exhausted")
        }
    }

    fn temperature_snapshot(celsius: u8) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu_temperature: crate::hardware::TemperatureCelsius::try_from(celsius).ok(),
            ..Default::default()
        }
    }

    fn engine_with(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
    ) -> MonitorEngine<ScriptedBackend> {
        MonitorEngine::new(ScriptedBackend::new(script), SnapshotHistory::default())
    }

    fn engine_with_capacity(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
        capacity: usize,
    ) -> MonitorEngine<ScriptedBackend> {
        MonitorEngine::new(
            ScriptedBackend::new(script),
            SnapshotHistory::new(NonZeroUsize::new(capacity).unwrap()),
        )
    }

    #[test]
    fn new_engine_history_starts_empty() {
        let engine = engine_with(vec![Ok(temperature_snapshot(60))]);
        assert!(engine.history().is_empty());
        assert_eq!(engine.history().len(), 0);
    }

    #[test]
    fn successful_sample_calls_backend_snapshot_once() {
        let calls = Rc::new(Cell::new(0));
        let mut engine = MonitorEngine::new(
            ScriptedBackend::with_counter(vec![Ok(temperature_snapshot(60))], Rc::clone(&calls)),
            SnapshotHistory::default(),
        );
        let _ = engine.sample().unwrap();
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn successful_sample_is_stored() {
        let mut engine = engine_with(vec![Ok(temperature_snapshot(61))]);
        engine.sample().unwrap();
        assert_eq!(engine.history().len(), 1);
        assert_eq!(
            engine.history().latest().unwrap().cpu_temperature,
            temperature_snapshot(61).cpu_temperature
        );
    }

    #[test]
    fn sample_returns_newest_stored_snapshot() {
        let mut engine = engine_with(vec![
            Ok(temperature_snapshot(61)),
            Ok(temperature_snapshot(62)),
        ]);
        engine.sample().unwrap();
        let newest = engine.sample().unwrap();
        assert_eq!(
            newest.cpu_temperature,
            temperature_snapshot(62).cpu_temperature
        );
        assert_eq!(engine.history().len(), 2);
    }

    #[test]
    fn repeated_samples_preserve_order() {
        let mut engine = engine_with(vec![
            Ok(temperature_snapshot(61)),
            Ok(temperature_snapshot(62)),
            Ok(temperature_snapshot(63)),
        ]);
        for _ in 0..3 {
            engine.sample().unwrap();
        }
        let temperatures: Vec<u8> = engine
            .history()
            .iter()
            .map(|snapshot| snapshot.cpu_temperature.unwrap().get())
            .collect();
        assert_eq!(temperatures, vec![61, 62, 63]);
    }

    #[test]
    fn history_respects_configured_capacity() {
        let mut engine = engine_with_capacity(
            vec![
                Ok(temperature_snapshot(61)),
                Ok(temperature_snapshot(62)),
                Ok(temperature_snapshot(63)),
            ],
            2,
        );
        for _ in 0..3 {
            engine.sample().unwrap();
        }
        assert_eq!(engine.history().len(), 2);
    }

    #[test]
    fn capacity_overflow_evicts_oldest() {
        let mut engine = engine_with_capacity(
            vec![
                Ok(temperature_snapshot(61)),
                Ok(temperature_snapshot(62)),
                Ok(temperature_snapshot(63)),
            ],
            2,
        );
        for _ in 0..3 {
            engine.sample().unwrap();
        }
        let temperatures: Vec<u8> = engine
            .history()
            .iter()
            .map(|snapshot| snapshot.cpu_temperature.unwrap().get())
            .collect();
        assert_eq!(temperatures, vec![62, 63]);
    }

    #[test]
    fn backend_error_propagates() {
        let mut engine = engine_with(vec![Err(BackendError::Unavailable)]);
        let error = engine.sample().unwrap_err();
        assert_eq!(error, BackendError::Unavailable);
    }

    #[test]
    fn backend_error_pushes_no_history_entry() {
        let mut engine = engine_with(vec![
            Ok(temperature_snapshot(61)),
            Err(BackendError::Unavailable),
        ]);
        engine.sample().unwrap();
        let _ = engine.sample().unwrap_err();
        assert_eq!(engine.history().len(), 1);
        assert_eq!(
            engine.history().latest().unwrap().cpu_temperature,
            temperature_snapshot(61).cpu_temperature
        );
    }

    #[test]
    fn sampling_can_succeed_after_error() {
        let mut engine = engine_with(vec![
            Err(BackendError::Unavailable),
            Ok(temperature_snapshot(61)),
        ]);
        assert!(engine.sample().is_err());
        let newest = engine.sample().unwrap();
        assert_eq!(
            newest.cpu_temperature,
            temperature_snapshot(61).cpu_temperature
        );
        assert_eq!(engine.history().len(), 1);
    }

    #[test]
    fn into_history_returns_collected_samples() {
        let mut engine = engine_with(vec![
            Ok(temperature_snapshot(61)),
            Ok(temperature_snapshot(62)),
        ]);
        engine.sample().unwrap();
        engine.sample().unwrap();
        let history = engine.into_history();
        assert_eq!(history.len(), 2);
    }
}
