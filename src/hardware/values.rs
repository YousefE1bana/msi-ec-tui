//! Validated mode names and sensor readings.

use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModeValidationError {
    #[error("mode name must not be empty")]
    Empty,
    #[error("mode name must already be trimmed")]
    Untrimmed,
    #[error("mode name must not contain newline, carriage return, or NUL")]
    ForbiddenCharacter,
    #[error("mode name must not exceed 64 UTF-8 bytes")]
    TooLong,
}

fn validate_mode(value: &str) -> Result<(), ModeValidationError> {
    if value.is_empty() {
        return Err(ModeValidationError::Empty);
    }
    if value.contains(['\n', '\r', '\0']) {
        return Err(ModeValidationError::ForbiddenCharacter);
    }
    if value.trim() != value {
        return Err(ModeValidationError::Untrimmed);
    }
    if value.len() > 64 {
        return Err(ModeValidationError::TooLong);
    }
    Ok(())
}

macro_rules! mode_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<&str> for $name {
            type Error = ModeValidationError;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                validate_mode(value)?;
                Ok(Self(value.to_owned()))
            }
        }

        impl TryFrom<String> for $name {
            type Error = ModeValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_mode(&value)?;
                Ok(Self(value))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

mode_type!(
    FanMode,
    "A validated, dynamically named fan mode; validity does not imply device support."
);
mode_type!(
    ShiftMode,
    "A validated, dynamically named shift mode; validity does not imply device support."
);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SensorValueError {
    #[error("temperature {0} is outside 0..=100 degrees Celsius")]
    TemperatureOutOfRange(u8),
    #[error("fan percentage-style value {0} is outside 0..=150")]
    FanPercentOutOfRange(u16),
}

/// A temperature reading in degrees Celsius, within 0..=100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemperatureCelsius(u8);

impl TemperatureCelsius {
    pub fn get(&self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for TemperatureCelsius {
    type Error = SensorValueError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        if value > 100 {
            return Err(SensorValueError::TemperatureOutOfRange(value));
        }
        Ok(Self(value))
    }
}

/// The raw percentage-style fan value exposed by `msi-ec`, within 0..=150.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FanPercent(u16);

impl FanPercent {
    pub fn get(&self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for FanPercent {
    type Error = SensorValueError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        if value > 150 {
            return Err(SensorValueError::FanPercentOutOfRange(value));
        }
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! mode_tests {
        ($module:ident, $mode:ident, $known:literal) => {
            mod $module {
                use super::*;

                #[test]
                fn known_mode_and_accessor() {
                    assert_eq!($mode::try_from($known).unwrap().as_str(), $known);
                }

                #[test]
                fn empty_is_rejected() {
                    assert!($mode::try_from("").is_err());
                }

                #[test]
                fn surrounding_whitespace_is_rejected() {
                    for input in [" auto", "auto ", "\tauto", "auto\t", "\u{2003}auto", " "] {
                        assert!($mode::try_from(input).is_err(), "{input:?}");
                    }
                }

                #[test]
                fn newline_is_rejected() {
                    assert!($mode::try_from("auto\nsilent").is_err());
                }

                #[test]
                fn carriage_return_is_rejected() {
                    assert!($mode::try_from("auto\rsilent").is_err());
                }

                #[test]
                fn nul_is_rejected() {
                    assert!($mode::try_from("auto\0silent").is_err());
                }

                #[test]
                fn byte_limit_is_enforced() {
                    for input in ["a".repeat(64), "é".repeat(32)] {
                        assert_eq!($mode::try_from(input.as_str()).unwrap().as_str(), input);
                        assert!($mode::try_from(input).is_ok());
                    }
                    for input in ["a".repeat(65), "é".repeat(33)] {
                        assert!($mode::try_from(input.as_str()).is_err());
                        assert!($mode::try_from(input).is_err());
                    }
                }

                #[test]
                fn unknown_valid_mode_is_accepted() {
                    let mode = $mode::try_from("future-mode_v2").unwrap();
                    assert_eq!(mode.as_str(), "future-mode_v2");
                    assert_eq!(mode.to_string(), "future-mode_v2");
                }

                #[test]
                fn owned_input_preserves_validation() {
                    assert_eq!(
                        $mode::try_from(String::from($known)).unwrap().as_str(),
                        $known
                    );
                    for input in [
                        "",
                        " auto",
                        "auto ",
                        "auto\nsilent",
                        "auto\rsilent",
                        "auto\0silent",
                    ] {
                        assert!($mode::try_from(String::from(input)).is_err());
                    }
                }

                #[test]
                fn equality_clone_and_hash() {
                    let mode = $mode::try_from($known).unwrap();
                    assert_eq!(mode, mode.clone());
                    let modes = std::collections::HashSet::from([mode.clone()]);
                    assert!(modes.contains(&mode));
                }
            }
        };
    }

    mode_tests!(fan_mode, FanMode, "auto");
    mode_tests!(shift_mode, ShiftMode, "comfort");

    #[test]
    fn temperature_boundaries_and_accessor() {
        for value in [0, 63, 100] {
            assert_eq!(TemperatureCelsius::try_from(value).unwrap().get(), value);
        }
    }

    #[test]
    fn excessive_temperature_is_rejected() {
        for value in [101, u8::MAX] {
            assert!(TemperatureCelsius::try_from(value).is_err());
        }
    }

    #[test]
    fn fan_percent_boundaries_and_accessor() {
        for value in [0, 42, 150] {
            assert_eq!(FanPercent::try_from(value).unwrap().get(), value);
        }
    }

    #[test]
    fn excessive_fan_percent_is_rejected() {
        for value in [151, u16::MAX] {
            assert!(FanPercent::try_from(value).is_err());
        }
    }
}
