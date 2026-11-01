//! Domain primitives shared by DecodeCAD Civil modules.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

pub const MAX_COORDINATE: f64 = 1.0e12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearUnit {
    Millimetre,
    Centimetre,
    #[default]
    Metre,
    Foot,
    Inch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectSettings {
    pub linear_unit: LinearUnit,
    pub crs: Option<String>,
    pub horizontal_datum: Option<String>,
    pub vertical_datum: Option<String>,
    pub coordinate_precision: u8,
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            linear_unit: LinearUnit::Metre,
            crs: None,
            horizontal_datum: None,
            vertical_datum: None,
            coordinate_precision: 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coordinate3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Coordinate3 {
    pub fn new(x: f64, y: f64, z: f64) -> Result<Self, CoordinateError> {
        if ![x, y, z].iter().all(|value| value.is_finite()) {
            return Err(CoordinateError::NonFinite);
        }
        if [x, y, z]
            .iter()
            .any(|value| value.abs() > MAX_COORDINATE)
        {
            return Err(CoordinateError::OutOfRange);
        }
        Ok(Self { x, y, z })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoordinateError {
    #[error("coordinates must be finite numbers")]
    NonFinite,
    #[error("coordinate magnitude exceeds the supported limit")]
    OutOfRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_finite() {
        assert_eq!(
            Coordinate3::new(f64::NAN, 0.0, 0.0),
            Err(CoordinateError::NonFinite)
        );
    }

    #[test]
    fn rejects_extreme_values() {
        assert_eq!(
            Coordinate3::new(MAX_COORDINATE * 2.0, 0.0, 0.0),
            Err(CoordinateError::OutOfRange)
        );
    }
}
