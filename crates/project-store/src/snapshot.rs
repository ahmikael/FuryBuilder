use minecraft_version::{profile_for_data_version, BiomeFormat, VersionProfile, WorldLayout};
use world_domain::{
    BiomeSection, BiomeState, BlockState, ChunkPos, Section, World, BIOME_CELLS, MAX_BIOME_PALETTE,
};

use crate::error::ProjectError;

pub const SNAPSHOT_FORMAT_VERSION: u16 = 3;
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

fn layout_id(layout: WorldLayout) -> &'static str {
    match layout {
        WorldLayout::ClassicAnvil => "classic-anvil",
        WorldLayout::SplitAnvil => "split-anvil",
        WorldLayout::McRegion => "mc-region",
        WorldLayout::Unknown => "unknown",
    }
}

fn biome_mode_id(format: BiomeFormat) -> &'static str {
    match format {
        BiomeFormat::Palette4x4x4 => "palette-4x4x4",
        BiomeFormat::Column => "column",
        BiomeFormat::Section => "section",
        BiomeFormat::Unknown => "unknown",
    }
}

fn snapshot_profile(world: &World) -> Result<VersionProfile, ProjectError> {
    let version = world.version();
    profile_for_data_version(version.data_version())
        .filter(|profile| {
            profile.capabilities.write
                && profile.minecraft_version.as_str() == version.minecraft_version()
        })
        .ok_or_else(|| {
            ProjectError::UnsupportedWorldVersion(format!(
                "Minecraft {} (DataVersion {})",
                version.minecraft_version(),
                version.data_version()
            ))
        })
}

/// A snapshot can only be written when its dimension set and vertical bounds are exactly
/// those declared by the project's version profile.
fn validate_world_dimensions(world: &World, profile: &VersionProfile) -> Result<(), ProjectError> {
    if world.dimensions().len() != profile.dimension_profiles.len() {
        return Err(ProjectError::corrupt(format!(
            "world has {} dimensions but profile `{}` declares {}",
            world.dimensions().len(),
            profile.id,
            profile.dimension_profiles.len()
        )));
    }
    for expected in &profile.dimension_profiles {
        let dimension = world.dimension(&expected.id).map_err(|_| {
            ProjectError::corrupt(format!(
                "world is missing profile dimension `{}`",
                expected.id
            ))
        })?;
        if dimension.min_y() != expected.min_y || dimension.height() != expected.height {
            return Err(ProjectError::corrupt(format!(
                "dimension `{}` bounds do not match profile `{}`",
                expected.id, profile.id
            )));
        }
    }
    Ok(())
}

/// Deterministic binary encoding used by project snapshots. BTreeMap iteration makes the
/// byte stream stable for the same world, which lets save verification compare a decode
/// and re-encode byte-for-byte.
pub fn encode_world(world: &World) -> Result<Vec<u8>, ProjectError> {
    let profile = snapshot_profile(world)?;
    validate_world_dimensions(world, &profile)?;
    let mut writer = Writer::new();
    writer.bytes.extend_from_slice(SNAPSHOT_MAGIC);
    writer.u16(SNAPSHOT_FORMAT_VERSION);
    writer.u64(world.revision());
    writer.string(&profile.id)?;
    writer.i32(profile.data_version);
    writer.string(layout_id(profile.world_layout))?;
    writer.string(biome_mode_id(profile.biome_format))?;
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
    if version != SNAPSHOT_FORMAT_VERSION {
        return Err(ProjectError::UnsupportedSchema {
            found: version as u32,
            supported: SNAPSHOT_FORMAT_VERSION as u32,
        });
    }
    let revision = reader.u64()?;
    let profile_id = reader.string()?;
    let data_version = reader.i32()?;
    let layout = reader.string()?;
    let biome_mode = reader.string()?;
    let profile = profile_for_data_version(data_version)
        .filter(|profile| profile.capabilities.write && profile.id == profile_id)
        .ok_or_else(|| {
            ProjectError::UnsupportedWorldVersion(format!(
                "snapshot profile `{profile_id}` (DataVersion {data_version})"
            ))
        })?;
    if layout != layout_id(profile.world_layout) {
        return Err(ProjectError::corrupt(format!(
            "snapshot layout `{layout}` does not match profile `{}`",
            profile.id
        )));
    }
    if biome_mode != biome_mode_id(profile.biome_format) {
        return Err(ProjectError::corrupt(format!(
            "snapshot biome mode `{biome_mode}` does not match profile `{}`",
            profile.id
        )));
    }
    let dimensions = reader.u32()?;
    if dimensions > MAX_DIMENSIONS {
        return Err(ProjectError::corrupt("snapshot has too many dimensions"));
    }

    let mut world = World::for_profile(&profile);
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
                                ProjectError::corrupt(format!("invalid biome state `{encoded}`"))
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
    use world_domain::{BlockPos, ChunkPos, Section, WorldVersion};

    fn stone() -> BlockState {
        "minecraft:stone".parse().expect("stone state")
    }

    fn plains() -> BiomeState {
        "minecraft:plains".parse().expect("plains biome")
    }

    fn v3_header(profile_id: &str, data_version: i32, layout: &str, biome_mode: &str) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.bytes.extend_from_slice(SNAPSHOT_MAGIC);
        writer.u16(3);
        writer.u64(1);
        writer.string(profile_id).expect("profile id");
        writer.i32(data_version);
        writer.string(layout).expect("layout");
        writer.string(biome_mode).expect("biome mode");
        writer.u32(0);
        writer.finish()
    }

    #[test]
    fn round_trips_biome_cells_and_profile_header_in_v3() {
        let mut world = World::new_empty();
        let section = Section::single(stone()).with_biomes(BiomeSection::uniform(plains()));
        world
            .install_section("minecraft:overworld", ChunkPos::new(0, 0), 0, Some(section))
            .expect("install section");

        let bytes = encode_world(&world).expect("encode v3");
        let mut reader = Reader::new(&bytes);
        assert_eq!(reader.take(SNAPSHOT_MAGIC.len()).unwrap(), SNAPSHOT_MAGIC);
        assert_eq!(reader.u16().unwrap(), 3);
        assert_eq!(reader.u64().unwrap(), 0);
        assert_eq!(reader.string().unwrap(), "java-classic-1.21.4");
        assert_eq!(reader.i32().unwrap(), 4189);
        assert_eq!(reader.string().unwrap(), "classic-anvil");
        assert_eq!(reader.string().unwrap(), "palette-4x4x4");

        let decoded = decode_world(&bytes).expect("decode v3");
        assert_eq!(decoded.version().minecraft_version(), "1.21.4");
        assert_eq!(decoded.version().data_version(), 4189);
        assert_eq!(
            decoded
                .get_biome("minecraft:overworld", BlockPos::new(5, 9, 15))
                .expect("biome"),
            Some(plains())
        );
        assert_eq!(encode_world(&decoded).expect("re-encode"), bytes);
    }

    #[test]
    fn unsupported_world_versions_cannot_be_encoded() {
        let mut world = World::new_empty();
        world.set_version(WorldVersion::new("1.20.6", 3839).expect("world version"));
        match encode_world(&world) {
            Err(ProjectError::UnsupportedWorldVersion(_)) => {}
            other => panic!("expected unsupported world version, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_dimension_bounds_must_match_the_profile() {
        let header = |dimension: &str, min_y: i32, height: i32| {
            let mut writer = Writer::new();
            writer.bytes.extend_from_slice(SNAPSHOT_MAGIC);
            writer.u16(3);
            writer.u64(0);
            writer.string("java-classic-1.21.4").expect("profile id");
            writer.i32(4189);
            writer.string("classic-anvil").expect("layout");
            writer.string("palette-4x4x4").expect("biome mode");
            writer.u32(1);
            writer.string(dimension).expect("dimension id");
            writer.i32(min_y);
            writer.i32(height);
            writer.u32(0);
            writer.finish()
        };

        let valid = decode_world(&header("minecraft:overworld", -64, 384)).expect("valid bounds");
        assert_eq!(
            valid
                .dimension("minecraft:overworld")
                .expect("overworld")
                .max_y(),
            319
        );

        assert!(matches!(
            decode_world(&header("minecraft:overworld", 0, 256)),
            Err(ProjectError::Corrupt(_))
        ));
        assert!(matches!(
            decode_world(&header("custom:missing", -64, 384)),
            Err(ProjectError::Corrupt(_))
        ));
    }

    #[test]
    fn profile_header_is_validated() {
        let valid = v3_header(
            "java-classic-1.21.4",
            4189,
            "classic-anvil",
            "palette-4x4x4",
        );
        let world = decode_world(&valid).expect("valid profile header");
        assert_eq!(world.version().minecraft_version(), "1.21.4");
        assert_eq!(world.version().data_version(), 4189);

        let wrong_layout = v3_header("java-classic-1.21.4", 4189, "split-anvil", "palette-4x4x4");
        assert!(matches!(
            decode_world(&wrong_layout),
            Err(ProjectError::Corrupt(_))
        ));

        let wrong_biomes = v3_header("java-classic-1.21.4", 4189, "classic-anvil", "column");
        assert!(matches!(
            decode_world(&wrong_biomes),
            Err(ProjectError::Corrupt(_))
        ));

        let unknown_profile = v3_header(
            "java-classic-1.21.5",
            4325,
            "classic-anvil",
            "palette-4x4x4",
        );
        assert!(matches!(
            decode_world(&unknown_profile),
            Err(ProjectError::UnsupportedWorldVersion(_))
        ));
    }
}
