//! Built-in version profiles.
//!
//! Metadata and editor capabilities are separate: a known release has exact
//! game metadata, but only codec-backed profiles may be used for projects.

use std::sync::LazyLock;

use crate::{
    build_version_profile, PackFormatMinors, ProfileBuildInput, SupportLevel, VersionFamily,
    VersionId, VersionProfile, WorldCapabilities,
};

/// Stable id of the verified 1.21.4 classic Anvil profile.
pub const JAVA_CLASSIC_1_21_4_ID: &str = "java-classic-1.21.4";

struct ReleaseFacts {
    version: &'static str,
    data_version: i32,
    java_major: u32,
    resource_pack_format: u32,
    datapack_format: u32,
    pack_format_minors: Option<PackFormatMinors>,
}

impl ReleaseFacts {
    const fn new(
        version: &'static str,
        data_version: i32,
        java_major: u32,
        resource_pack_format: u32,
        datapack_format: u32,
    ) -> Self {
        Self {
            version,
            data_version,
            java_major,
            resource_pack_format,
            datapack_format,
            pack_format_minors: None,
        }
    }

    const fn with_pack_minors(mut self, resource: u32, data: u32) -> Self {
        self.pack_format_minors = Some(PackFormatMinors { resource, data });
        self
    }

    fn profile(&self) -> VersionProfile {
        let version_id = version(self.version);
        let full = self.version == "1.21.4";
        let modern_classic = version_id.family() == VersionFamily::Modern;
        let flattened_classic = version_id.family() == VersionFamily::Flattened;
        let notes = if full {
            vec![
                "Block-state and biome writes are verified for classic Anvil 1.21.4.".to_owned(),
                "The client JAR is the source of the full block and item catalog.".to_owned(),
                "Heightmaps and light are preserved or invalidated, never recalculated.".to_owned(),
            ]
        } else if modern_classic {
            vec![
                format!(
                    "Classic Anvil import, block/biome editing, project snapshots and same-version export are verified for Minecraft {}.",
                    self.version
                ),
                format!(
                    "Vanilla generation with the official Minecraft {} server JAR is enabled for this release.",
                    self.version
                ),
                "The client JAR is the source of the full block and item catalog.".to_owned(),
            ]
        } else if flattened_classic {
            vec![
                format!(
                    "Flattened classic Anvil import, block/biome editing, project snapshots and same-version export are verified for Minecraft {}.",
                    self.version
                ),
                "Vanilla generation is not enabled for this release yet.".to_owned(),
                "The client JAR is the source of the full block and item catalog.".to_owned(),
            ]
        } else {
            vec![
                "Release metadata is known, but this build has no verified world adapter for it."
                    .to_owned(),
            ]
        };
        let capabilities = if full {
            WorldCapabilities {
                support_level: SupportLevel::Full,
                read: true,
                edit: true,
                write: true,
                export: true,
                generate: true,
                assets: true,
            }
        } else if modern_classic {
            WorldCapabilities {
                support_level: SupportLevel::ReadWriteSubset,
                read: true,
                edit: true,
                write: true,
                export: true,
                generate: true,
                assets: true,
            }
        } else if flattened_classic {
            WorldCapabilities {
                support_level: SupportLevel::ReadWriteSubset,
                read: true,
                edit: true,
                write: true,
                export: true,
                generate: false,
                assets: false,
            }
        } else {
            WorldCapabilities {
                support_level: SupportLevel::Unverified,
                read: false,
                edit: false,
                write: false,
                export: false,
                generate: false,
                assets: false,
            }
        };
        build_version_profile(ProfileBuildInput {
            version: version_id,
            data_version: self.data_version,
            java_major: self.java_major,
            resource_pack_format: Some(self.resource_pack_format),
            datapack_format: Some(self.datapack_format),
            pack_format_minors: self.pack_format_minors,
            capabilities,
            notes,
        })
        .expect("built-in Minecraft release facts must be valid")
    }
}

// Exact release ids and Java majors come from Mojang's version manifest and
// per-release launcher JSON; DataVersion and pack formats come from version.json
// inside each official client JAR. No JAR content is bundled.
const RELEASES: &[ReleaseFacts] = &[
    // Flattened classic Anvil. Pack formats are the shared resource/data values
    // declared by each release's version.json; 1.17.1 stays at format 7.
    ReleaseFacts::new("1.13", 1519, 8, 4, 4),
    ReleaseFacts::new("1.13.1", 1628, 8, 4, 4),
    ReleaseFacts::new("1.13.2", 1631, 8, 4, 4),
    ReleaseFacts::new("1.14", 1952, 8, 4, 4),
    ReleaseFacts::new("1.14.1", 1957, 8, 4, 4),
    ReleaseFacts::new("1.14.2", 1963, 8, 4, 4),
    ReleaseFacts::new("1.14.3", 1968, 8, 4, 4),
    ReleaseFacts::new("1.14.4", 1976, 8, 4, 4),
    ReleaseFacts::new("1.15", 2225, 8, 5, 5),
    ReleaseFacts::new("1.15.1", 2227, 8, 5, 5),
    ReleaseFacts::new("1.15.2", 2230, 8, 5, 5),
    ReleaseFacts::new("1.16", 2566, 8, 5, 5),
    ReleaseFacts::new("1.16.1", 2567, 8, 5, 5),
    ReleaseFacts::new("1.16.2", 2578, 8, 6, 6),
    ReleaseFacts::new("1.16.3", 2580, 8, 6, 6),
    ReleaseFacts::new("1.16.4", 2584, 8, 6, 6),
    ReleaseFacts::new("1.16.5", 2586, 8, 6, 6),
    ReleaseFacts::new("1.17", 2724, 16, 7, 7),
    ReleaseFacts::new("1.17.1", 2730, 16, 7, 7),
    // Modern classic Anvil. Pack/DataVersion metadata cross-checked with mcmeta.
    //                          id        DataVersion  Java  resource  data
    ReleaseFacts::new("1.18", 2860, 17, 8, 8),
    ReleaseFacts::new("1.18.1", 2865, 17, 8, 8),
    ReleaseFacts::new("1.18.2", 2975, 17, 8, 9),
    ReleaseFacts::new("1.19", 3105, 17, 9, 10),
    ReleaseFacts::new("1.19.1", 3117, 17, 9, 10),
    ReleaseFacts::new("1.19.2", 3120, 17, 9, 10),
    ReleaseFacts::new("1.19.3", 3218, 17, 12, 10),
    ReleaseFacts::new("1.19.4", 3337, 17, 13, 12),
    ReleaseFacts::new("1.20", 3463, 17, 15, 15),
    ReleaseFacts::new("1.20.1", 3465, 17, 15, 15),
    ReleaseFacts::new("1.20.2", 3578, 17, 18, 18),
    ReleaseFacts::new("1.20.3", 3698, 17, 22, 26),
    ReleaseFacts::new("1.20.4", 3700, 17, 22, 26),
    ReleaseFacts::new("1.20.5", 3837, 21, 32, 41),
    ReleaseFacts::new("1.20.6", 3839, 21, 32, 41),
    ReleaseFacts::new("1.21", 3953, 21, 34, 48),
    ReleaseFacts::new("1.21.1", 3955, 21, 34, 48),
    ReleaseFacts::new("1.21.2", 4080, 21, 42, 57),
    ReleaseFacts::new("1.21.3", 4082, 21, 42, 57),
    ReleaseFacts::new("1.21.4", 4189, 21, 46, 61),
    ReleaseFacts::new("1.21.5", 4325, 21, 55, 71),
    ReleaseFacts::new("1.21.6", 4435, 21, 63, 80),
    ReleaseFacts::new("1.21.7", 4438, 21, 64, 81),
    ReleaseFacts::new("1.21.8", 4440, 21, 64, 81),
    ReleaseFacts::new("1.21.9", 4554, 21, 69, 88).with_pack_minors(0, 0),
    ReleaseFacts::new("1.21.10", 4556, 21, 69, 88).with_pack_minors(0, 0),
    ReleaseFacts::new("1.21.11", 4671, 21, 75, 94).with_pack_minors(0, 1),
    // Split save layout. Pack/DataVersion metadata cross-checked with mcmeta.
    ReleaseFacts::new("26.1", 4786, 25, 84, 101).with_pack_minors(0, 1),
    ReleaseFacts::new("26.1.1", 4788, 25, 84, 101).with_pack_minors(0, 1),
    ReleaseFacts::new("26.1.2", 4790, 25, 84, 101).with_pack_minors(0, 1),
    ReleaseFacts::new("26.2", 4903, 25, 88, 107).with_pack_minors(0, 1),
    ReleaseFacts::new("26.3", 5023, 25, 97, 121).with_pack_minors(1, 0),
];

/// All built-in profiles, including releases whose adapters are not verified.
pub fn builtin_profiles() -> &'static [VersionProfile] {
    static PROFILES: LazyLock<Vec<VersionProfile>> =
        LazyLock::new(|| RELEASES.iter().map(ReleaseFacts::profile).collect());
    &PROFILES
}

/// Returns the verified Minecraft Java 1.21.4 classic Anvil profile.
pub fn java_classic_1_21_4() -> VersionProfile {
    profile_for_id(JAVA_CLASSIC_1_21_4_ID).expect("the default profile must be built in")
}

/// Returns the built-in profile that owns a Minecraft `DataVersion`.
pub fn profile_for_data_version(data_version: i32) -> Option<VersionProfile> {
    builtin_profiles()
        .iter()
        .find(|profile| profile.data_version == data_version)
        .cloned()
}

/// Returns the built-in profile with a stable profile id.
pub fn profile_for_id(id: &str) -> Option<VersionProfile> {
    builtin_profiles()
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
}

/// Returns the built-in profile for an exact Minecraft version id.
pub fn profile_for_version(version: &VersionId) -> Option<VersionProfile> {
    builtin_profiles()
        .iter()
        .find(|profile| profile.minecraft_version == *version)
        .cloned()
}

/// Returns the default profile for a new empty project.
pub fn default_profile() -> VersionProfile {
    java_classic_1_21_4()
}

fn version(value: &str) -> VersionId {
    value
        .parse()
        .expect("built-in Minecraft version ids must be valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BiomeFormat, BlockstateFormat, CatalogRegistryFormat, ChunkFamily, DimensionKind,
        ItemModelFormat, LevelTypeSyntax, ReleaseType, VersionFamily, WorldLayout,
    };

    #[test]
    fn baseline_profile_matches_1_21_4_facts() {
        let profile = java_classic_1_21_4();

        assert_eq!(profile.id, JAVA_CLASSIC_1_21_4_ID);
        assert_eq!(profile.minecraft_version.as_str(), "1.21.4");
        assert_eq!(profile.data_version, 4189);
        assert_eq!(profile.release_type, ReleaseType::Release);
        assert_eq!(profile.family, VersionFamily::Modern);
        assert_eq!(profile.world_layout, WorldLayout::ClassicAnvil);
        assert_eq!(profile.chunk_family, ChunkFamily::ModernPaletted);
        assert_eq!(profile.biome_format, BiomeFormat::Palette4x4x4);
        assert_eq!(profile.java.minimum_major, 21);
        assert_eq!(profile.java.recommended_major, 21);
        assert_eq!(profile.resource_pack_format, Some(46));
        assert_eq!(profile.datapack_format, Some(61));
    }

    #[test]
    fn profile_lookup_by_id_is_exact() {
        assert_eq!(
            profile_for_id(JAVA_CLASSIC_1_21_4_ID).map(|profile| profile.id),
            Some(JAVA_CLASSIC_1_21_4_ID.to_owned())
        );
        assert!(profile_for_id("java-classic-9.9.9").is_none());
    }

    #[test]
    fn baseline_dimensions_use_modern_bounds_and_folders() {
        let profile = java_classic_1_21_4();
        let dimensions = &profile.dimension_profiles;

        assert_eq!(dimensions.len(), 3);
        assert_eq!(dimensions[0].id, "minecraft:overworld");
        assert_eq!(dimensions[0].kind, DimensionKind::Overworld);
        assert_eq!(dimensions[0].min_y, -64);
        assert_eq!(dimensions[0].height, 384);
        assert_eq!(dimensions[0].folder, "region");
        assert_eq!(dimensions[1].id, "minecraft:the_nether");
        assert_eq!(dimensions[1].folder, "DIM-1/region");
        assert_eq!(dimensions[2].id, "minecraft:the_end");
        assert_eq!(dimensions[2].folder, "DIM1/region");
    }

    #[test]
    fn baseline_is_full_and_internally_consistent() {
        let profile = java_classic_1_21_4();

        assert_eq!(profile.capabilities.support_level, SupportLevel::Full);
        assert!(profile.capabilities.validate().is_ok());
        assert!(profile.capabilities.write);
        assert!(profile.capabilities.generate);
        assert!(profile.capabilities.assets);
    }

    #[test]
    fn baseline_catalog_is_named_and_uses_item_definitions() {
        let catalog = java_classic_1_21_4().content_catalog;

        assert_eq!(catalog.registry_format, CatalogRegistryFormat::Named);
        assert_eq!(catalog.blockstate_format, BlockstateFormat::Json);
        assert_eq!(catalog.item_model_format, ItemModelFormat::ItemDefinitions);
        assert!(catalog.creative_categories);
        assert!(catalog.data_components);
    }

    #[test]
    fn builtin_profile_ids_are_unique() {
        let profiles = builtin_profiles();
        let mut ids = profiles
            .iter()
            .map(|profile| profile.id.as_str())
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), profiles.len());
    }

    #[test]
    fn classic_release_metadata_covers_every_exact_version() {
        let profiles = builtin_profiles();
        assert_eq!(
            profiles
                .iter()
                .filter(|p| p.family == VersionFamily::Modern)
                .count(),
            27
        );
        for (minor, last_patch) in [(18, 2), (19, 4), (20, 6), (21, 11)] {
            for patch in 0..=last_patch {
                let release = if patch == 0 {
                    format!("1.{minor}")
                } else {
                    format!("1.{minor}.{patch}")
                };
                let version = version(&release);
                let profile = profile_for_version(&version).expect(&release);
                assert_eq!(profile.id, format!("java-classic-{version}"));
                assert_eq!(profile_for_id(&profile.id).as_ref(), Some(&profile));
                assert_eq!(
                    profile_for_data_version(profile.data_version).as_ref(),
                    Some(&profile)
                );
                assert_eq!(profile.family, VersionFamily::Modern);
                assert_eq!(profile.world_layout, WorldLayout::ClassicAnvil);
                assert_eq!(profile.biome_format, BiomeFormat::Palette4x4x4);
                assert_eq!(
                    profile
                        .dimension_profiles
                        .iter()
                        .map(|dimension| (
                            dimension.id.as_str(),
                            dimension.min_y,
                            dimension.height,
                            dimension.folder.as_str(),
                        ))
                        .collect::<Vec<_>>(),
                    [
                        ("minecraft:overworld", -64, 384, "region"),
                        ("minecraft:the_nether", 0, 256, "DIM-1/region"),
                        ("minecraft:the_end", 0, 256, "DIM1/region"),
                    ]
                );
            }
        }
        for id in [
            "1.18.0",
            "1.21.4-pre1",
            "1.21.12",
            "26.1-snapshot-6",
            "26.4",
        ] {
            assert!(profile_for_version(&version(id)).is_none(), "{id}");
        }
    }

    #[test]
    fn flattened_release_metadata_covers_every_exact_version() {
        #[derive(Clone, Copy)]
        struct Facts {
            version: &'static str,
            data_version: i32,
            java_major: u32,
            resource_pack_format: u32,
            datapack_format: u32,
            biome_format: BiomeFormat,
            level_type: LevelTypeSyntax,
            overworld: (i32, i32),
        }

        macro_rules! facts {
            ($version:expr, $data_version:expr, $java_major:expr, $resource:expr, $data:expr, $biome:expr, $level_type:expr, $overworld:expr $(,)?) => {
                Facts {
                    version: $version,
                    data_version: $data_version,
                    java_major: $java_major,
                    resource_pack_format: $resource,
                    datapack_format: $data,
                    biome_format: $biome,
                    level_type: $level_type,
                    overworld: $overworld,
                }
            };
        }

        const RELEASES: [Facts; 19] = [
            facts!(
                "1.13",
                1519,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.13.1",
                1628,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.13.2",
                1631,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.14",
                1952,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.14.1",
                1957,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.14.2",
                1963,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.14.3",
                1968,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.14.4",
                1976,
                8,
                4,
                4,
                BiomeFormat::Column,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.15",
                2225,
                8,
                5,
                5,
                BiomeFormat::Section,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.15.1",
                2227,
                8,
                5,
                5,
                BiomeFormat::Section,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.15.2",
                2230,
                8,
                5,
                5,
                BiomeFormat::Section,
                LevelTypeSyntax::LegacyCamelCase,
                (0, 256),
            ),
            facts!(
                "1.16",
                2566,
                8,
                5,
                5,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.16.1",
                2567,
                8,
                5,
                5,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.16.2",
                2578,
                8,
                6,
                6,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.16.3",
                2580,
                8,
                6,
                6,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.16.4",
                2584,
                8,
                6,
                6,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.16.5",
                2586,
                8,
                6,
                6,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.17",
                2724,
                16,
                7,
                7,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
            facts!(
                "1.17.1",
                2730,
                16,
                7,
                7,
                BiomeFormat::Section,
                LevelTypeSyntax::ModernNamespaced,
                (0, 256),
            ),
        ];

        for facts in RELEASES {
            let id = facts.version;
            let profile = profile_for_version(&version(id)).expect(id);
            assert_eq!(profile.id, format!("java-classic-{id}"), "{id}");
            assert_eq!(profile.data_version, facts.data_version, "{id}");
            assert_eq!(profile.family, VersionFamily::Flattened, "{id}");
            assert_eq!(profile.world_layout, WorldLayout::ClassicAnvil, "{id}");
            assert_eq!(profile.chunk_family, ChunkFamily::SectionBiomes, "{id}");
            assert_eq!(profile.biome_format, facts.biome_format, "{id}");
            assert_eq!(profile.java.minimum_major, facts.java_major, "{id}");
            assert_eq!(profile.java.recommended_major, facts.java_major, "{id}");
            assert_eq!(
                profile.resource_pack_format,
                Some(facts.resource_pack_format),
                "{id}"
            );
            assert_eq!(profile.datapack_format, Some(facts.datapack_format), "{id}");
            assert_eq!(profile.pack_format_minors, None, "{id}");
            assert_eq!(profile.server.level_type_syntax, facts.level_type, "{id}");
            assert!(profile.server.official_jar, "{id}");
            assert!(!profile.server.generation.normal, "{id}");
            assert_eq!(
                profile.content_catalog.registry_format,
                CatalogRegistryFormat::Named,
                "{id}"
            );
            assert_eq!(
                profile.content_catalog.blockstate_format,
                BlockstateFormat::Json,
                "{id}"
            );
            assert_eq!(
                profile.content_catalog.item_model_format,
                ItemModelFormat::ItemModels,
                "{id}"
            );
            assert!(!profile.content_catalog.data_components, "{id}");
            assert_eq!(
                profile
                    .dimension_profiles
                    .iter()
                    .map(|dimension| (
                        dimension.id.as_str(),
                        dimension.min_y,
                        dimension.height,
                        dimension.folder.as_str(),
                    ))
                    .collect::<Vec<_>>(),
                [
                    (
                        "minecraft:overworld",
                        facts.overworld.0,
                        facts.overworld.1,
                        "region"
                    ),
                    ("minecraft:the_nether", 0, 256, "DIM-1/region"),
                    ("minecraft:the_end", 0, 256, "DIM1/region"),
                ],
                "{id}"
            );
            assert_eq!(
                profile.capabilities.support_level,
                SupportLevel::ReadWriteSubset,
                "{id}"
            );
            assert!(profile.capabilities.read, "{id}");
            assert!(profile.capabilities.edit, "{id}");
            assert!(profile.capabilities.write, "{id}");
            assert!(profile.capabilities.export, "{id}");
            assert!(!profile.capabilities.generate, "{id}");
            assert!(!profile.capabilities.assets, "{id}");
            assert!(profile.capabilities.validate().is_ok(), "{id}");
            assert_eq!(
                profile_for_data_version(facts.data_version).as_ref(),
                Some(&profile),
                "{id}"
            );
        }
    }

    #[test]
    fn split_release_metadata_does_not_admit_world_io() {
        for (id, dv, pack, data, resource_minor, data_minor) in [
            ("26.1", 4786, 84, 101, 0, 1),
            ("26.1.1", 4788, 84, 101, 0, 1),
            ("26.1.2", 4790, 84, 101, 0, 1),
            ("26.2", 4903, 88, 107, 0, 1),
            ("26.3", 5023, 97, 121, 1, 0),
        ] {
            let profile = profile_for_version(&version(id)).expect(id);
            assert_eq!(profile_for_data_version(dv).as_ref(), Some(&profile));
            assert_eq!(profile.id, format!("java-split-{id}"));
            assert_eq!(profile.world_layout, WorldLayout::SplitAnvil);
            assert_eq!(profile.java.minimum_major, 25);
            assert_eq!(profile.resource_pack_format, Some(pack));
            assert_eq!(profile.datapack_format, Some(data));
            assert_eq!(
                profile.pack_format_minors,
                Some(PackFormatMinors {
                    resource: resource_minor,
                    data: data_minor
                })
            );
            assert_eq!(profile.capabilities.support_level, SupportLevel::Unverified);
            assert!(!profile.capabilities.read);
            assert!(!profile.capabilities.write);
        }
    }

    #[test]
    fn java_pack_and_item_format_boundaries_are_exact() {
        for (id, dv, java, resource, data, minors, models, components) in [
            (
                "1.18.2",
                2975,
                17,
                8,
                9,
                None,
                ItemModelFormat::ItemModels,
                false,
            ),
            (
                "1.20.4",
                3700,
                17,
                22,
                26,
                None,
                ItemModelFormat::ItemModels,
                false,
            ),
            (
                "1.20.5",
                3837,
                21,
                32,
                41,
                None,
                ItemModelFormat::ItemModels,
                true,
            ),
            (
                "1.21.3",
                4082,
                21,
                42,
                57,
                None,
                ItemModelFormat::ItemModels,
                true,
            ),
            (
                "1.21.4",
                4189,
                21,
                46,
                61,
                None,
                ItemModelFormat::ItemDefinitions,
                true,
            ),
            (
                "1.21.9",
                4554,
                21,
                69,
                88,
                Some(PackFormatMinors {
                    resource: 0,
                    data: 0,
                }),
                ItemModelFormat::ItemDefinitions,
                true,
            ),
            (
                "1.21.11",
                4671,
                21,
                75,
                94,
                Some(PackFormatMinors {
                    resource: 0,
                    data: 1,
                }),
                ItemModelFormat::ItemDefinitions,
                true,
            ),
        ] {
            let profile = profile_for_version(&version(id)).expect(id);
            assert_eq!(profile.data_version, dv, "{id}");
            assert_eq!(profile.java.minimum_major, java, "{id}");
            assert_eq!(profile.java.recommended_major, java, "{id}");
            assert_eq!(profile.resource_pack_format, Some(resource), "{id}");
            assert_eq!(profile.datapack_format, Some(data), "{id}");
            assert_eq!(profile.pack_format_minors, minors, "{id}");
            assert_eq!(profile.content_catalog.item_model_format, models, "{id}");
            assert_eq!(profile.content_catalog.data_components, components, "{id}");
        }
    }

    #[test]
    fn metadata_only_releases_do_not_grant_editor_capabilities() {
        for profile in builtin_profiles()
            .iter()
            .filter(|profile| profile.family == VersionFamily::Split)
        {
            assert_eq!(profile.capabilities.support_level, SupportLevel::Unverified);
            assert!(profile.capabilities.validate().is_ok());
            assert!(!profile.structure_formats.java_nbt.import);
            assert!(!profile.structure_formats.java_nbt.export);
            assert!(profile.server.official_jar);
            assert!(!profile.server.generation.normal);
            assert!(!profile.server.generation.flat);
            assert!(!profile.server.generation.large_biomes);
            assert!(!profile.server.generation.amplified);
        }
    }

    #[test]
    fn modern_classic_releases_admit_import_editing_snapshots_export_and_generation() {
        for profile in builtin_profiles()
            .iter()
            .filter(|profile| profile.family == VersionFamily::Modern)
        {
            assert!(profile.capabilities.validate().is_ok(), "{}", profile.id);
            assert!(profile.capabilities.read, "{} read", profile.id);
            assert!(profile.capabilities.edit, "{} edit", profile.id);
            assert!(profile.capabilities.write, "{} write", profile.id);
            assert!(profile.capabilities.export, "{} export", profile.id);
            assert!(profile.capabilities.assets, "{} assets", profile.id);
            assert!(profile.capabilities.generate, "{} generate", profile.id);
            assert!(profile.server.official_jar, "{} server", profile.id);
            assert_eq!(
                profile.server.level_type_syntax,
                crate::LevelTypeSyntax::ModernNamespaced,
                "{} level-type",
                profile.id
            );
            assert!(profile.server.generation.normal, "{} normal", profile.id);
            assert!(profile.server.generation.flat, "{} flat", profile.id);
            assert!(
                profile.server.generation.large_biomes,
                "{} large biomes",
                profile.id
            );
            assert!(
                profile.server.generation.amplified,
                "{} amplified",
                profile.id
            );
            if profile.id == JAVA_CLASSIC_1_21_4_ID {
                assert_eq!(profile.capabilities.support_level, SupportLevel::Full);
                assert!(profile.structure_formats.java_nbt.import);
                assert!(profile.structure_formats.java_nbt.export);
            } else {
                assert_eq!(
                    profile.capabilities.support_level,
                    SupportLevel::ReadWriteSubset
                );
                assert!(
                    !profile.structure_formats.java_nbt.import,
                    "{} structures stay in P2",
                    profile.id
                );
                assert!(
                    !profile.structure_formats.java_nbt.export,
                    "{} structures stay in P2",
                    profile.id
                );
            }
        }
    }

    #[test]
    fn every_builtin_profile_is_complete_and_consistent() {
        for profile in builtin_profiles() {
            assert!(!profile.id.is_empty(), "profile id");
            assert_eq!(
                profile.release_type,
                profile.minecraft_version.release_type(),
                "{}",
                profile.id
            );
            assert_eq!(
                profile.family,
                profile.minecraft_version.family(),
                "{}",
                profile.id
            );
            assert!(
                profile.capabilities.validate().is_ok(),
                "{} has inconsistent capabilities",
                profile.id
            );
            assert!(
                !profile.dimension_profiles.is_empty(),
                "{} has no dimensions",
                profile.id
            );

            let mut dimension_ids = std::collections::HashSet::new();
            for dimension in &profile.dimension_profiles {
                assert!(dimension.height > 0, "{} height", dimension.id);
                assert_eq!(
                    dimension.height % 16,
                    0,
                    "{} section alignment",
                    dimension.id
                );
                assert!(
                    dimension_ids.insert(dimension.id.as_str()),
                    "{} has duplicate dimension {}",
                    profile.id,
                    dimension.id
                );
                assert!(!dimension.folder.is_empty(), "{} folder", dimension.id);
            }

            if profile.capabilities.support_level == SupportLevel::Full {
                assert!(profile.capabilities.edit, "{} full edit", profile.id);
                assert!(profile.capabilities.write, "{} full write", profile.id);
            }
        }
    }

    #[test]
    fn baseline_profile_serializes_with_verified_values() {
        let value = serde_json::to_value(java_classic_1_21_4()).expect("profile serializes");

        assert_eq!(value["id"], JAVA_CLASSIC_1_21_4_ID);
        assert_eq!(value["minecraftVersion"], "1.21.4");
        assert_eq!(value["dataVersion"], 4189);
        assert_eq!(value["dimensionProfiles"][0]["minY"], -64);
        assert_eq!(value["java"]["recommendedMajor"], 21);
        assert_eq!(value["resourcePackFormat"], 46);
        assert_eq!(value["datapackFormat"], 61);
        assert_eq!(value["capabilities"]["supportLevel"], "full");
        assert_eq!(
            value["contentCatalog"]["itemModelFormat"],
            "item-definitions"
        );
    }
}
