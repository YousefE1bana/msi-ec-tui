//! Pure history series for thermal and fan visualization.
//!
//! [`SnapshotHistory`] in, render-ready series out. No backend reads, no
//! mutation, no invention: a metric that is `None` in a stored snapshot is
//! omitted from that series rather than zero-filled, with no interpolation
//! and no clamping. Order is always oldest -> newest. Temperatures stay
//! actual degrees Celsius; fan values stay percentage/raw (valid up to
//! 150) and are never labeled as revolutions per minute.

use crate::monitoring::SnapshotHistory;

/// Block glyphs for terminal-native sparklines, quietest to loudest.
const GLYPHS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// CPU temperatures in degrees Celsius, oldest first. Missing samples are
/// omitted, never zero-filled.
pub fn cpu_temperatures(history: &SnapshotHistory) -> Vec<u8> {
    history
        .iter()
        .filter_map(|snapshot| snapshot.cpu_temperature.map(|reading| reading.get()))
        .collect()
}

/// GPU temperatures in degrees Celsius, oldest first. Missing samples are
/// omitted, never zero-filled.
pub fn gpu_temperatures(history: &SnapshotHistory) -> Vec<u8> {
    history
        .iter()
        .filter_map(|snapshot| snapshot.gpu_temperature.map(|reading| reading.get()))
        .collect()
}

/// CPU fan values as percentage/raw units, oldest first. Values may
/// validly reach 150. Missing samples are omitted, never zero-filled.
pub fn cpu_fans(history: &SnapshotHistory) -> Vec<u16> {
    history
        .iter()
        .filter_map(|snapshot| snapshot.cpu_fan.map(|value| value.get()))
        .collect()
}

/// GPU fan values as percentage/raw units, oldest first. Values may
/// validly reach 150. Missing samples are omitted, never zero-filled.
pub fn gpu_fans(history: &SnapshotHistory) -> Vec<u16> {
    history
        .iter()
        .filter_map(|snapshot| snapshot.gpu_fan.map(|value| value.get()))
        .collect()
}

/// Newest at most `max_points` values, kept in original oldest -> newest
/// order so time always runs left -> right. Copies nothing beyond the
/// returned window.
pub fn take_newest<T: Copy>(values: &[T], max_points: usize) -> &[T] {
    let start = values.len().saturating_sub(max_points);
    &values[start..]
}

/// Minimum and maximum of a series, or `None` when empty.
pub fn min_max<T: Copy + Ord>(values: &[T]) -> Option<(T, T)> {
    let min = values.iter().min()?;
    let max = values.iter().max()?;
    Some((*min, *max))
}

/// Terminal-native sparkline over the newest at most `max_points` samples.
/// Scales to the shown window's own min/max; a flat window renders mid
/// glyphs rather than fabricating variation. Empty input renders empty.
pub fn sparkline<T>(values: &[T], max_points: usize) -> String
where
    T: Copy + Ord + Into<u64>,
{
    let shown = take_newest(values, max_points);
    if shown.is_empty() {
        return String::new();
    }
    let numbers: Vec<u64> = shown.iter().map(|value| (*value).into()).collect();
    let min = numbers
        .iter()
        .min()
        .expect("non-empty window has a minimum");
    let max = numbers
        .iter()
        .max()
        .expect("non-empty window has a maximum");
    numbers
        .iter()
        .map(|value| {
            let index = if max == min {
                3
            } else {
                ((value - min) * (GLYPHS.len() as u64 - 1) / (max - min)) as usize
            };
            GLYPHS[index]
        })
        .collect()
}

/// Full-detail temperature history rows for the Dashboard panel. Each
/// sensor renders a latest/min/max label line plus a glyph line capped at
/// `max_points` newest samples, or a single honest no-history row when
/// that sensor has no samples. Never invents the other sensor.
pub fn temperature_history_lines(history: &SnapshotHistory, max_points: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for (label, series) in [
        ("CPU Temp History", cpu_temperatures(history)),
        ("GPU Temp History", gpu_temperatures(history)),
    ] {
        match (series.last(), min_max(&series)) {
            (Some(latest), Some((min, max))) => {
                rows.push(format!("{label}: {latest}°C min {min} / max {max}"));
                let glyphs = sparkline(&series, max_points);
                if !glyphs.is_empty() {
                    rows.push(glyphs);
                }
            }
            _ => rows.push(format!("{label}: No history")),
        }
    }
    rows
}

/// Full-detail fan history rows for the Fans panel: a latest/min/max label
/// line plus a glyph line capped at `max_points` newest samples per fan.
/// Percentage/raw units with `%` semantics throughout. Never invents the
/// other fan.
pub fn fan_history_lines(history: &SnapshotHistory, max_points: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for (label, series) in [
        ("CPU Fan History", cpu_fans(history)),
        ("GPU Fan History", gpu_fans(history)),
    ] {
        match (series.last(), min_max(&series)) {
            (Some(latest), Some((min, max))) => {
                rows.push(format!("{label}: {latest}% min {min} / max {max}"));
                let glyphs = sparkline(&series, max_points);
                if !glyphs.is_empty() {
                    rows.push(glyphs);
                }
            }
            _ => rows.push(format!("{label}: No history")),
        }
    }
    rows
}

/// Concise temperature summaries for compact tiers:
/// `CPU History: 12 samples 52-67°C`. Sensors without samples contribute
/// no row rather than a fabricated one.
pub fn temperature_summary_lines(history: &SnapshotHistory) -> Vec<String> {
    let mut rows = Vec::new();
    for (label, series) in [
        ("CPU History", cpu_temperatures(history)),
        ("GPU History", gpu_temperatures(history)),
    ] {
        if let Some((min, max)) = min_max(&series) {
            rows.push(format!("{label}: {} samples {min}-{max}°C", series.len()));
        }
    }
    rows
}

/// Concise fan summaries for compact tiers:
/// `CPU Fan History: 12 samples 31-64%`. Fans without samples contribute
/// no row rather than a fabricated one.
pub fn fan_summary_lines(history: &SnapshotHistory) -> Vec<String> {
    let mut rows = Vec::new();
    for (label, series) in [
        ("CPU Fan History", cpu_fans(history)),
        ("GPU Fan History", gpu_fans(history)),
    ] {
        if let Some((min, max)) = min_max(&series) {
            rows.push(format!("{label}: {} samples {min}-{max}%", series.len()));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use crate::hardware::{FanPercent, HardwareSnapshot, TemperatureCelsius};

    use super::*;

    fn temperature_history(points: &[(Option<u8>, Option<u8>)]) -> SnapshotHistory {
        let mut history =
            SnapshotHistory::new(NonZeroUsize::new(60).expect("test capacity is non-zero"));
        for (cpu, gpu) in points {
            history.push(HardwareSnapshot {
                cpu_temperature: cpu.and_then(|value| TemperatureCelsius::try_from(value).ok()),
                gpu_temperature: gpu.and_then(|value| TemperatureCelsius::try_from(value).ok()),
                ..Default::default()
            });
        }
        history
    }

    fn fan_history(points: &[(Option<u16>, Option<u16>)]) -> SnapshotHistory {
        let mut history =
            SnapshotHistory::new(NonZeroUsize::new(60).expect("test capacity is non-zero"));
        for (cpu, gpu) in points {
            history.push(HardwareSnapshot {
                cpu_fan: cpu.and_then(|value| FanPercent::try_from(value).ok()),
                gpu_fan: gpu.and_then(|value| FanPercent::try_from(value).ok()),
                ..Default::default()
            });
        }
        history
    }

    #[test]
    fn empty_history_yields_empty_series() {
        let history = SnapshotHistory::default();
        assert!(cpu_temperatures(&history).is_empty());
        assert!(gpu_temperatures(&history).is_empty());
        assert!(cpu_fans(&history).is_empty());
        assert!(gpu_fans(&history).is_empty());
        assert_eq!(min_max(&cpu_temperatures(&history)), None);
        assert_eq!(sparkline(&cpu_temperatures(&history), 20), "");
    }

    #[test]
    fn cpu_temperature_extraction_preserves_order() {
        let history = temperature_history(&[(Some(52), None), (Some(67), None), (Some(58), None)]);
        assert_eq!(cpu_temperatures(&history), vec![52, 67, 58]);
    }

    #[test]
    fn gpu_temperature_extraction_preserves_order() {
        let history = temperature_history(&[(None, Some(47)), (None, Some(59)), (None, Some(51))]);
        assert_eq!(gpu_temperatures(&history), vec![47, 59, 51]);
    }

    #[test]
    fn cpu_fan_extraction_preserves_order() {
        let history = fan_history(&[(Some(31), None), (Some(64), None), (Some(42), None)]);
        assert_eq!(cpu_fans(&history), vec![31, 64, 42]);
    }

    #[test]
    fn gpu_fan_extraction_preserves_order() {
        let history = fan_history(&[(None, Some(20)), (None, Some(90)), (None, Some(33))]);
        assert_eq!(gpu_fans(&history), vec![20, 90, 33]);
    }

    #[test]
    fn missing_values_are_omitted_not_zero_filled() {
        let history = temperature_history(&[(Some(60), None), (None, Some(50)), (None, None)]);
        assert_eq!(cpu_temperatures(&history), vec![60]);
        assert_eq!(gpu_temperatures(&history), vec![50]);
        let fans = fan_history(&[(Some(40), None), (None, None)]);
        assert_eq!(cpu_fans(&fans), vec![40]);
        assert!(gpu_fans(&fans).is_empty());
    }

    #[test]
    fn fan_ceiling_value_survives_verbatim() {
        let history = fan_history(&[(Some(150), Some(150))]);
        assert_eq!(cpu_fans(&history), vec![150]);
        assert_eq!(gpu_fans(&history), vec![150]);
        assert_eq!(min_max(&cpu_fans(&history)), Some((150, 150)));
    }

    #[test]
    fn history_capacity_remains_sixty() {
        assert_eq!(SnapshotHistory::default().capacity(), 60);
        assert_eq!(crate::monitoring::DEFAULT_HISTORY_CAPACITY, 60);
    }

    #[test]
    fn newest_truncation_keeps_newest_in_order() {
        let values = vec![10u8, 20, 30, 40, 50];
        assert_eq!(take_newest(&values, 3), &[30, 40, 50]);
        assert_eq!(take_newest(&values, 99), &[10, 20, 30, 40, 50]);
        assert_eq!(take_newest(&values, 0), &[] as &[u8]);
        assert_eq!(take_newest(&values, 1), &[50]);
    }

    #[test]
    fn min_max_is_correct() {
        assert_eq!(min_max(&[52u8, 67, 58]), Some((52, 67)));
        assert_eq!(min_max(&[42u8]), Some((42, 42)));
        assert_eq!(min_max(&[] as &[u8]), None);
    }

    #[test]
    fn sparkline_scales_to_window_without_reversal() {
        // Oldest -> newest must render left -> right: rising values rise.
        let glyphs: Vec<char> = sparkline(&[10u8, 20, 30, 40], 10).chars().collect();
        assert_eq!(glyphs.len(), 4);
        assert!(glyphs.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(sparkline(&[10u8, 20, 30, 40], 2).chars().count(), 2);
    }

    #[test]
    fn sparkline_flat_window_has_no_fabricated_variation() {
        assert_eq!(sparkline(&[60u8, 60, 60], 10), "▄▄▄");
    }

    #[test]
    fn glyph_rows_never_exceed_point_budget() {
        let mut history =
            SnapshotHistory::new(NonZeroUsize::new(60).expect("test capacity is non-zero"));
        for value in 40..100u8 {
            history.push(HardwareSnapshot {
                cpu_temperature: TemperatureCelsius::try_from(value).ok(),
                ..Default::default()
            });
        }
        let rows = temperature_history_lines(&history, 12);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].chars().count(), 12);
        // Newest sample still anchors the row.
        assert!(rows[0].contains("99°C"));
        // GPU side stays honest without invention.
        assert_eq!(rows[2], "GPU Temp History: No history");
    }

    #[test]
    fn helpers_touch_no_backend_by_construction() {
        // Helpers accept only &SnapshotHistory: there is no backend
        // parameter to read from. This test pins that with a history
        // built purely from values.
        let history = temperature_history(&[(Some(60), Some(50))]);
        let rows = temperature_history_lines(&history, 10);
        assert_eq!(rows.len(), 4);
        assert!(rows[0].contains("CPU Temp History: 60"));
        assert!(rows[2].contains("GPU Temp History: 50"));
        assert_eq!(fan_history_lines(&history, 10).len(), 2);
    }
}
