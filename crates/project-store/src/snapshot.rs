use world_domain::{
    BiomeSection, BiomeState, BlockState, ChunkPos, Section, World, BIOME_CELLS, MAX_BIOME_PALETTE,
};

use crate::error::ProjectError;

pub const SNAPSHOT_FORMAT_VERSION: u16 = 2;
/// Older snapshots without biome data remain readable.
pub const SNAPSHOT_MIN_VERSION: u16 = 1;
pub const SNAPSHOT_MAGIC: &[u8; 8] = b"FURYSNAP";
pub const MAX_SNAPSHOT_BYTES: usize = 1024 * 1024 * 1024;
const MAX_STRING_BYTES: usize = 512;
const MAX_DIMENSIONS: u32 = 32;
const MAX_CHUNKS: u32 = 1_000_000;
const MAX_SECTIONS_PER_CHUNK: u32 = 256;
const MAX_PALETTE: u32 = 4096;
const MAX_WORDS: u32 = 1024;

struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(64 * 1024),
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) -> Result<(), ProjectError> {
        if value.len() > MAX_STRING_BYTES {
            return Err(ProjectError::LimitExceeded(format!(
                "snapshot string exceeds {MAX_STRING_BYTES} bytes"
            )));
        }
        self.u16(value.len() as u16);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProjectError> {
        if self.remaining() < count {
            return Err(ProjectError::corrupt("snapshot ended unexpectedly"));
        }
        let slice = &self.bytes[self.position..self.position + count];
        self.position += count;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, ProjectError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProjectError> {
        let bytes: [u8; 2] = self.take(2)?.try_into().expect("length checked");
        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, ProjectError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("length checked");
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, ProjectError> {
        let bytes: [u8; 8] = self.take(8)?.try_into().expect("length checked");
        Ok(u64::from_le_bytes(bytes))
    }

    fn i32(&mut self) -> Result<i32, ProjectError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().expect("length checked");
        Ok(i32::from_le_bytes(bytes))
    }

    fn string(&mut self) -> Result<String, ProjectError> {
        let length = self.u16()? as usize;
        if length > MAX_STRING_BYTES {
            return Err(ProjectError::corrupt("snapshot string is too long"));
        }
        let bytes = self.take(length)?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| ProjectError::corrupt("snapshot string is not valid UTF-8"))
    }
}

/// Deterministic binary encoding used by project snapshots. BTreeMap iteration makes the
/// byte stream stable for the same world, which lets save verification compare a decode
/// and re-encode byte-for-byte.
pub fn encode_world(world: &World) -> Result<Vec<u8>, ProjectError> {
    let mut writer = Writer::new();
    writer.bytes.extend_from_slice(SNAPSHOT_MAGIC);
    writer.u16(SNAPSHOT_FORMAT_VERSION);
    writer.u64(world.revision());
    writer.u32(world.dimensions().len() as u32);
    for (id, dimension) in world.dimensions() {
        writer.string(id)?;
        writer.i32(dimension.min_y());
        writer.i32(dimension.height());
        writer.u32(dimension.chunks().len() as u32);
        for (chunk_pos, chunk) in dimension.chunks() {
            writer.i32(chunk_pos.x);
            writer.i32(chunk_pos.z);
            writer.u32(chunk.sections().len() as u32);
            for (section_y, section) in chunk.sections() {
                writer.i32(*section_y);
                writer.u8(section.bits_per_index());
                writer.u32(section.palette().len() as u32);
                for state in section.palette() {
                    writer.string(&state.to_string())?;
                }
                writer.u32(section.packed_data().len() as u32);
                for word in section.packed_data() {
                    writer.u64(*word);
                }
                match section.biomes() {
                    Some(biomes) => {
                        writer.u8(1);
                        writer.u16(biomes.palette().len() as u16);
                        for biome in biomes.palette() {
                            writer.string(biome.as_str())?;
                        }
                        for cell in biomes.cells() {
                            writer.u16(*cell);
                        }
                    }
                    None => writer.u8(0),
                }
            }
        }
    }
    let bytes = writer.finish();
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(ProjectError::LimitExceeded(format!(
            "snapshot exceeds {} bytes",
            MAX_SNAPSHOT_BYTES
        )));
    }
    Ok(bytes)
}

pub fn decode_world(bytes: &[u8]) -> Result<World, ProjectError> {
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        return Err(ProjectError::LimitExceeded(format!(
            "snapshot exceeds {} bytes",
            MAX_SNAPSHOT_BYTES
        )));
    }
    if bytes.len() < SNAPSHOT_MAGIC.len() + 2 {
        return Err(ProjectError::corrupt("snapshot is too short"));
    }
    let mut reader = Reader::new(bytes);
    let magic = reader.take(SNAPSHOT_MAGIC.len())?;
    if magic != SNAPSHOT_MAGIC {
        return Err(ProjectError::corrupt("snapshot magic does not match"));
    }
    let version = reader.u16()?;
    if !(SNAPSHOT_MIN_VERSION..=SNAPSHOT_FORMAT_VERSION).contains(&version) {
        return Err(ProjectError::UnsupportedSchema {
            found: version as u32,
            supported: SNAPSHOT_FORMAT_VERSION as u32,
        });
    }
    let revision = reader.u64()?;
    let dimensions = reader.u32()?;
    if dimensions > MAX_DIMENSIONS {
        return Err(ProjectError::corrupt("snapshot has too many dimensions"));
    }

    let mut world = World::new_empty();
    world.restore_revision(revision);
    for _ in 0..dimensions {
        let dimension_id = reader.string()?;
        let min_y = reader.i32()?;
        let height = reader.i32()?;
        let expected = world.dimension(&dimension_id).map_err(|_| {
            ProjectError::corrupt(format!(
                "snapshot contains unknown dimension `{dimension_id}`"
            ))
        })?;
        let (expected_min_y, expected_max_y) = (expected.min_y(), expected.max_y());
        if expected.min_y() != min_y || expected.height() != height {
            return Err(ProjectError::corrupt(format!(
                "dimension `{dimension_id}` bounds do not match the target version"
            )));
        }
        let chunk_count = reader.u32()?;
        if chunk_count > MAX_CHUNKS {
            return Err(ProjectError::corrupt("snapshot has too many chunks"));
        }
        for _ in 0..chunk_count {
            let x = reader.i32()?;
            let z = reader.i32()?;
            let chunk_pos = ChunkPos::new(x, z);
            let section_count = reader.u32()?;
            if section_count > MAX_SECTIONS_PER_CHUNK {
                return Err(ProjectError::corrupt("snapshot has too many sections"));
            }
            for _ in 0..section_count {
                let section_y = reader.i32()?;
                let bits = reader.u8()?;
                let palette_len = reader.u32()?;
                if palette_len == 0 || palette_len > MAX_PALETTE {
                    return Err(ProjectError::corrupt("snapshot palette size is invalid"));
                }
                let mut palette = Vec::with_capacity(palette_len as usize);
                for _ in 0..palette_len {
                    let encoded = reader.string()?;
                    let state: BlockState = encoded.parse().map_err(|_| {
                        ProjectError::corrupt(format!("invalid block state `{encoded}`"))
                    })?;
                    palette.push(state);
                }
                let word_count = reader.u32()?;
                if word_count > MAX_WORDS {
                    return Err(ProjectError::corrupt("snapshot index buffer is too large"));
                }
                let mut data = Vec::with_capacity(word_count as usize);
                for _ in 0..word_count {
                    data.push(reader.u64()?);
                }
                let mut section = Section::from_raw(palette, data, bits)
                    .ok_or_else(|| ProjectError::corrupt("snapshot section is invalid"))?;
                if version >= 2 {
                    match reader.u8()? {
                        0 => {}
                        1 => {
                            let palette_len = reader.u16()? as usize;
                            if palette_len == 0 || palette_len > MAX_BIOME_PALETTE {
                                return Err(ProjectError::corrupt(
                                    "snapshot biome palette size is invalid",
                                ));
                            }
                            let mut palette = Vec::with_capacity(palette_len);
                            for _ in 0..palette_len {
                                let encoded = reader.string()?;
                                let biome: BiomeState = encoded.parse().map_err(|_| {
                                    ProjectError::corrupt(format!(
                                        "invalid biome state `{encoded}`"
                                    ))
                                })?;
                                palette.push(biome);
                            }
                            let mut cells = Vec::with_capacity(BIOME_CELLS);
                            for _ in 0..BIOME_CELLS {
                                cells.push(reader.u16()?);
                            }
                            let biomes =
                                BiomeSection::from_palette(palette, cells).ok_or_else(|| {
                                    ProjectError::corrupt("snapshot biome section is invalid")
                                })?;
                            section.set_biomes(Some(biomes));
                        }
                        _ => {
                            return Err(ProjectError::corrupt(
                                "snapshot biome presence flag is invalid",
                            ))
                        }
                    }
                }
                let first_y = section_y
                    .checked_mul(16)
                    .ok_or_else(|| ProjectError::corrupt("section Y overflows"))?;
                let last_y = first_y
                    .checked_add(15)
                    .ok_or_else(|| ProjectError::corrupt("section Y overflows"))?;
                if first_y < expected_min_y || last_y > expected_max_y {
                    return Err(ProjectError::corrupt(format!(
                        "section {section_y} is outside dimension `{dimension_id}`"
                    )));
                }
                if !section.is_void() {
                    world
                        .install_section(&dimension_id, chunk_pos, section_y, Some(section))
                        .map_err(|error| {
                            ProjectError::corrupt(format!(
                                "section {section_y} cannot be installed: {error}"
                            ))
                        })?;
                }
            }
        }
    }
    if reader.remaining() != 0 {
        return Err(ProjectError::corrupt(
            "snapshot has trailing bytes after the declared payload",
        ));
    }
    Ok(world)
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_domain::{BlockPos, ChunkPos, Section};

    fn stone() -> BlockState {
        "minecraft:stone".parse().expect("stone state")
    }

    fn plains() -> BiomeState {
        "minecraft:plains".parse().expect("plains biome")
    }

    #[test]
    fn decodes_a_version_1_snapshot_without_biome_data() {
        let mut writer = Writer::new();
        writer.bytes.extend_from_slice(SNAPSHOT_MAGIC);
        writer.u16(1);
        writer.u64(7);
        writer.u32(1);
        writer.string("minecraft:overworld").expect("dimension");
        writer.i32(-64);
        writer.i32(384);
        writer.u32(1);
        writer.i32(0);
        writer.i32(0);
        writer.u32(1);
        writer.i32(0);
        writer.u8(0);
        writer.u32(1);
        writer.string("minecraft:stone").expect("palette");
        writer.u32(0);
        let bytes = writer.finish();

        let world = decode_world(&bytes).expect("v1 snapshot");
        assert_eq!(world.revision(), 7);
        assert_eq!(
            world
                .get_block("minecraft:overworld", BlockPos::new(0, 0, 0))
                .expect("block"),
            stone()
        );
        assert_eq!(
            world
                .get_biome("minecraft:overworld", BlockPos::new(0, 0, 0))
                .expect("biome"),
            None,
            "a version 1 snapshot has no biome data and must not invent any"
        );

        let upgraded = encode_world(&world).expect("upgrade snapshot");
        assert_eq!(u16::from_le_bytes([upgraded[8], upgraded[9]]), 2);
        let reloaded = decode_world(&upgraded).expect("v2 snapshot");
        assert_eq!(
            reloaded
                .get_biome("minecraft:overworld", BlockPos::new(0, 0, 0))
                .expect("biome"),
            None
        );
    }

    #[test]
    fn round_trips_biome_cells_in_a_version_2_snapshot() {
        let mut world = World::new_empty();
        let section = Section::single(stone()).with_biomes(BiomeSection::uniform(plains()));
        world
            .install_section("minecraft:overworld", ChunkPos::new(0, 0), 0, Some(section))
            .expect("install section");

        let bytes = encode_world(&world).expect("encode v2");
        let decoded = decode_world(&bytes).expect("decode v2");
        assert_eq!(
            decoded
                .get_biome("minecraft:overworld", BlockPos::new(5, 9, 15))
                .expect("biome"),
            Some(plains())
        );
        assert_eq!(encode_world(&decoded).expect("re-encode"), bytes);
    }
}
