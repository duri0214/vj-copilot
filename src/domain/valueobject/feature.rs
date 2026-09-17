use std::{error::Error, fmt};

pub const SILENCE_DBFS: f32 = -60.0;

#[derive(Debug, PartialEq)]
pub enum FeatureValueError {
    OutOfRange { name: &'static str, value: f32 },
}

impl fmt::Display for FeatureValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { name, value } => {
                write!(
                    formatter,
                    "{name} must be a finite value between 0 and 1, got {value}"
                )
            }
        }
    }
}

impl Error for FeatureValueError {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FeatureVector {
    energy: f32,
    brightness: f32,
}

impl FeatureVector {
    pub fn new(energy: f32, brightness: f32) -> Result<Self, FeatureValueError> {
        validate_unit_interval("energy", energy)?;
        validate_unit_interval("brightness", brightness)?;

        Ok(Self { energy, brightness })
    }

    pub(crate) fn from_clamped(energy: f32, brightness: f32) -> Self {
        Self {
            energy: clamp_unit(energy),
            brightness: clamp_unit(brightness),
        }
    }

    pub fn energy(self) -> f32 {
        self.energy
    }

    pub fn brightness(self) -> f32 {
        self.brightness
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AnalysisReading {
    pub features: FeatureVector,
    pub rms_dbfs: f32,
    pub peak_dbfs: f32,
    pub centroid_hz: f32,
    pub audible: bool,
}

fn validate_unit_interval(name: &'static str, value: f32) -> Result<(), FeatureValueError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(FeatureValueError::OutOfRange { name, value })
    }
}

fn clamp_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_finite_and_out_of_range_metadata_values() {
        assert!(matches!(
            FeatureVector::new(f32::NAN, 0.5),
            Err(FeatureValueError::OutOfRange { name: "energy", .. })
        ));
        assert!(matches!(
            FeatureVector::new(0.5, 1.1),
            Err(FeatureValueError::OutOfRange {
                name: "brightness",
                ..
            })
        ));
    }

    #[test]
    fn accepts_metadata_values_at_the_range_boundaries() {
        let features = FeatureVector::new(0.0, 1.0).unwrap();

        assert_eq!(features.energy(), 0.0);
        assert_eq!(features.brightness(), 1.0);
    }
}
