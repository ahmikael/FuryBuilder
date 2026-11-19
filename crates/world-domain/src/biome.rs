use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::block::is_lower_snake;

/// Number of biome cells in one section. Each cell covers a 4x4x4 block volume.
pub const BIOME_CELLS: usize = 64;
/// Largest palette accepted for one section. A section has at most 64 distinct cells.
pub const MAX_BIOME_PALETTE: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BiomeStateError {
    #[error("biome id must be namespaced as namespace:path (got `{0}`)")]
    NotNamespaced(String),
    #[error("biome id contains invalid characters: `{0}`")]
    InvalidCharacters(String),
}

/// A validated namespaced biome id, for example `minecraft:plains`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BiomeState(String);

impl BiomeState {
    pub fn new(value: impl Into<String>) -> Result<Self, BiomeStateError> {
        let value = value.into();
        if !value.contains(':') {
            return Err(BiomeStateError::NotNamespaced(value));
        }
        let (namespace, path) = value.split_once(':').expect("checked above");
        if !is_lower_snake(namespace) || !is_lower_snake(path) || path.contains(':') {
            return Err(BiomeStateError::InvalidCharacters(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BiomeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for BiomeState {
    type Err = BiomeStateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<String> for BiomeState {
    type Error = BiomeStateError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<BiomeState> for String {
    fn from(value: BiomeState) -> Self {
        value.0
    }
}

/// A 4x4x4 biome cell grid for one 16x16x16 section.
///
/// Cells are stored in YZX order as in the Java paletted biome container. The type keeps
/// the same bounded, validated palette model as the block sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeSection {
    palette: Vec<BiomeState>,
    cells: [u16; BIOME_CELLS],
}

impl BiomeSection {
    pub fn from_palette(palette: Vec<BiomeState>, cells: Vec<u16>) -> Option<Self> {
        if palette.is_empty() || palette.len() > MAX_BIOME_PALETTE || cells.len() != BIOME_CELLS {
            return None;
        }
        if cells.iter().any(|index| *index as usize >= palette.len()) {
            return None;
        }
        let mut grid = [0u16; BIOME_CELLS];
        grid.copy_from_slice(&cells);
        Some(Self {
            palette,
            cells: grid,
        })
    }

    /// Builds a section where every cell has the same biome.
    pub fn uniform(biome: BiomeState) -> Self {
        Self {
            palette: vec![biome],
            cells: [0; BIOME_CELLS],
        }
    }

    pub fn palette(&self) -> &[BiomeState] {
        &self.palette
    }

    pub fn cells(&self) -> &[u16] {
        &self.cells
    }

    pub fn get(&self, x: u8, y: u8, z: u8) -> Option<&BiomeState> {
        let index = *self.cells.get(cell_index(x, y, z)?)? as usize;
        self.palette.get(index)
    }

    /// Reads the biome covering a block inside the section (local coordinates 0..=15).
    pub fn biome_at_block(&self, x: u8, y: u8, z: u8) -> Option<&BiomeState> {
        self.get(x / 4, y / 4, z / 4)
    }

    /// Sets one 4x4x4 cell. Returns `None` when a coordinate is out of range.
    pub fn set(&mut self, x: u8, y: u8, z: u8, biome: &BiomeState) -> Option<bool> {
        let index = cell_index(x, y, z)?;
        let palette_index = match self.palette.iter().position(|entry| entry == biome) {
            Some(index) => index,
            None => {
                if self.palette.len() >= MAX_BIOME_PALETTE {
                    return None;
                }
                self.palette.push(biome.clone());
                self.palette.len() - 1
            }
        };
        if self.cells[index] as usize == palette_index {
            return Some(false);
        }
        self.cells[index] = palette_index as u16;
        Some(true)
    }

    /// Sets the biome covering a block inside the section (local coordinates 0..=15).
    pub fn set_at_block(&mut self, x: u8, y: u8, z: u8, biome: &BiomeState) -> Option<bool> {
        if x >= 16 || y >= 16 || z >= 16 {
            return None;
        }
        self.set(x / 4, y / 4, z / 4, biome)
    }

    pub fn is_uniform(&self) -> bool {
        self.palette.len() == 1
    }
}

fn cell_index(x: u8, y: u8, z: u8) -> Option<usize> {
    if x >= 4 || y >= 4 || z >= 4 {
        return None;
    }
    Some((y as usize * 4 + z as usize) * 4 + x as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn biome(name: &str) -> BiomeState {
        name.parse().expect("valid biome")
    }

    #[test]
    fn validates_namespaced_biomes() {
        assert_eq!(biome("minecraft:plains").as_str(), "minecraft:plains");
        assert!("plains".parse::<BiomeState>().is_err());
        assert!("minecraft:".parse::<BiomeState>().is_err());
        assert!("minecraft:Bad Biome".parse::<BiomeState>().is_err());
        assert!("minecraft:a:b".parse::<BiomeState>().is_err());
    }

    #[test]
    fn maps_blocks_to_covering_cells() {
        let mut section = BiomeSection::uniform(biome("minecraft:plains"));
        assert_eq!(
            section.biome_at_block(0, 0, 0).unwrap().as_str(),
            "minecraft:plains"
        );
        assert!(section
            .set_at_block(5, 9, 15, &biome("minecraft:forest"))
            .unwrap());
        assert_eq!(section.get(1, 2, 3).unwrap().as_str(), "minecraft:forest");
        assert_eq!(section.get(0, 0, 0).unwrap().as_str(), "minecraft:plains");
        assert_eq!(section.cells().len(), BIOME_CELLS);
    }

    #[test]
    fn palette_is_bounded_and_rejects_bad_cells() {
        let planes = vec![biome("minecraft:plains")];
        assert!(BiomeSection::from_palette(planes.clone(), vec![0; 64]).is_some());
        assert!(BiomeSection::from_palette(planes.clone(), vec![0; 63]).is_none());
        assert!(BiomeSection::from_palette(planes.clone(), vec![1; 64]).is_none());
        assert!(BiomeSection::from_palette(Vec::new(), vec![0; 64]).is_none());
        assert!(BiomeSection::from_palette(
            (0..=MAX_BIOME_PALETTE)
                .map(|index| biome(&format!("minecraft:biome_{index}")))
                .collect(),
            vec![0; 64]
        )
        .is_none());
    }

    #[test]
    fn serde_uses_the_plain_namespaced_string() {
        let value = biome("minecraft:forest");
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(json, "\"minecraft:forest\"");
        let parsed: BiomeState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, value);
        assert!(serde_json::from_str::<BiomeState>("\"forest\"").is_err());
    }
}
