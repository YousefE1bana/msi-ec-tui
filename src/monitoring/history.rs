//! Bounded in-memory history of hardware snapshots.
//!
//! A plain FIFO data structure: no I/O, no timers, no backend knowledge.
//! Stores [`HardwareSnapshot`] only — identity and mode do not change per
//! tick and must not be duplicated into every sample.

use std::collections::VecDeque;
use std::num::NonZeroUsize;

use crate::hardware::HardwareSnapshot;

/// Default sample-count bound. This is a sample count, not a time
/// guarantee: the covered wall-clock span varies with the poll interval.
pub const DEFAULT_HISTORY_CAPACITY: usize = 60;

/// Bounded FIFO of snapshots; oldest entries are evicted first.
#[derive(Debug, Clone)]
pub struct SnapshotHistory {
    capacity: NonZeroUsize,
    entries: VecDeque<HardwareSnapshot>,
}

impl SnapshotHistory {
    /// Creates an empty history holding at most `capacity` snapshots.
    /// Non-zero capacity is enforced by type.
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity.get()),
        }
    }

    /// Appends a snapshot, evicting exactly the oldest entry when full.
    /// The history never exceeds capacity.
    pub fn push(&mut self, snapshot: HardwareSnapshot) {
        if self.entries.len() == self.capacity.get() {
            self.entries.pop_front();
        }
        self.entries.push_back(snapshot);
    }

    /// Number of stored snapshots.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no snapshots are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Maximum number of stored snapshots.
    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }

    /// Most recently pushed snapshot, if any.
    pub fn latest(&self) -> Option<&HardwareSnapshot> {
        self.entries.back()
    }

    /// Stored snapshots, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = &HardwareSnapshot> {
        self.entries.iter()
    }

    /// Drops all stored snapshots, keeping the configured capacity.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for SnapshotHistory {
    fn default() -> Self {
        Self::new(
            NonZeroUsize::new(DEFAULT_HISTORY_CAPACITY)
                .expect("default history capacity is non-zero"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_with_capacity(capacity: usize) -> SnapshotHistory {
        SnapshotHistory::new(NonZeroUsize::new(capacity).unwrap())
    }

    fn battery_snapshot(percentage: u8) -> HardwareSnapshot {
        HardwareSnapshot {
            battery_percentage: Some(percentage),
            ..Default::default()
        }
    }

    #[test]
    fn new_history_starts_empty() {
        let history = history_with_capacity(4);
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
        assert_eq!(history.latest(), None);
    }

    #[test]
    fn capacity_reports_configured_value() {
        assert_eq!(history_with_capacity(7).capacity(), 7);
    }

    #[test]
    fn push_increases_len_and_latest() {
        let mut history = history_with_capacity(4);
        history.push(battery_snapshot(10));
        assert_eq!(history.len(), 1);
        assert!(!history.is_empty());
        assert_eq!(history.latest().unwrap().battery_percentage, Some(10));
    }

    #[test]
    fn pushes_below_capacity_preserve_all_entries() {
        let mut history = history_with_capacity(4);
        for percentage in [10, 20, 30] {
            history.push(battery_snapshot(percentage));
        }
        let kept: Vec<u8> = history
            .iter()
            .map(|snapshot| snapshot.battery_percentage.unwrap())
            .collect();
        assert_eq!(kept, vec![10, 20, 30]);
    }

    #[test]
    fn reaching_capacity_preserves_all_entries() {
        let mut history = history_with_capacity(3);
        for percentage in [10, 20, 30] {
            history.push(battery_snapshot(percentage));
        }
        assert_eq!(history.len(), 3);
        let kept: Vec<u8> = history
            .iter()
            .map(|snapshot| snapshot.battery_percentage.unwrap())
            .collect();
        assert_eq!(kept, vec![10, 20, 30]);
    }

    #[test]
    fn pushing_past_capacity_evicts_oldest_exactly_once() {
        let mut history = history_with_capacity(3);
        for percentage in [10, 20, 30, 40] {
            history.push(battery_snapshot(percentage));
        }
        assert_eq!(history.len(), 3);
        let kept: Vec<u8> = history
            .iter()
            .map(|snapshot| snapshot.battery_percentage.unwrap())
            .collect();
        assert_eq!(kept, vec![20, 30, 40]);
    }

    #[test]
    fn repeated_overflow_never_exceeds_capacity() {
        let mut history = history_with_capacity(3);
        for percentage in 0..50 {
            history.push(battery_snapshot(percentage));
            assert!(history.len() <= 3);
        }
        assert_eq!(history.len(), 3);
        let kept: Vec<u8> = history
            .iter()
            .map(|snapshot| snapshot.battery_percentage.unwrap())
            .collect();
        assert_eq!(kept, vec![47, 48, 49]);
    }

    #[test]
    fn iteration_order_is_oldest_to_newest() {
        let mut history = history_with_capacity(5);
        for percentage in [3, 1, 2] {
            history.push(battery_snapshot(percentage));
        }
        let mut iterated = history.iter();
        assert_eq!(iterated.next().unwrap().battery_percentage, Some(3));
        assert_eq!(iterated.next().unwrap().battery_percentage, Some(1));
        assert_eq!(iterated.next().unwrap().battery_percentage, Some(2));
        assert_eq!(iterated.next(), None);
    }

    #[test]
    fn latest_updates_after_overflow() {
        let mut history = history_with_capacity(2);
        history.push(battery_snapshot(10));
        history.push(battery_snapshot(20));
        history.push(battery_snapshot(30));
        assert_eq!(history.latest().unwrap().battery_percentage, Some(30));
    }

    #[test]
    fn capacity_one_keeps_only_newest() {
        let mut history = history_with_capacity(1);
        history.push(battery_snapshot(10));
        history.push(battery_snapshot(20));
        assert_eq!(history.len(), 1);
        assert_eq!(history.latest().unwrap().battery_percentage, Some(20));
    }

    #[test]
    fn default_capacity_matches_constant() {
        assert_eq!(
            SnapshotHistory::default().capacity(),
            DEFAULT_HISTORY_CAPACITY
        );
        assert_eq!(DEFAULT_HISTORY_CAPACITY, 60);
    }

    #[test]
    fn history_owns_snapshots_independently() {
        let mut history = history_with_capacity(4);
        {
            let snapshot = battery_snapshot(42);
            history.push(snapshot);
        }
        assert_eq!(history.latest().unwrap().battery_percentage, Some(42));
    }

    #[test]
    fn clear_empties_history() {
        let mut history = history_with_capacity(4);
        history.push(battery_snapshot(10));
        history.push(battery_snapshot(20));
        history.clear();
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
        assert_eq!(history.latest(), None);
        assert_eq!(history.capacity(), 4);
        history.push(battery_snapshot(30));
        assert_eq!(history.latest().unwrap().battery_percentage, Some(30));
    }
}
