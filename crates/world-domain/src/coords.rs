use serde::{Deserialize, Serialize};

/// Floor division for possibly negative integers. Regular integer division truncates
/// toward zero, which produces wrong chunk/section coordinates for negative values.
pub const fn div_floor(value: i32, divisor: i32) -> i32 {
    debug_assert!(divisor > 0);
    let quotient = value / divisor;
    let remainder = value % divisor;
    if remainder != 0 && remainder < 0 {
        quotient - 1
    } else {
        quotient
    }
}

/// Floor modulo for possibly negative integers. Always returns a value in `0..divisor`.
pub const fn mod_floor(value: i32, divisor: i32) -> i32 {
    debug_assert!(divisor > 0);
    let remainder = value % divisor;
    if remainder < 0 {
        remainder + divisor
    } else {
        remainder
    }
}

pub const CHUNK_SIZE: i32 = 16;
pub const SECTION_SIZE: i32 = 16;
pub const REGION_CHUNKS: i32 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn chunk_pos(self) -> ChunkPos {
        ChunkPos::new(div_floor(self.x, CHUNK_SIZE), div_floor(self.z, CHUNK_SIZE))
    }

    pub const fn section_y(self) -> i32 {
        div_floor(self.y, SECTION_SIZE)
    }

    pub const fn section_pos(self) -> SectionPos {
        SectionPos::new(
            div_floor(self.x, SECTION_SIZE),
            div_floor(self.y, SECTION_SIZE),
            div_floor(self.z, SECTION_SIZE),
        )
    }

    pub const fn local_x(self) -> i32 {
        mod_floor(self.x, CHUNK_SIZE)
    }

    pub const fn local_z(self) -> i32 {
        mod_floor(self.z, CHUNK_SIZE)
    }

    pub const fn local_y(self) -> i32 {
        mod_floor(self.y, SECTION_SIZE)
    }

    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub const fn region_pos(self) -> RegionPos {
        RegionPos::new(
            div_floor(self.x, REGION_CHUNKS),
            div_floor(self.z, REGION_CHUNKS),
        )
    }

    pub const fn min_block(self) -> BlockPos {
        BlockPos::new(self.x * CHUNK_SIZE, 0, self.z * CHUNK_SIZE)
    }

    pub fn block_origin(self) -> (i32, i32) {
        (self.x * CHUNK_SIZE, self.z * CHUNK_SIZE)
    }

    pub fn contains_block(self, pos: BlockPos) -> bool {
        pos.chunk_pos() == self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn chunk_pos(self) -> ChunkPos {
        ChunkPos::new(self.x, self.z)
    }

    pub const fn block_origin(self) -> BlockPos {
        BlockPos::new(
            self.x * SECTION_SIZE,
            self.y * SECTION_SIZE,
            self.z * SECTION_SIZE,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl RegionPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_division_handles_negative_values() {
        assert_eq!(div_floor(0, 16), 0);
        assert_eq!(div_floor(15, 16), 0);
        assert_eq!(div_floor(16, 16), 1);
        assert_eq!(div_floor(-1, 16), -1);
        assert_eq!(div_floor(-16, 16), -1);
        assert_eq!(div_floor(-17, 16), -2);
        assert_eq!(mod_floor(-1, 16), 15);
        assert_eq!(mod_floor(-16, 16), 0);
        assert_eq!(mod_floor(-17, 16), 15);
    }

    #[test]
    fn negative_block_positions_map_to_chunks_and_sections() {
        let pos = BlockPos::new(-1, -17, -33);
        assert_eq!(pos.chunk_pos(), ChunkPos::new(-1, -3));
        assert_eq!(pos.section_y(), -2);
        assert_eq!(pos.local_x(), 15);
        assert_eq!(pos.local_z(), 15);
        assert_eq!(pos.local_y(), 15);
        assert_eq!(pos.section_pos(), SectionPos::new(-1, -2, -3));

        let pos = BlockPos::new(-33, -64, 32);
        assert_eq!(pos.chunk_pos(), ChunkPos::new(-3, 2));
        assert_eq!(pos.section_y(), -4);
        assert_eq!(pos.local_x(), 15);
        assert_eq!(pos.local_z(), 0);
    }

    #[test]
    fn region_coordinates_use_floor_division() {
        assert_eq!(ChunkPos::new(-1, -1).region_pos(), RegionPos::new(-1, -1));
        assert_eq!(ChunkPos::new(-32, 32).region_pos(), RegionPos::new(-1, 1));
        assert_eq!(ChunkPos::new(-33, 0).region_pos(), RegionPos::new(-2, 0));
    }

    #[test]
    fn chunk_contains_matches_boundaries() {
        let chunk = ChunkPos::new(0, 0);
        assert!(chunk.contains_block(BlockPos::new(0, 100, 0)));
        assert!(chunk.contains_block(BlockPos::new(15, -64, 15)));
        assert!(!chunk.contains_block(BlockPos::new(16, 0, 0)));
        assert!(!chunk.contains_block(BlockPos::new(0, 0, -1)));
        assert_eq!(BlockPos::new(16, 0, -1).chunk_pos(), ChunkPos::new(1, -1));
    }
}
