//! Shared, format-neutral structure model.
//!
//! Every adapter converts to and from this model. Positions are local to the structure
//! minimum corner (`0..size`); the caller chooses a world origin when inserting. Palette
//! entries are canonical `BlockState` values, so properties and orientations survive
//! import/export without string concatenation shortcuts.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use thiserror::Error;
use world_domain::{BlockBox, BlockPos, BlockState, World};

pub const MAX_STRUCTURE_AXIS: i32 = 1024;
pub const MAX_STRUCTURE_VOLUME: u64 = 1_000_000;
pub const MAX_STRUCTURE_PALETTE: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StructureError {
    #[error("structure size {0:?} is invalid")]
    InvalidSize([i32; 3]),
    #[error("structure volume {volume} exceeds limit {limit}")]
    TooLarge { volume: u64, limit: u64 },
    #[error("structure palette is empty or exceeds {MAX_STRUCTURE_PALETTE} entries")]
    InvalidPalette,
    #[error("structure block index {index} is outside the palette")]
    InvalidPaletteIndex { index: usize },
    #[error("structure block position {pos:?} is outside the structure")]
    OutOfBounds { pos: [i32; 3] },
    #[error("structure contains duplicate block position {pos:?}")]
    DuplicatePosition { pos: [i32; 3] },
    #[error("structure block count {count} exceeds the volume {volume}")]
    TooManyBlocks { count: usize, volume: u64 },
    #[error("block state `{0}` is invalid")]
    InvalidBlockState(String),
    #[error("world region could not be read: {0}")]
    World(String),
    #[error("block position overflow at {0:?}")]
    PositionOverflow([i32; 3]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureBlock {
    pub pos: [i32; 3],
    pub palette_index: usize,
}

/// Dense bounds plus a sparse list of explicitly stored blocks. Missing positions are air,
/// which is the Java structure convention. Export emits non-air blocks only; import treats
/// missing positions as air. `include_air` controls whether insertion clears the volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    pub data_version: i32,
    pub size: [i32; 3],
    pub palette: Vec<BlockState>,
    pub blocks: Vec<StructureBlock>,
}

impl Structure {
    pub fn new(
        data_version: i32,
        size: [i32; 3],
        palette: Vec<BlockState>,
        blocks: Vec<StructureBlock>,
    ) -> Result<Self, StructureError> {
        let mut structure = Self {
            data_version,
            size,
            palette,
            blocks,
        };
        structure.blocks.sort_by_key(|block| block.pos);
        structure.validate()?;
        Ok(structure)
    }

    pub fn validate(&self) -> Result<(), StructureError> {
        let volume = self.volume()?;
        if self.palette.is_empty() || self.palette.len() > MAX_STRUCTURE_PALETTE {
            return Err(StructureError::InvalidPalette);
        }
        if self.blocks.len() as u64 > volume {
            return Err(StructureError::TooManyBlocks {
                count: self.blocks.len(),
                volume,
            });
        }
        let mut seen = BTreeSet::new();
        for block in &self.blocks {
            if block.palette_index >= self.palette.len() {
                return Err(StructureError::InvalidPaletteIndex {
                    index: block.palette_index,
                });
            }
            self.check_local(block.pos)?;
            if !seen.insert(block.pos) {
                return Err(StructureError::DuplicatePosition { pos: block.pos });
            }
        }
        Ok(())
    }

    pub fn volume(&self) -> Result<u64, StructureError> {
        if self
            .size
            .iter()
            .any(|axis| *axis <= 0 || *axis > MAX_STRUCTURE_AXIS)
        {
            return Err(StructureError::InvalidSize(self.size));
        }
        let volume = self.size[0] as u64 * self.size[1] as u64 * self.size[2] as u64;
        if volume > MAX_STRUCTURE_VOLUME {
            return Err(StructureError::TooLarge {
                volume,
                limit: MAX_STRUCTURE_VOLUME,
            });
        }
        Ok(volume)
    }

    fn check_local(&self, pos: [i32; 3]) -> Result<(), StructureError> {
        let inside = (0..3).all(|axis| pos[axis] >= 0 && pos[axis] < self.size[axis]);
        if !inside {
            return Err(StructureError::OutOfBounds { pos });
        }
        Ok(())
    }

    /// Captures an inclusive world region. Non-air blocks are stored sparsely with local
    /// coordinates; deterministic palette ordering makes repeated exports byte-identical.
    pub fn from_world(
        world: &World,
        dimension: &str,
        bounds: BlockBox,
        data_version: i32,
    ) -> Result<Self, StructureError> {
        let size = bounds.size();
        let mut structure = Self {
            data_version,
            size: [size[0] as i32, size[1] as i32, size[2] as i32],
            palette: Vec::new(),
            blocks: Vec::new(),
        };
        structure.volume()?;

        let mut states = BTreeSet::new();
        for pos in bounds.iter() {
            let state = world
                .get_block(dimension, pos)
                .map_err(|error| StructureError::World(error.to_string()))?;
            if !state.is_air() {
                states.insert(state);
            }
        }
        if states.is_empty() {
            states.insert(BlockState::default());
        }
        structure.palette = states.into_iter().collect();
        let index: HashMap<&BlockState, usize> = structure
            .palette
            .iter()
            .enumerate()
            .map(|(index, state)| (state, index))
            .collect();
        for pos in bounds.iter() {
            let state = world
                .get_block(dimension, pos)
                .map_err(|error| StructureError::World(error.to_string()))?;
            if state.is_air() {
                continue;
            }
            structure.blocks.push(StructureBlock {
                pos: [
                    pos.x - bounds.min.x,
                    pos.y - bounds.min.y,
                    pos.z - bounds.min.z,
                ],
                palette_index: *index.get(&state).expect("palette was collected above"),
            });
        }
        // The capture iterator visits X, then Z, then Y. Java structure determinism and
        // `prepare_export` compare blocks by a canonical position order, so normalize here.
        structure.blocks.sort_by_key(|block| block.pos);
        structure.validate()?;
        Ok(structure)
    }

    pub fn non_air_blocks(&self) -> usize {
        self.blocks
            .iter()
            .filter(|block| !self.palette[block.palette_index].is_air())
            .count()
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Converts the structure into world positions at `origin`.
    ///
    /// With `include_air` the whole bounding box is emitted (missing positions become air),
    /// which clears the target volume like a structure block paste. Without it only
    /// explicitly stored non-air blocks are written and existing blocks in holes remain.
    pub fn to_world_changes(
        &self,
        origin: BlockPos,
        include_air: bool,
    ) -> Result<Vec<(BlockPos, BlockState)>, StructureError> {
        self.validate()?;
        let volume = self.volume()?;
        if include_air {
            let mut dense: BTreeMap<[i32; 3], BlockState> = BTreeMap::new();
            for block in &self.blocks {
                dense.insert(block.pos, self.palette[block.palette_index].clone());
            }
            let mut changes = Vec::with_capacity(volume as usize);
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    for x in 0..self.size[0] {
                        let state = dense
                            .get(&[x, y, z])
                            .cloned()
                            .unwrap_or_else(BlockState::default);
                        changes.push((world_pos(origin, [x, y, z])?, state));
                    }
                }
            }
            Ok(changes)
        } else {
            let mut changes = Vec::with_capacity(self.blocks.len());
            for block in &self.blocks {
                let state = self.palette[block.palette_index].clone();
                if state.is_air() {
                    continue;
                }
                changes.push((world_pos(origin, block.pos)?, state));
            }
            Ok(changes)
        }
    }
}

fn world_pos(origin: BlockPos, local: [i32; 3]) -> Result<BlockPos, StructureError> {
    let x = origin.x.checked_add(local[0]);
    let y = origin.y.checked_add(local[1]);
    let z = origin.z.checked_add(local[2]);
    match (x, y, z) {
        (Some(x), Some(y), Some(z)) => Ok(BlockPos::new(x, y, z)),
        _ => Err(StructureError::PositionOverflow(local)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_domain::TARGET_DATA_VERSION;

    fn stone() -> BlockState {
        "minecraft:stone".parse().unwrap()
    }

    fn stairs() -> BlockState {
        "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]"
            .parse()
            .unwrap()
    }

    #[test]
    fn captures_negative_regions_with_properties_and_local_positions() {
        let mut world = World::new_empty();
        world
            .set_block("minecraft:overworld", BlockPos::new(-3, 64, -2), &stone())
            .unwrap();
        world
            .set_block("minecraft:overworld", BlockPos::new(-1, 66, 0), &stairs())
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(-3, 64, -2), BlockPos::new(-1, 66, 0)).unwrap();
        let structure =
            Structure::from_world(&world, "minecraft:overworld", bounds, TARGET_DATA_VERSION)
                .unwrap();
        assert_eq!(structure.size, [3, 3, 3]);
        assert_eq!(structure.palette, vec![stairs(), stone()]);
        assert_eq!(structure.blocks.len(), 2);
        assert!(structure.blocks.iter().any(
            |block| block.pos == [0, 0, 0] && structure.palette[block.palette_index] == stone()
        ));
        assert!(structure
            .blocks
            .iter()
            .any(|block| block.pos == [2, 2, 2]
                && structure.palette[block.palette_index] == stairs()));
    }

    #[test]
    fn all_air_region_has_a_valid_palette_and_no_blocks() {
        let world = World::new_empty();
        let bounds = BlockBox::new(BlockPos::new(0, 70, 0), BlockPos::new(1, 71, 1)).unwrap();
        let structure =
            Structure::from_world(&world, "minecraft:overworld", bounds, TARGET_DATA_VERSION)
                .unwrap();
        assert_eq!(structure.palette, vec![BlockState::default()]);
        assert!(structure.blocks.is_empty());
        let changes = structure
            .to_world_changes(BlockPos::new(10, 70, 10), true)
            .unwrap();
        assert_eq!(changes.len(), 8);
        assert!(changes.iter().all(|(_, state)| state.is_air()));
    }

    #[test]
    fn insertion_honours_air_and_origin() {
        let structure = Structure::new(
            TARGET_DATA_VERSION,
            [2, 1, 1],
            vec![stone(), BlockState::default()],
            vec![StructureBlock {
                pos: [0, 0, 0],
                palette_index: 0,
            }],
        )
        .unwrap();
        let overlay = structure
            .to_world_changes(BlockPos::new(-5, 80, 7), false)
            .unwrap();
        assert_eq!(overlay.len(), 1);
        assert_eq!(overlay[0].0, BlockPos::new(-5, 80, 7));
        assert_eq!(overlay[0].1, stone());

        let replace = structure
            .to_world_changes(BlockPos::new(-5, 80, 7), true)
            .unwrap();
        assert_eq!(replace.len(), 2);
        assert!(replace[1].1.is_air());
    }

    #[test]
    fn rejects_invalid_bounds_indexes_and_duplicates() {
        assert!(Structure::new(4189, [0, 1, 1], vec![stone()], vec![]).is_err());
        assert!(Structure::new(4189, [1, 1, 1], vec![], vec![]).is_err());
        assert!(Structure::new(
            4189,
            [1, 1, 1],
            vec![stone()],
            vec![StructureBlock {
                pos: [1, 0, 0],
                palette_index: 0
            }]
        )
        .is_err());
        assert!(Structure::new(
            4189,
            [1, 1, 1],
            vec![stone()],
            vec![StructureBlock {
                pos: [0, 0, 0],
                palette_index: 1
            }]
        )
        .is_err());
        assert!(Structure::new(
            4189,
            [1, 1, 1],
            vec![stone()],
            vec![
                StructureBlock {
                    pos: [0, 0, 0],
                    palette_index: 0
                },
                StructureBlock {
                    pos: [0, 0, 0],
                    palette_index: 0
                }
            ]
        )
        .is_err());
    }
}
