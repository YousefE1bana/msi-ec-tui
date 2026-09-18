//! Live read-only hardware state for the TUI application core.
//!
//! [`LiveHardware`] pairs already-established startup metadata with a
//! [`MonitorEngine`] so later screens can render current telemetry without
//! ever presenting stale samples as live data.

use crate::hardware::{BackendError, DeviceInfo, EcBackend, HardwareSnapshot, SupportMode};
use crate::monitoring::{MonitorEngine, SnapshotHistory};

/// Application-level live telemetry over a generic [`EcBackend`].
///
/// Startup metadata is supplied by the future orchestration layer, so
/// construction is side-effect free: no device detection, no capability
/// discovery, no sampling. Only [`LiveHardware::refresh`] samples, and it
/// delegates entirely to [`MonitorEngine::sample`].
pub struct LiveHardware<B> {
    device: DeviceInfo,
    mode: SupportMode,
    engine: MonitorEngine<B>,
    snapshot_error: Option<BackendError>,
}

impl<B> LiveHardware<B>
where
    B: EcBackend,
{
    /// Pairs startup metadata with a backend and history without sampling.
    pub fn new(
        device: DeviceInfo,
        mode: SupportMode,
        backend: B,
        history: SnapshotHistory,
    ) -> Self {
        Self {
            device,
            mode,
            engine: MonitorEngine::new(backend, history),
            snapshot_error: None,
        }
    }

    /// Device identity established at startup.
    pub fn device(&self) -> &DeviceInfo {
        &self.device
    }

    /// Support verdict established at startup.
    pub fn mode(&self) -> &SupportMode {
        &self.mode
    }

    /// The most recent successful sample, or `None` when the latest attempt
    /// failed. Older history entries are never presented as current.
    pub fn current_snapshot(&self) -> Option<&HardwareSnapshot> {
        if self.snapshot_error.is_some() {
            None
        } else {
            self.engine.history().latest()
        }
    }

    /// Collected successful samples, oldest first, for future graphs.
    pub fn history(&self) -> &SnapshotHistory {
        self.engine.history()
    }

    /// The latest sampling failure, if the current state is degraded.
    pub fn snapshot_error(&self) -> Option<&BackendError> {
        self.snapshot_error.as_ref()
    }

    /// Whether the most recent sampling attempt failed.
    pub fn is_degraded(&self) -> bool {
        self.snapshot_error.is_some()
    }

    /// Samples once through the engine. Success clears the error; failure
    /// preserves the exact error and pushes nothing.
    pub fn refresh(&mut self) {
        match self.engine.sample() {
            Ok(_) => self.snapshot_error = None,
            Err(error) => self.snapshot_error = Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::num::NonZeroUsize;
    use std::rc::Rc;

    use crate::hardware::{
        BackendError, Capabilities, DeviceInfo, EcBackend, HardwareSnapshot, SupportMode,
        TemperatureCelsius,
    };
    use crate::monitoring::SnapshotHistory;

    use super::LiveHardware;

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
            panic!("live refresh must not detect device identity");
        }

        fn capabilities(&self) -> Result<Capabilities, BackendError> {
            panic!("live refresh must not discover capabilities");
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
            product_name: "TUI Test Fixture".to_owned(),
            board_name: None,
            bios_version: None,
            ec_firmware_version: None,
        }
    }

    fn temperature(celsius: u8) -> HardwareSnapshot {
        HardwareSnapshot {
            cpu_temperature: TemperatureCelsius::try_from(celsius).ok(),
            ..Default::default()
        }
    }

    fn live_with(
        script: Vec<Result<HardwareSnapshot, BackendError>>,
    ) -> (LiveHardware<ScriptedBackend>, Rc<Cell<usize>>) {
        let calls = Rc::new(Cell::new(0));
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            ScriptedBackend::with_counter(script, Rc::clone(&calls)),
            SnapshotHistory::default(),
        );
        (live, calls)
    }

    #[test]
    fn construction_performs_zero_backend_calls() {
        let (_, calls) = live_with(vec![Ok(temperature(60))]);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn supplied_device_is_preserved() {
        let (live, _) = live_with(vec![]);
        assert_eq!(live.device().product_name, "TUI Test Fixture");
        assert_eq!(live.device().manufacturer, "MSI");
    }

    #[test]
    fn supplied_mode_is_preserved() {
        let (live, _) = live_with(vec![]);
        assert_eq!(live.mode(), &SupportMode::Ready);
    }

    #[test]
    fn new_state_has_no_current_snapshot() {
        let (live, _) = live_with(vec![]);
        assert_eq!(live.current_snapshot(), None);
    }

    #[test]
    fn new_state_is_not_degraded() {
        let (live, _) = live_with(vec![]);
        assert!(!live.is_degraded());
        assert_eq!(live.snapshot_error(), None);
    }

    #[test]
    fn first_successful_refresh_stores_one_snapshot() {
        let (mut live, _) = live_with(vec![Ok(temperature(60))]);
        live.refresh();
        assert_eq!(live.history().len(), 1);
    }

    #[test]
    fn successful_refresh_exposes_current_snapshot() {
        let (mut live, _) = live_with(vec![Ok(temperature(60))]);
        live.refresh();
        assert_eq!(
            live.current_snapshot().unwrap().cpu_temperature,
            temperature(60).cpu_temperature
        );
    }

    #[test]
    fn successful_refresh_has_no_snapshot_error() {
        let (mut live, _) = live_with(vec![Ok(temperature(60))]);
        live.refresh();
        assert_eq!(live.snapshot_error(), None);
        assert!(!live.is_degraded());
    }

    #[test]
    fn first_failed_refresh_stores_no_snapshot() {
        let (mut live, _) = live_with(vec![Err(BackendError::Unavailable)]);
        live.refresh();
        assert_eq!(live.history().len(), 0);
        assert_eq!(live.current_snapshot(), None);
    }

    #[test]
    fn first_failed_refresh_becomes_degraded() {
        let (mut live, _) = live_with(vec![Err(BackendError::Unavailable)]);
        live.refresh();
        assert!(live.is_degraded());
    }

    #[test]
    fn failed_refresh_preserves_exact_backend_error() {
        let (mut live, _) = live_with(vec![Err(BackendError::InvalidData("boom".to_owned()))]);
        live.refresh();
        assert_eq!(
            live.snapshot_error(),
            Some(&BackendError::InvalidData("boom".to_owned()))
        );
    }

    #[test]
    fn failure_after_success_leaves_history_unchanged() {
        let (mut live, _) = live_with(vec![Ok(temperature(60)), Err(BackendError::Unavailable)]);
        live.refresh();
        live.refresh();
        assert_eq!(live.history().len(), 1);
    }

    #[test]
    fn failure_after_success_hides_stale_current_snapshot() {
        let (mut live, _) = live_with(vec![Ok(temperature(60)), Err(BackendError::Unavailable)]);
        live.refresh();
        assert!(live.current_snapshot().is_some());
        live.refresh();
        assert_eq!(live.current_snapshot(), None);
        assert!(live.is_degraded());
    }

    #[test]
    fn success_after_failure_clears_degraded_state() {
        let (mut live, _) = live_with(vec![Err(BackendError::Unavailable), Ok(temperature(61))]);
        live.refresh();
        assert!(live.is_degraded());
        live.refresh();
        assert!(!live.is_degraded());
        assert_eq!(live.snapshot_error(), None);
    }

    #[test]
    fn success_after_failure_exposes_recovered_snapshot() {
        let (mut live, _) = live_with(vec![Err(BackendError::Unavailable), Ok(temperature(61))]);
        live.refresh();
        live.refresh();
        assert_eq!(
            live.current_snapshot().unwrap().cpu_temperature,
            temperature(61).cpu_temperature
        );
    }

    #[test]
    fn recovery_appends_only_successful_sample() {
        let (mut live, _) = live_with(vec![
            Ok(temperature(60)),
            Err(BackendError::Unavailable),
            Ok(temperature(61)),
        ]);
        live.refresh();
        live.refresh();
        live.refresh();
        assert_eq!(live.history().len(), 2);
    }

    #[test]
    fn repeated_failures_never_grow_history() {
        let (mut live, _) = live_with(vec![
            Err(BackendError::Unavailable),
            Err(BackendError::AccessDenied),
        ]);
        live.refresh();
        live.refresh();
        assert_eq!(live.history().len(), 0);
        assert!(live.is_degraded());
        assert_eq!(live.snapshot_error(), Some(&BackendError::AccessDenied));
    }

    #[test]
    fn bounded_history_remains_respected() {
        let live = LiveHardware::new(
            device(),
            SupportMode::Ready,
            ScriptedBackend::new(vec![
                Ok(temperature(60)),
                Ok(temperature(61)),
                Ok(temperature(62)),
            ]),
            SnapshotHistory::new(NonZeroUsize::new(2).unwrap()),
        );
        let mut live = live;
        live.refresh();
        live.refresh();
        live.refresh();
        assert_eq!(live.history().len(), 2);
        let temperatures: Vec<u8> = live
            .history()
            .iter()
            .map(|snapshot| snapshot.cpu_temperature.unwrap().get())
            .collect();
        assert_eq!(temperatures, vec![61, 62]);
    }

    #[test]
    fn refresh_calls_backend_snapshot_exactly_once_per_refresh() {
        let (mut live, calls) = live_with(vec![Ok(temperature(60)), Ok(temperature(61))]);
        live.refresh();
        assert_eq!(calls.get(), 1);
        live.refresh();
        assert_eq!(calls.get(), 2);
    }
}
