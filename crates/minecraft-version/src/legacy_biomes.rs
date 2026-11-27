//! Numeric biome registry for the pre-flattening Java releases (1.8–1.12.2).
//!
//! Numeric worlds store one biome id per block column in `Level.Biomes`. The
//! ids are the same registry used by 1.13–1.17, but the pre-1.13 names are
//! exposed with modern flattened names: id 8 is `minecraft:nether_wastes`, not
//! the 1.13 `minecraft:nether` alias. ID 127 (`the_void`) only exists from 1.9;
//! ids 40–50 and 168+ were introduced after the numeric world format.
//!
//! This is data only. Unknown ids are rejected by the chunk decoder instead of
//! being replaced with a default biome.

use crate::flattened_biomes::FlattenedBiomeRegistry;
use crate::{VersionFamily, VersionId, VersionKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyBiomeRegistry {
    inner: FlattenedBiomeRegistry,
    series: u32,
}

impl LegacyBiomeRegistry {
    /// Returns the registry for an exact Java release from 1.8 through 1.12.2.
    pub fn for_release(version: &VersionId) -> Option<Self> {
        if !version.is_release() || version.family() != VersionFamily::Numeric {
            return None;
        }
        let VersionKind::Release { numbers } = &version.kind else {
            return None;
        };
        let [1, series @ 8..=12, ..] = numbers.as_slice() else {
            return None;
        };
        Some(Self {
            inner: FlattenedBiomeRegistry::from_series(*series),
            series: *series,
        })
    }

    /// Returns the modern name of a numeric biome id that exists in the release.
    pub fn name(self, id: i32) -> Option<&'static str> {
        if !self.is_valid_id(id) {
            return None;
        }
        if id == 8 {
            return Some("minecraft:nether_wastes");
        }
        self.inner.name(id)
    }

    pub fn id(self, name: &str) -> Option<i32> {
        (0..=167).find(|id| self.name(*id) == Some(name))
    }

    /// Pre-1.13 saves only contain the base ids, their mutation ids, and the
    /// void after 1.8. 40–50, 168+ and the gaps in between belong to later
    /// releases and would otherwise be read from the shared 1.12 registry.
    fn is_valid_id(self, id: i32) -> bool {
        matches!(
            id,
            0..=39 | 129..=134 | 140 | 149 | 151 | 155..=158 | 160..=167
        ) || (id == 127 && self.series >= 9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(id: &str) -> VersionId {
        id.parse().expect("version id")
    }

    #[test]
    fn only_exact_numeric_releases_get_the_registry() {
        for id in ["1.8", "1.8.9", "1.9.4", "1.10.2", "1.11.2", "1.12.2"] {
            assert!(
                LegacyBiomeRegistry::for_release(&version(id)).is_some(),
                "{id}"
            );
        }
        for id in ["1.7.10", "1.13", "1.12-pre4", "1.21.4"] {
            assert!(
                LegacyBiomeRegistry::for_release(&version(id)).is_none(),
                "{id}"
            );
        }
    }

    #[test]
    fn names_use_pre_flattening_biome_ids_and_modern_names() {
        let one_eight = LegacyBiomeRegistry::for_release(&version("1.8.9")).unwrap();
        assert_eq!(one_eight.name(0), Some("minecraft:ocean"));
        assert_eq!(one_eight.name(3), Some("minecraft:mountains"));
        assert_eq!(one_eight.name(8), Some("minecraft:nether_wastes"));
        assert_eq!(one_eight.name(127), None);
        assert_eq!(
            one_eight.name(167),
            Some("minecraft:modified_badlands_plateau")
        );

        let one_12 = LegacyBiomeRegistry::for_release(&version("1.12.2")).unwrap();
        assert_eq!(one_12.name(127), Some("minecraft:the_void"));
        assert_eq!(one_12.name(168), None);
        assert_eq!(one_12.id("minecraft:nether_wastes"), Some(8));
        assert_eq!(one_12.id("minecraft:the_void"), Some(127));
        assert_eq!(one_12.id("minecraft:deep_dark"), None);
    }

    #[test]
    fn later_biome_ids_are_not_read_from_pre_1_13_saves() {
        let registry = LegacyBiomeRegistry::for_release(&version("1.12.2")).unwrap();
        for id in 40..=50 {
            assert_eq!(registry.name(id), None, "{id}");
        }
        assert_eq!(registry.name(128), None);
        for id in 135..=139 {
            assert_eq!(registry.name(id), None, "{id}");
        }
        for id in 141..=148 {
            assert_eq!(registry.name(id), None, "{id}");
        }
        assert_eq!(registry.name(150), None);
        for id in 152..=154 {
            assert_eq!(registry.name(id), None, "{id}");
        }
        assert_eq!(registry.name(159), None);
        for id in 168..=175 {
            assert_eq!(registry.name(id), None, "{id}");
        }
        assert_eq!(registry.name(-1), None);
    }

    #[test]
    fn every_supported_id_round_trips() {
        for (release, count) in [("1.8.9", 61), ("1.9.4", 62), ("1.12.2", 62)] {
            let registry = LegacyBiomeRegistry::for_release(&version(release)).unwrap();
            let names = (0..=167)
                .filter_map(|id| registry.name(id))
                .collect::<Vec<_>>();
            assert_eq!(names.len(), count, "{release}");
            for name in names {
                assert_eq!(
                    registry.name(registry.id(name).expect(name)),
                    Some(name),
                    "{release}: {name}"
                );
            }
        }
    }
}
