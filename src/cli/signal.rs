//! Process-global Ctrl+C distribution for monitor sessions.
//!
//! The `ctrlc` crate allows exactly one handler per process, so the handler
//! is installed once and fans notifications out to per-session subscribers.
//! Each `run_monitor` invocation subscribes a fresh receiver, making
//! repeated in-process sessions safe.

use std::sync::{Mutex, MutexGuard, OnceLock, mpsc};

use super::monitor::MonitorError;

/// Live per-session stop notifiers. The lock is held only to push, clone
/// lengths, or prune; never across I/O.
static SUBSCRIBERS: OnceLock<Mutex<Vec<mpsc::Sender<()>>>> = OnceLock::new();

/// One-time process handler installation outcome.
static HANDLER: OnceLock<Result<(), String>> = OnceLock::new();

fn subscribers() -> &'static Mutex<Vec<mpsc::Sender<()>>> {
    SUBSCRIBERS.get_or_init(|| Mutex::new(Vec::new()))
}

fn lock_subscribers() -> MutexGuard<'static, Vec<mpsc::Sender<()>>> {
    subscribers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Notifies every subscribed session, discarding senders whose receivers
/// went away. Minimal by design: safe to run as the signal callback.
fn notify_subscribers() {
    lock_subscribers().retain(|sender| sender.send(()).is_ok());
}

/// Ensures the process handler is installed exactly once, then subscribes
/// a fresh stop receiver for one monitor session.
pub(super) fn subscribe_stop() -> Result<mpsc::Receiver<()>, MonitorError> {
    if let Err(message) = HANDLER
        .get_or_init(|| ctrlc::set_handler(notify_subscribers).map_err(|error| error.to_string()))
    {
        return Err(MonitorError::Signal(message.clone()));
    }
    let (sender, receiver) = mpsc::channel();
    lock_subscribers().push(sender);
    Ok(receiver)
}

/// Number of currently registered session subscribers. Test-only helper
/// for proving stale entries are pruned.
#[cfg(test)]
fn live_subscriber_count() -> usize {
    lock_subscribers().len()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Serializes signal-mechanism tests: the subscriber registry is
    /// process-global, so tests must not notify concurrently.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock_serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Drops every subscriber created so far: receivers are test locals,
    /// so one broadcast prunes all of their senders from the registry.
    fn prune_stale() {
        notify_subscribers();
        assert_eq!(live_subscriber_count(), 0);
    }

    #[test]
    fn first_subscription_succeeds() {
        let _serial = lock_serial();
        prune_stale();
        assert!(subscribe_stop().is_ok());
    }

    #[test]
    fn second_session_subscription_succeeds_in_same_process() {
        let _serial = lock_serial();
        prune_stale();
        let first = subscribe_stop().unwrap();
        let second = subscribe_stop().unwrap();
        drop(first);
        drop(second);
    }

    #[test]
    fn dropped_subscription_does_not_break_later_ones() {
        let _serial = lock_serial();
        prune_stale();
        drop(subscribe_stop().unwrap());
        assert!(subscribe_stop().is_ok());
    }

    #[test]
    fn initialization_is_idempotent() {
        let _serial = lock_serial();
        prune_stale();
        for _ in 0..3 {
            assert!(subscribe_stop().is_ok());
        }
    }

    #[test]
    fn dead_subscribers_do_not_panic_and_are_pruned() {
        let _serial = lock_serial();
        prune_stale();
        let live = subscribe_stop().unwrap();
        drop(subscribe_stop().unwrap());
        notify_subscribers();
        assert!(live.try_recv().is_ok());
        assert_eq!(live_subscriber_count(), 1);
    }

    #[test]
    fn multiple_active_subscribers_coexist() {
        let _serial = lock_serial();
        prune_stale();
        let first = subscribe_stop().unwrap();
        let second = subscribe_stop().unwrap();
        assert_eq!(live_subscriber_count(), 2);
        notify_subscribers();
        assert!(first.try_recv().is_ok());
        assert!(second.try_recv().is_ok());
        drop(first);
        drop(second);
    }

    #[test]
    fn broadcast_reaches_fresh_subscriber() {
        let _serial = lock_serial();
        prune_stale();
        let receiver = subscribe_stop().unwrap();
        notify_subscribers();
        assert!(receiver.recv_timeout(Duration::from_secs(5)).is_ok());
        drop(receiver);
    }
}
