//! On-disk chunk layout, selected independently of a profile's support level.
//! All currently cataloged modern classic releases share this layout; their
//! dimension bounds are supplied separately.

use std::ops::Range;

use minecraft_version::{BiomeFormat, ChunkFamily, VersionProfile, WorldLayout};
use world_domain::SECTION_SIZE;

use super::reader::AnvilError;
use crate::version_support;

pub(crate) struct ClassicChunkFormat {
    pub sections: &'static str,
    pub chunk_y: &'static str,
    pub section_y: &'static str,
    pub blocks: PaletteLayout,
    pub biomes: PaletteLayout,
    pub heightmaps: &'static str,
    pub post_processing: &'static str,
    pub light: LightLayout,
}

pub(crate) struct PaletteLayout {
    pub container: &'static str,
    pub palette: &'static str,
    pub data: &'static str,
    pub count: usize,
    minimum_bits: u8,
}

pub(crate) struct LightLayout {
    pub sky: &'static str,
    pub block: &'static str,
    pub enabled: &'static str,
    pub padding_sections: i32,
    pub bytes_per_section: usize,
}

const MODERN_CLASSIC: ClassicChunkFormat = ClassicChunkFormat {
    sections: "sections",
    chunk_y: "yPos",
    section_y: "Y",
    blocks: PaletteLayout {
        container: "block_states",
        palette: "palette",
        data: "data",
        count: super::chunk::BLOCK_SECTION_VOLUME,
        minimum_bits: 4,
    },
    biomes: PaletteLayout {
        container: "biomes",
        palette: "palette",
        data: "data",
        count: super::chunk::BIOME_SECTION_VOLUME,
        minimum_bits: 1,
    },
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
        match (
            profile.world_layout,
            profile.chunk_family,
            profile.biome_format,
        ) {
            (WorldLayout::ClassicAnvil, ChunkFamily::ModernPaletted, BiomeFormat::Palette4x4x4) => {
                Ok(&MODERN_CLASSIC)
            }
            _ => Err(AnvilError::UnsupportedLayout(format!(
                "no classic chunk format for profile `{}`",
                profile.id
            ))),
        }
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
        PaddedLongs::for_max_value(height as usize, 1)
            .word_count((SECTION_SIZE * SECTION_SIZE) as usize)
    }
}

impl PaletteLayout {
    pub(crate) fn packing(&self, palette_len: usize) -> PaddedLongs {
        PaddedLongs::for_max_value(palette_len - 1, self.minimum_bits)
    }
}

/// Values are stored low bits first and never cross a 64-bit word boundary.
/// This concerns the Java wire format, not the editor's canonical packing.
pub(crate) struct PaddedLongs {
    pub bits: u8,
}

impl PaddedLongs {
    fn for_max_value(max: usize, minimum_bits: u8) -> Self {
        let bits = (usize::BITS - max.leading_zeros()) as u8;
        Self {
            bits: bits.max(minimum_bits),
        }
    }

    fn values_per_word(&self) -> usize {
        64 / self.bits as usize
    }

    pub(crate) fn word_count(&self, len: usize) -> usize {
        len.div_ceil(self.values_per_word())
    }

    pub(crate) fn pack(&self, indices: impl ExactSizeIterator<Item = u16>) -> Vec<i64> {
        let mut words = vec![0; self.word_count(indices.len())];
        let slots = self.values_per_word();
        for (index, value) in indices.enumerate() {
            let shift = (index % slots) * self.bits as usize;
            words[index / slots] |= (value as i64) << shift;
        }
        words
    }

    /// `words` must contain the exact size returned by `word_count`.
    pub(crate) fn unpack<'a>(
        &self,
        words: &'a [i64],
        len: usize,
    ) -> impl Iterator<Item = u16> + 'a {
        let slots = self.values_per_word();
        let bits = self.bits as usize;
        let mask = (1u64 << bits) - 1;
        (0..len).map(move |i| {
            let shift = (i % slots) * bits;
            ((words[i / slots] as u64 >> shift) & mask) as u16
        })
    }
}

#[cfg(test)]
mod tests {
    use minecraft_version::{builtin_profiles, default_profile};

    use super::*;

    #[test]
    fn cataloged_classic_releases_select_the_shared_format_without_enabling_io() {
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
}
