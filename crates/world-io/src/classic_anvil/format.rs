//! On-disk chunk layout, selected independently of a profile's support level.
//! Modern classic releases share section storage, but not every field's
//! encoding or registry. Dimension bounds are supplied separately.

use std::ops::Range;

use minecraft_version::{modern_classic::ModernClassicRelease, VersionProfile};
use world_domain::SECTION_SIZE;

use super::reader::AnvilError;
use crate::version_support;

pub(crate) const DATA_VERSION: &str = "DataVersion";

pub(crate) struct ClassicChunkFormat {
    pub x_pos: &'static str,
    pub z_pos: &'static str,
    pub chunk_y: &'static str,
    pub status: &'static str,
    pub full_status: &'static str,
    pub sections: &'static str,
    pub section_y: &'static str,
    pub blocks: PaletteLayout,
    pub biomes: PaletteLayout,
    pub block_entities: &'static str,
    pub entities: &'static str,
    pub heightmaps: &'static str,
    pub post_processing: &'static str,
    pub light: LightLayout,
}

pub(crate) struct PaletteLayout {
    pub container: &'static str,
    pub palette: &'static str,
    pub data: &'static str,
    pub count: usize,
    pub(super) minimum_bits: u8,
    pub(super) packing: LongPacking,
}

/// How palette indices are packed into the Java `LongArray`.
///
/// 1.13–1.15 used a continuous bit stream where one value may cross a
/// 64-bit boundary. 1.16 changed the on-disk block-state array to pad each
/// word ("no value crosses a long"); the heightmap array gained the same
/// padding in 1.16. Values are always least-significant-bit first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LongPacking {
    Padded,
    Unpadded,
}

pub(crate) struct LightLayout {
    pub sky: &'static str,
    pub block: &'static str,
    pub enabled: &'static str,
    pub padding_sections: i32,
    pub bytes_per_section: usize,
}

const MODERN_CLASSIC: ClassicChunkFormat = ClassicChunkFormat {
    x_pos: "xPos",
    z_pos: "zPos",
    chunk_y: "yPos",
    status: "Status",
    full_status: "minecraft:full",
    sections: "sections",
    section_y: "Y",
    blocks: PaletteLayout {
        container: "block_states",
        palette: "palette",
        data: "data",
        count: super::chunk::BLOCK_SECTION_VOLUME,
        minimum_bits: 4,
        packing: LongPacking::Padded,
    },
    biomes: PaletteLayout {
        container: "biomes",
        palette: "palette",
        data: "data",
        count: super::chunk::BIOME_SECTION_VOLUME,
        minimum_bits: 1,
        packing: LongPacking::Padded,
    },
    block_entities: "block_entities",
    entities: "entities",
    heightmaps: "Heightmaps",
    post_processing: "PostProcessing",
    light: LightLayout {
        sky: "SkyLight",
        block: "BlockLight",
        enabled: "isLightOn",
        padding_sections: 1,
        bytes_per_section: super::chunk::BLOCK_SECTION_VOLUME / 2,
    },
};

// 1.18–1.19 store the chunk status registry path without its namespace.
const BARE_STATUS_CLASSIC: ClassicChunkFormat = ClassicChunkFormat {
    full_status: "full",
    ..MODERN_CLASSIC
};

impl ClassicChunkFormat {
    pub(crate) fn for_read(data_version: i32) -> Result<&'static Self, AnvilError> {
        let profile = version_support::read_profile(data_version).ok_or_else(|| {
            AnvilError::UnsupportedDataVersion {
                found: data_version,
                supported: version_support::supported_data_versions(),
            }
        })?;
        Self::for_profile(profile)
    }

    pub(crate) fn for_profile(profile: &VersionProfile) -> Result<&'static Self, AnvilError> {
        let release = ModernClassicRelease::for_profile(profile).ok_or_else(|| {
            AnvilError::UnsupportedLayout(format!(
                "no classic chunk format for profile `{}`",
                profile.id
            ))
        })?;
        Ok(if release.uses_namespaced_chunk_status() {
            &MODERN_CLASSIC
        } else {
            &BARE_STATUS_CLASSIC
        })
    }

    pub(crate) fn is_full(&self, status: Option<&str>) -> bool {
        status == Some(self.full_status)
    }

    pub(crate) fn section_range(&self, min_y: i32, height: i32) -> Option<Range<i32>> {
        if min_y % SECTION_SIZE != 0 || height <= 0 || height % SECTION_SIZE != 0 {
            return None;
        }
        let start = min_y / SECTION_SIZE;
        let end = start.checked_add(height / SECTION_SIZE)?;
        // Serialized section Y is a signed byte, including the light halo.
        i8::try_from(start - self.light.padding_sections).ok()?;
        i8::try_from(end + self.light.padding_sections - 1).ok()?;
        Some(start..end)
    }

    pub(crate) fn heightmap_words(&self, height: i32) -> usize {
        // One value per column. Zero means no block; otherwise the value is
        // top Y + 1 relative to the bottom, so height itself must fit.
        PackedLongs::new(height as usize, 1, LongPacking::Padded)
            .word_count((SECTION_SIZE * SECTION_SIZE) as usize)
    }
}

impl PaletteLayout {
    pub(crate) fn packing(&self, palette_len: usize) -> PackedLongs {
        PackedLongs::new(palette_len - 1, self.minimum_bits, self.packing)
    }
}

/// Java palette indices packed least-significant-bit first. The padded form
/// never crosses a 64-bit word; the unpadded form is one continuous stream.
/// This concerns the on-disk format, not the editor's canonical packing.
pub(crate) struct PackedLongs {
    pub bits: u8,
    packing: LongPacking,
}

impl PackedLongs {
    pub(crate) fn new(max: usize, minimum_bits: u8, packing: LongPacking) -> Self {
        let bits = (usize::BITS - max.leading_zeros()) as u8;
        Self {
            bits: bits.max(minimum_bits),
            packing,
        }
    }

    fn values_per_word(&self) -> usize {
        64 / self.bits as usize
    }

    pub(crate) fn word_count(&self, len: usize) -> usize {
        match self.packing {
            LongPacking::Padded => len.div_ceil(self.values_per_word()),
            LongPacking::Unpadded => (len * self.bits as usize).div_ceil(64),
        }
    }

    pub(crate) fn pack(&self, indices: impl ExactSizeIterator<Item = u16>) -> Vec<i64> {
        let len = indices.len();
        let mut words = vec![0i64; self.word_count(len)];
        match self.packing {
            LongPacking::Padded => {
                let slots = self.values_per_word();
                for (index, value) in indices.enumerate() {
                    let shift = (index % slots) * self.bits as usize;
                    words[index / slots] |= (value as i64) << shift;
                }
            }
            LongPacking::Unpadded => {
                let bits = self.bits as usize;
                for (index, value) in indices.enumerate() {
                    let bit = index * bits;
                    let word = bit / 64;
                    let shift = bit % 64;
                    let value = u64::from(value);
                    words[word] |= (value << shift) as i64;
                    if shift + bits > 64 {
                        words[word + 1] |= (value >> (64 - shift)) as i64;
                    }
                }
            }
        }
        words
    }

    /// `words` must contain the exact size returned by `word_count`.
    pub(crate) fn unpack<'a>(
        &self,
        words: &'a [i64],
        len: usize,
    ) -> impl Iterator<Item = u16> + 'a {
        let bits = self.bits as usize;
        let mask = (1u64 << bits) - 1;
        let slots = self.values_per_word();
        let packing = self.packing;
        (0..len).map(move |index| match packing {
            LongPacking::Padded => {
                let shift = (index % slots) * bits;
                ((words[index / slots] as u64 >> shift) & mask) as u16
            }
            LongPacking::Unpadded => {
                let bit = index * bits;
                let word = bit / 64;
                let shift = bit % 64;
                let mut value = words[word] as u64 >> shift;
                if shift + bits > 64 {
                    value |= (words[word + 1] as u64) << (64 - shift);
                }
                (value & mask) as u16
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use minecraft_version::{builtin_profiles, default_profile, ChunkFamily, WorldLayout};

    use super::*;

    #[test]
    fn cataloged_classic_releases_select_the_shared_format() {
        for profile in builtin_profiles().iter().filter(|profile| {
            profile.world_layout == WorldLayout::ClassicAnvil
                && profile.chunk_family == ChunkFamily::ModernPaletted
        }) {
            let layout = ClassicChunkFormat::for_profile(profile)
                .unwrap_or_else(|error| panic!("{}: {error}", profile.minecraft_version.as_str()));
            assert_eq!(layout.sections, "sections");
            assert_eq!(layout.blocks.packing(2).bits, 4);
            assert_eq!(layout.biomes.packing(2).bits, 1);
            assert_eq!(layout.light.padding_sections, 1);
            assert_eq!(layout.light.bytes_per_section, 2048);
            for dimension in &profile.dimension_profiles {
                let sections = if dimension.id == "minecraft:overworld" {
                    -4..20
                } else {
                    0..16
                };
                assert_eq!(
                    layout.section_range(dimension.min_y, dimension.height),
                    Some(sections)
                );
                assert_eq!(layout.heightmap_words(dimension.height), 37);
            }
            assert_eq!(
                ClassicChunkFormat::for_read(profile.data_version).is_ok(),
                profile.capabilities.read
            );
        }
        let mut profile = default_profile();
        profile.world_layout = WorldLayout::SplitAnvil;
        assert!(ClassicChunkFormat::for_profile(&profile).is_err());
        profile.world_layout = WorldLayout::ClassicAnvil;
        profile.chunk_family = ChunkFamily::SectionBiomes;
        assert!(ClassicChunkFormat::for_profile(&profile).is_err());
    }

    #[test]
    fn only_the_releases_own_status_spelling_is_fully_generated() {
        for (version, full, wrong) in [
            ("1.18.2", "full", "minecraft:full"),
            ("1.19.4", "full", "minecraft:full"),
            ("1.20", "minecraft:full", "full"),
            ("1.21.11", "minecraft:full", "full"),
        ] {
            let profile =
                minecraft_version::profile_for_version(&version.parse().unwrap()).unwrap();
            let format = ClassicChunkFormat::for_profile(&profile).unwrap();
            assert_eq!(format.status, "Status");
            assert!(format.is_full(Some(full)), "{version}");
            for status in [
                None,
                Some(wrong),
                Some("example:full"),
                Some("minecraft:light"),
            ] {
                assert!(!format.is_full(status), "{version}: {status:?}");
            }
        }
    }

    #[test]
    fn padded_longs_leave_unused_bits_at_word_edges() {
        let packing = MODERN_CLASSIC.blocks.packing(17);
        assert_eq!(packing.bits, 5);
        let indices = [16; 13];
        let words = packing.pack(indices.into_iter());
        assert_eq!(words, [0x0842_1084_2108_4210, 16]);
        assert_eq!(
            packing.unpack(&words, indices.len()).collect::<Vec<_>>(),
            indices
        );
        assert_eq!(MODERN_CLASSIC.heightmap_words(128), 32);
        assert_eq!(MODERN_CLASSIC.heightmap_words(256), 37);
        assert_eq!(MODERN_CLASSIC.heightmap_words(384), 37);
    }

    #[test]
    fn unpadded_longs_are_one_continuous_bit_stream() {
        let packing = PackedLongs::new(16, 4, LongPacking::Unpadded);
        assert_eq!(packing.bits, 5);
        assert_eq!(packing.word_count(4096), 320);
        let indices = (0..4096)
            .map(|index| (index % 17) as u16)
            .collect::<Vec<_>>();
        let words = packing.pack(indices.iter().copied());
        assert_eq!(words.len(), 320);
        assert_eq!(
            packing.unpack(&words, indices.len()).collect::<Vec<_>>(),
            indices
        );

        let padded = PackedLongs::new(16, 4, LongPacking::Padded);
        assert_eq!(padded.word_count(4096), 342);
        let words = padded.pack(indices.iter().copied());
        assert_eq!(words.len(), 342);
        assert_eq!(
            padded.unpack(&words, indices.len()).collect::<Vec<_>>(),
            indices
        );
    }
}
