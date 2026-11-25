//! Release-specific facts for the shared modern classic Anvil layout.
//!
//! Registry names are vanilla defaults, not a claim about a save's datapacks.
//! These facts describe on-disk data and never grant I/O capabilities.

use crate::{builtin_profiles, BiomeFormat, ChunkFamily, VersionKind, VersionProfile, WorldLayout};

#[derive(Debug, Clone, Copy)]
pub struct ModernClassicRelease {
    minor: u32,
    patch: u32,
}

impl ModernClassicRelease {
    /// Resolves only the exact stable identities covered by the built-in metadata.
    pub fn for_profile(profile: &VersionProfile) -> Option<Self> {
        if profile.world_layout != WorldLayout::ClassicAnvil
            || profile.chunk_family != ChunkFamily::ModernPaletted
            || profile.biome_format != BiomeFormat::Palette4x4x4
            || !builtin_profiles().iter().any(|known| {
                known.data_version == profile.data_version
                    && known.minecraft_version == profile.minecraft_version
            })
        {
            return None;
        }
        let VersionKind::Release { numbers } = &profile.minecraft_version.kind else {
            return None;
        };
        let [1, minor @ 18..=21, ..] = numbers.as_slice() else {
            return None;
        };
        Some(Self {
            minor: *minor,
            patch: numbers.get(2).copied().unwrap_or(0),
        })
    }

    /// `Status` was namespaced in 23w12a (first stable release: 1.20).
    pub fn uses_namespaced_chunk_status(self) -> bool {
        self.minor >= 20
    }

    /// Returns true for a namespaced ID in this release's vanilla biome registry.
    pub fn contains_biome(self, id: &str) -> bool {
        id.strip_prefix("minecraft:").is_some_and(|path| {
            BASE_BIOMES.binary_search(&path).is_ok() || self.added_biomes().contains(&path)
        })
    }

    fn added_biomes(self) -> &'static [&'static str] {
        let len = match (self.minor, self.patch) {
            (18, _) => 0,
            (19, _) => 2,
            (20, _) | (21, 0..=3) => 3,
            (21, _) => 4,
            _ => unreachable!("validated classic release"),
        };
        &ADDED_BIOMES[..len]
    }
}

// Cross-checked against the vanilla registry reports mirrored in Misode/mcmeta:
// <release>-registries/worldgen/biome/data.json, for all 27 built-in releases.
const ADDED_BIOMES: [&str; 4] = ["deep_dark", "mangrove_swamp", "cherry_grove", "pale_garden"];
const BASE_BIOMES: [&str; 61] = [
    "badlands",
    "bamboo_jungle",
    "basalt_deltas",
    "beach",
    "birch_forest",
    "cold_ocean",
    "crimson_forest",
    "dark_forest",
    "deep_cold_ocean",
    "deep_frozen_ocean",
    "deep_lukewarm_ocean",
    "deep_ocean",
    "desert",
    "dripstone_caves",
    "end_barrens",
    "end_highlands",
    "end_midlands",
    "eroded_badlands",
    "flower_forest",
    "forest",
    "frozen_ocean",
    "frozen_peaks",
    "frozen_river",
    "grove",
    "ice_spikes",
    "jagged_peaks",
    "jungle",
    "lukewarm_ocean",
    "lush_caves",
    "meadow",
    "mushroom_fields",
    "nether_wastes",
    "ocean",
    "old_growth_birch_forest",
    "old_growth_pine_taiga",
    "old_growth_spruce_taiga",
    "plains",
    "river",
    "savanna",
    "savanna_plateau",
    "small_end_islands",
    "snowy_beach",
    "snowy_plains",
    "snowy_slopes",
    "snowy_taiga",
    "soul_sand_valley",
    "sparse_jungle",
    "stony_peaks",
    "stony_shore",
    "sunflower_plains",
    "swamp",
    "taiga",
    "the_end",
    "the_void",
    "warm_ocean",
    "warped_forest",
    "windswept_forest",
    "windswept_gravelly_hills",
    "windswept_hills",
    "windswept_savanna",
    "wooded_badlands",
];

#[cfg(test)]
mod tests {
    use sha1::{Digest, Sha1};

    use super::*;
    use crate::profile_for_version;

    #[test]
    fn vanilla_biome_membership_changes_at_the_exact_release_boundaries() {
        for (id, count, namespaced) in [
            ("1.18.2", 61, false),
            ("1.19", 63, false),
            ("1.19.4", 63, false),
            ("1.20", 64, true),
            ("1.21.3", 64, true),
            ("1.21.4", 65, true),
            ("1.21.11", 65, true),
        ] {
            let profile = profile_for_version(&id.parse().unwrap()).unwrap();
            let release = ModernClassicRelease::for_profile(&profile).unwrap();
            let total = BASE_BIOMES
                .iter()
                .chain(&ADDED_BIOMES)
                .filter(|path| release.contains_biome(&format!("minecraft:{path}")))
                .count();
            assert_eq!(total, count, "{id}");
            assert_eq!(release.uses_namespaced_chunk_status(), namespaced, "{id}");
            assert!(release.contains_biome("minecraft:plains"));
            for (index, path) in ADDED_BIOMES.iter().enumerate() {
                assert_eq!(
                    release.contains_biome(&format!("minecraft:{path}")),
                    index < count - BASE_BIOMES.len(),
                    "{id}: {path}"
                );
            }
            assert!(!release.contains_biome("example:plains"));
            assert!(!release.contains_biome("minecraft:mountains"));
        }
    }

    #[test]
    fn rejects_hybrid_and_uncataloged_identities() {
        let mut profile = crate::default_profile();
        let other = profile_for_version(&"1.20.6".parse().unwrap()).unwrap();
        profile.data_version = other.data_version;
        assert!(ModernClassicRelease::for_profile(&profile).is_none());
        for version in ["1.21.4-pre1", "1.21.99", "26.1"] {
            profile.minecraft_version = version.parse().unwrap();
            assert!(ModernClassicRelease::for_profile(&profile).is_none());
        }
    }

    #[test]
    fn complete_biome_sets_match_vanilla_registry_reports_for_all_profiles() {
        // SHA-1 of the sorted paths joined with LF, with no trailing newline.
        let reported = [
            "50f091619b43facd384b4f96c00e3631934bd986",
            "5a10820387d2a460a0dac8d486e607ddd2816477",
            "0444762afdc8972481abe98e8df39ccc59170a29",
            "1aeabf1ebff7d0eda411b88f0f8e07195e52726d",
        ];
        for profile in builtin_profiles() {
            let release = ModernClassicRelease::for_profile(profile).unwrap();
            let expected = match profile.minecraft_version.as_str() {
                id if id.starts_with("1.18") => reported[0],
                id if id.starts_with("1.19") => reported[1],
                "1.21" | "1.21.1" | "1.21.2" | "1.21.3" => reported[2],
                id if id.starts_with("1.20") => reported[2],
                _ => reported[3],
            };
            let mut paths: Vec<_> = BASE_BIOMES
                .iter()
                .chain(&ADDED_BIOMES)
                .copied()
                .filter(|path| release.contains_biome(&format!("minecraft:{path}")))
                .collect();
            paths.sort_unstable();
            assert_eq!(
                format!("{:x}", Sha1::digest(paths.join("\n"))),
                expected,
                "{}",
                profile.minecraft_version,
            );
        }
    }
}
