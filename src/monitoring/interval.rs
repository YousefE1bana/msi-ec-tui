//! Validated poll intervals for monitoring.
//!
//! Only the design-contract intervals exist. No clamping, no timers.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use thiserror::Error;

/// Poll intervals allowed by the design contract.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PollInterval {
    /// 500 milliseconds.
    Ms500,
    /// 1 second (default).
    #[default]
    Sec1,
    /// 2 seconds.
    Sec2,
    /// 5 seconds.
    Sec5,
}

/// Typed error for rejected poll intervals. User-displayable.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PollIntervalError {
    /// A millisecond count outside the design contract.
    #[error("unsupported poll interval: {0} ms")]
    UnsupportedMillis(u64),
    /// A textual spelling outside the accepted forms.
    #[error("unsupported poll interval text: {0}")]
    InvalidText(String),
}

impl PollInterval {
    /// Validates a millisecond count against the design contract.
    pub fn from_millis(value: u64) -> Result<Self, PollIntervalError> {
        match value {
            500 => Ok(Self::Ms500),
            1000 => Ok(Self::Sec1),
            2000 => Ok(Self::Sec2),
            5000 => Ok(Self::Sec5),
            _ => Err(PollIntervalError::UnsupportedMillis(value)),
        }
    }

    /// Converts to a sleep duration for the future polling loop.
    pub fn as_duration(self) -> Duration {
        match self {
            Self::Ms500 => Duration::from_millis(500),
            Self::Sec1 => Duration::from_secs(1),
            Self::Sec2 => Duration::from_secs(2),
            Self::Sec5 => Duration::from_secs(5),
        }
    }
}

impl FromStr for PollInterval {
    type Err = PollIntervalError;

    /// Accepts `500ms`/`1s`/`2s`/`5s` and the canonical millisecond
    /// spellings `500`/`1000`/`2000`/`5000`. Nothing else.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "500ms" | "500" => Ok(Self::Ms500),
            "1s" | "1000" => Ok(Self::Sec1),
            "2s" | "2000" => Ok(Self::Sec2),
            "5s" | "5000" => Ok(Self::Sec5),
            _ => Err(PollIntervalError::InvalidText(text.to_owned())),
        }
    }
}

impl fmt::Display for PollInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ms500 => "500ms",
            Self::Sec1 => "1s",
            Self::Sec2 => "2s",
            Self::Sec5 => "5s",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_one_second() {
        assert_eq!(PollInterval::default(), PollInterval::Sec1);
        assert_eq!(
            PollInterval::default().as_duration(),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn intervals_map_to_durations() {
        assert_eq!(
            PollInterval::Ms500.as_duration(),
            Duration::from_millis(500)
        );
        assert_eq!(PollInterval::Sec1.as_duration(), Duration::from_secs(1));
        assert_eq!(PollInterval::Sec2.as_duration(), Duration::from_secs(2));
        assert_eq!(PollInterval::Sec5.as_duration(), Duration::from_secs(5));
    }

    #[test]
    fn from_millis_accepts_contract_values() {
        assert_eq!(PollInterval::from_millis(500).unwrap(), PollInterval::Ms500);
        assert_eq!(PollInterval::from_millis(1000).unwrap(), PollInterval::Sec1);
        assert_eq!(PollInterval::from_millis(2000).unwrap(), PollInterval::Sec2);
        assert_eq!(PollInterval::from_millis(5000).unwrap(), PollInterval::Sec5);
    }

    #[test]
    fn from_millis_rejects_other_values() {
        for value in [0, 1, 100, 999, 1500, 2500, 10000, u64::MAX] {
            assert!(PollInterval::from_millis(value).is_err(), "{value}");
        }
    }

    #[test]
    fn textual_forms_parse() {
        assert_eq!(
            "500ms".parse::<PollInterval>().unwrap(),
            PollInterval::Ms500
        );
        assert_eq!("1s".parse::<PollInterval>().unwrap(), PollInterval::Sec1);
        assert_eq!("2s".parse::<PollInterval>().unwrap(), PollInterval::Sec2);
        assert_eq!("5s".parse::<PollInterval>().unwrap(), PollInterval::Sec5);
    }

    #[test]
    fn canonical_millis_forms_parse() {
        assert_eq!("500".parse::<PollInterval>().unwrap(), PollInterval::Ms500);
        assert_eq!("1000".parse::<PollInterval>().unwrap(), PollInterval::Sec1);
        assert_eq!("2000".parse::<PollInterval>().unwrap(), PollInterval::Sec2);
        assert_eq!("5000".parse::<PollInterval>().unwrap(), PollInterval::Sec5);
    }

    #[test]
    fn unsupported_text_fails() {
        for text in [
            "", "fast", "slow", "0.5", "1sec", "100ms", "1S", " 1s", "1s ", "6s", "0",
        ] {
            assert!(text.parse::<PollInterval>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn display_emits_canonical_forms() {
        assert_eq!(PollInterval::Ms500.to_string(), "500ms");
        assert_eq!(PollInterval::Sec1.to_string(), "1s");
        assert_eq!(PollInterval::Sec2.to_string(), "2s");
        assert_eq!(PollInterval::Sec5.to_string(), "5s");
    }

    #[test]
    fn parse_display_round_trip_is_stable() {
        for text in ["500ms", "1s", "2s", "5s"] {
            let parsed: PollInterval = text.parse().unwrap();
            assert_eq!(parsed.to_string(), text);
        }
    }

    #[test]
    fn errors_are_displayable() {
        assert!(
            !PollInterval::from_millis(7)
                .unwrap_err()
                .to_string()
                .is_empty()
        );
        assert!(
            !"soon"
                .parse::<PollInterval>()
                .unwrap_err()
                .to_string()
                .is_empty()
        );
    }
}
