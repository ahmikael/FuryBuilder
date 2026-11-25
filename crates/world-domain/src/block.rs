use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A namespaced block state: `minecraft:oak_stairs[facing=north,half=bottom]`.
///
/// Identity is the exact namespaced id plus the property map. Ordering of properties is
/// canonicalized through `BTreeMap`, so two equal states always serialize identically.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlockState {
    name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlockStateError {
    #[error("block id must be namespaced as namespace:path (got `{0}`)")]
    NotNamespaced(String),
    #[error("block id contains invalid characters: `{0}`")]
    InvalidCharacters(String),
    #[error("property key contains invalid characters: `{0}`")]
    InvalidPropertyKey(String),
    #[error("property value contains invalid characters: `{0}`")]
    InvalidPropertyValue(String),
}

pub(crate) fn is_lower_snake(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.' || b == b'-'
        })
}

impl BlockState {
    /// Creates a validated block state. Property ordering is normalized.
    pub fn new(
        name: impl Into<String>,
        properties: BTreeMap<String, String>,
    ) -> Result<Self, BlockStateError> {
        let name = name.into();
        if !name.contains(':') {
            return Err(BlockStateError::NotNamespaced(name));
        }
        let (namespace, path) = name.split_once(':').expect("checked above");
        if !is_lower_snake(namespace) || !is_lower_snake(path) || path.contains(':') {
            return Err(BlockStateError::InvalidCharacters(name));
        }
        for (key, value) in &properties {
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                return Err(BlockStateError::InvalidPropertyKey(key.clone()));
            }
            if value.is_empty()
                || value
                    .chars()
                    .any(|c| c.is_whitespace() || c == ',' || c == ']')
            {
                return Err(BlockStateError::InvalidPropertyValue(value.clone()));
            }
        }
        Ok(Self { name, properties })
    }

    pub fn simple(name: impl Into<String>) -> Result<Self, BlockStateError> {
        Self::new(name, BTreeMap::new())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn properties(&self) -> &BTreeMap<String, String> {
        &self.properties
    }

    pub fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }

    /// The only state that can be dropped from sparse storage without changing the block.
    pub fn is_implicit_air(&self) -> bool {
        self.name == "minecraft:air" && self.properties.is_empty()
    }
}

impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)?;
        if !self.properties.is_empty() {
            f.write_str("[")?;
            for (index, (key, value)) in self.properties.iter().enumerate() {
                if index > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{key}={value}")?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}

impl FromStr for BlockState {
    type Err = BlockStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, properties) = match value.split_once('[') {
            Some((name, rest)) => {
                let rest = rest
                    .strip_suffix(']')
                    .ok_or_else(|| BlockStateError::InvalidCharacters(value.to_owned()))?;
                let mut map = BTreeMap::new();
                if !rest.is_empty() {
                    for pair in rest.split(',') {
                        let (key, prop_value) = pair
                            .split_once('=')
                            .ok_or_else(|| BlockStateError::InvalidCharacters(value.to_owned()))?;
                        map.insert(key.to_owned(), prop_value.to_owned());
                    }
                }
                (name, map)
            }
            None => (value, BTreeMap::new()),
        };
        Self::new(name, properties)
    }
}

impl Default for BlockState {
    fn default() -> Self {
        Self::simple("minecraft:air").expect("air is a valid literal")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_displays_properties() {
        let parsed: BlockState = "minecraft:oak_stairs[facing=north,half=bottom]"
            .parse()
            .expect("valid state");
        assert_eq!(parsed.name(), "minecraft:oak_stairs");
        assert_eq!(
            parsed.properties().get("facing").map(String::as_str),
            Some("north")
        );
        assert_eq!(
            parsed.to_string(),
            "minecraft:oak_stairs[facing=north,half=bottom]"
        );
    }

    #[test]
    fn property_order_is_canonical() {
        let a: BlockState = "minecraft:oak_stairs[half=bottom,facing=north]"
            .parse()
            .unwrap();
        let b: BlockState = "minecraft:oak_stairs[facing=north,half=bottom]"
            .parse()
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.to_string(), b.to_string());
    }

    #[test]
    fn rejects_invalid_ids() {
        assert!(BlockState::simple("stone").is_err());
        assert!(BlockState::simple("minecraft:Oak Planks").is_err());
        assert!(BlockState::simple("minecraft:").is_err());
        assert!("minecraft:stone[=x]".parse::<BlockState>().is_err());
    }

    #[test]
    fn recognises_air_variants() {
        assert!(BlockState::default().is_air());
        assert!(BlockState::simple("minecraft:cave_air").unwrap().is_air());
        assert!(!BlockState::simple("minecraft:stone").unwrap().is_air());
    }
}
