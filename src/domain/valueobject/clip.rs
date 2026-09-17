use std::{error::Error, fmt};

use super::FeatureVector;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ClipIdError {
    Empty,
}

impl fmt::Display for ClipIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("clip file name must not be empty"),
        }
    }
}

impl Error for ClipIdError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClipId(String);

impl ClipId {
    pub fn new(value: impl Into<String>) -> Result<Self, ClipIdError> {
        let value = value.into();

        if value.trim().is_empty() {
            return Err(ClipIdError::Empty);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ClipId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug)]
pub struct ClipMetadata {
    pub id: ClipId,
    pub features: FeatureVector,
}

impl ClipMetadata {
    pub fn new(id: ClipId, features: FeatureVector) -> Self {
        Self { id, features }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_blank_clip_id() {
        assert_eq!(ClipId::new("  ").unwrap_err(), ClipIdError::Empty);
    }
}
