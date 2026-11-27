//! Vanilla numeric biome registry for flattened Java releases before 1.18.
//!
//! These are names in the source release, not modern upgrade aliases. Numeric
//! IDs are stable across these releases except for new entries; ID 8 changes
//! names in 1.16. No modded IDs or snapshot registries are inferred.

use crate::{VersionId, VersionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlattenedBiomeRegistry {
    series: u32,
}

impl FlattenedBiomeRegistry {
    pub(crate) fn from_series(series: u32) -> Self {
        Self { series }
    }

    pub fn for_release(version: &VersionId) -> Option<Self> {
        let VersionKind::Release { numbers } = &version.kind else {
            return None;
        };
        let [1, series @ 13..=17, ..] = numbers.as_slice() else {
            return None;
        };
        Some(Self { series: *series })
    }

    /// Whether the save stores biome values at a 4×4×4 block resolution.
    pub fn is_volumetric(self) -> bool {
        self.series >= 15
    }

    pub fn name(self, id: i32) -> Option<&'static str> {
        match id {
            8 if self.series < 16 => Some("minecraft:nether"),
            168..=169 if self.series < 14 => None,
            170..=173 if self.series < 16 => None,
            174..=175 if self.series < 17 => None,
            0..=50 => Some(COMMON[id as usize]),
            _ => EXTENDED
                .binary_search_by_key(&id, |(id, _)| *id)
                .ok()
                .map(|index| EXTENDED[index].1),
        }
    }

    pub fn id(self, name: &str) -> Option<i32> {
        (0..COMMON.len() as i32)
            .chain(EXTENDED.iter().map(|(id, _)| *id))
            .find(|id| self.name(*id) == Some(name))
    }
}

// Save registry IDs cross-checked against the per-release PyMCTranslate
// int_maps and the IDs in Minecraft's ChunkHeightAndBiomeFix.
const COMMON: [&str; 51] = [
    "minecraft:ocean",
    "minecraft:plains",
    "minecraft:desert",
    "minecraft:mountains",
    "minecraft:forest",
    "minecraft:taiga",
    "minecraft:swamp",
    "minecraft:river",
    "minecraft:nether_wastes",
    "minecraft:the_end",
    "minecraft:frozen_ocean",
    "minecraft:frozen_river",
    "minecraft:snowy_tundra",
    "minecraft:snowy_mountains",
    "minecraft:mushroom_fields",
    "minecraft:mushroom_field_shore",
    "minecraft:beach",
    "minecraft:desert_hills",
    "minecraft:wooded_hills",
    "minecraft:taiga_hills",
    "minecraft:mountain_edge",
    "minecraft:jungle",
    "minecraft:jungle_hills",
    "minecraft:jungle_edge",
    "minecraft:deep_ocean",
    "minecraft:stone_shore",
    "minecraft:snowy_beach",
    "minecraft:birch_forest",
    "minecraft:birch_forest_hills",
    "minecraft:dark_forest",
    "minecraft:snowy_taiga",
    "minecraft:snowy_taiga_hills",
    "minecraft:giant_tree_taiga",
    "minecraft:giant_tree_taiga_hills",
    "minecraft:wooded_mountains",
    "minecraft:savanna",
    "minecraft:savanna_plateau",
    "minecraft:badlands",
    "minecraft:wooded_badlands_plateau",
    "minecraft:badlands_plateau",
    "minecraft:small_end_islands",
    "minecraft:end_midlands",
    "minecraft:end_highlands",
    "minecraft:end_barrens",
    "minecraft:warm_ocean",
    "minecraft:lukewarm_ocean",
    "minecraft:cold_ocean",
    "minecraft:deep_warm_ocean",
    "minecraft:deep_lukewarm_ocean",
    "minecraft:deep_cold_ocean",
    "minecraft:deep_frozen_ocean",
];

const EXTENDED: [(i32, &str); 30] = [
    (127, "minecraft:the_void"),
    (129, "minecraft:sunflower_plains"),
    (130, "minecraft:desert_lakes"),
    (131, "minecraft:gravelly_mountains"),
    (132, "minecraft:flower_forest"),
    (133, "minecraft:taiga_mountains"),
    (134, "minecraft:swamp_hills"),
    (140, "minecraft:ice_spikes"),
    (149, "minecraft:modified_jungle"),
    (151, "minecraft:modified_jungle_edge"),
    (155, "minecraft:tall_birch_forest"),
    (156, "minecraft:tall_birch_hills"),
    (157, "minecraft:dark_forest_hills"),
    (158, "minecraft:snowy_taiga_mountains"),
    (160, "minecraft:giant_spruce_taiga"),
    (161, "minecraft:giant_spruce_taiga_hills"),
    (162, "minecraft:modified_gravelly_mountains"),
    (163, "minecraft:shattered_savanna"),
    (164, "minecraft:shattered_savanna_plateau"),
    (165, "minecraft:eroded_badlands"),
    (166, "minecraft:modified_wooded_badlands_plateau"),
    (167, "minecraft:modified_badlands_plateau"),
    (168, "minecraft:bamboo_jungle"),
    (169, "minecraft:bamboo_jungle_hills"),
    (170, "minecraft:soul_sand_valley"),
    (171, "minecraft:crimson_forest"),
    (172, "minecraft:warped_forest"),
    (173, "minecraft:basalt_deltas"),
    (174, "minecraft:dripstone_caves"),
    (175, "minecraft:lush_caves"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_matches_release_families_and_round_trips_ids() {
        for (version, count, volumetric, nether) in [
            ("1.13.2", 73, false, "minecraft:nether"),
            ("1.14.4", 75, false, "minecraft:nether"),
            ("1.15.2", 75, true, "minecraft:nether"),
            ("1.16.5", 79, true, "minecraft:nether_wastes"),
            ("1.17.1", 81, true, "minecraft:nether_wastes"),
        ] {
            let registry = FlattenedBiomeRegistry::for_release(&version.parse().unwrap()).unwrap();
            assert_eq!(registry.is_volumetric(), volumetric);
            assert_eq!(registry.name(8), Some(nether));
            let ids: Vec<_> = (0..=175)
                .filter(|id| registry.name(*id).is_some())
                .collect();
            assert_eq!(ids.len(), count);
            for id in ids {
                assert_eq!(registry.id(registry.name(id).unwrap()), Some(id));
            }
            assert_eq!(registry.name(-1), None);
            assert_eq!(registry.name(128), None);
            assert_eq!(registry.name(256), None);
        }
        for version in ["1.12.2", "1.18", "1.17-pre1"] {
            assert!(FlattenedBiomeRegistry::for_release(&version.parse().unwrap()).is_none());
        }
    }
}
