//! Length unit types and conversion utilities.

use serde::{Deserialize, Serialize};

/// Supported length units for printer configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LengthUnit {
    /// Micrometers (µm)
    Micron,
    /// Millimeters (mm)
    Millimeter,
    /// Centimeters (cm)
    Centimeter,
    /// Inches (in)
    Inch,
}

impl LengthUnit {
    /// Returns the standard abbreviation for this unit.
    pub fn abbreviation(self) -> &'static str {
        match self {
            Self::Micron => "µm",
            Self::Millimeter => "mm",
            Self::Centimeter => "cm",
            Self::Inch => "in",
        }
    }
}

impl std::fmt::Display for LengthUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Micron => "micron",
            Self::Millimeter => "millimeter",
            Self::Centimeter => "centimeter",
            Self::Inch => "inch",
        };
        f.write_str(name)
    }
}

/// Millimeters per centimeter.
const MM_PER_CM: f64 = 10.0;
/// Millimeters per inch.
const MM_PER_INCH: f64 = 25.4;
/// Millimeters per micron.
const MM_PER_MICRON: f64 = 0.001;

/// Converts a length value from one unit to another.
///
/// The conversion goes through millimeters as an intermediate representation.
///
/// # Examples
///
/// ```
/// use microtome_core::units::{LengthUnit, convert_length};
///
/// let inches = convert_length(25.4, LengthUnit::Millimeter, LengthUnit::Inch);
/// assert!((inches - 1.0).abs() < f64::EPSILON);
/// ```
pub fn convert_length(value: f64, from: LengthUnit, to: LengthUnit) -> f64 {
    if from == to {
        return value;
    }

    // Convert to millimeters first
    let mm = match from {
        LengthUnit::Micron => value * MM_PER_MICRON,
        LengthUnit::Millimeter => value,
        LengthUnit::Centimeter => value * MM_PER_CM,
        LengthUnit::Inch => value * MM_PER_INCH,
    };

    // Convert from millimeters to target unit
    match to {
        LengthUnit::Micron => mm / MM_PER_MICRON,
        LengthUnit::Millimeter => mm,
        LengthUnit::Centimeter => mm / MM_PER_CM,
        LengthUnit::Inch => mm / MM_PER_INCH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_unit_is_identity() {
        assert!(
            (convert_length(42.0, LengthUnit::Millimeter, LengthUnit::Millimeter) - 42.0).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn mm_to_inch() {
        let result = convert_length(25.4, LengthUnit::Millimeter, LengthUnit::Inch);
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn inch_to_mm() {
        let result = convert_length(1.0, LengthUnit::Inch, LengthUnit::Millimeter);
        assert!((result - 25.4).abs() < f64::EPSILON);
    }

    #[test]
    fn mm_to_micron() {
        let result = convert_length(1.0, LengthUnit::Millimeter, LengthUnit::Micron);
        assert!((result - 1000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn micron_to_mm() {
        let result = convert_length(1000.0, LengthUnit::Micron, LengthUnit::Millimeter);
        assert!((result - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cm_to_mm() {
        let result = convert_length(1.0, LengthUnit::Centimeter, LengthUnit::Millimeter);
        assert!((result - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn inch_to_cm() {
        let result = convert_length(1.0, LengthUnit::Inch, LengthUnit::Centimeter);
        assert!((result - 2.54).abs() < 1e-10);
    }

    #[test]
    fn abbreviations() {
        assert_eq!(LengthUnit::Micron.abbreviation(), "µm");
        assert_eq!(LengthUnit::Millimeter.abbreviation(), "mm");
        assert_eq!(LengthUnit::Centimeter.abbreviation(), "cm");
        assert_eq!(LengthUnit::Inch.abbreviation(), "in");
    }

    #[test]
    fn display_names() {
        assert_eq!(LengthUnit::Micron.to_string(), "micron");
        assert_eq!(LengthUnit::Millimeter.to_string(), "millimeter");
    }

    #[test]
    fn serde_round_trip() {
        let unit = LengthUnit::Millimeter;
        let json = serde_json::to_string(&unit).unwrap();
        assert_eq!(json, "\"millimeter\"");
        let deserialized: LengthUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, unit);
    }
}
