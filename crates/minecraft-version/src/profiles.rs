//! Built-in version profiles.
//!
//! A profile is only listed here once its layout, bounds, component
//! requirements and capabilities are backed by this repository's codecs and
//! fixtures. The baseline profile is deliberately the only one for now.

use crate::{
    build_version_profile, ProfileBuildInput, SupportLevel, VersionId, VersionProfile,
    WorldCapabilities,
};

/// Stable id of the verified 1.21.4 classic Anvil profile.
pub const JAVA_CLASSIC_1_21_4_ID: &str = "java-classic-1.21.4";

/// Returns the verified Minecraft Java 1.21.4 classic Anvil profile.
pub fn java_classic_1_21_4() -> VersionProfile {
    build_version_profile(ProfileBuildInput {
        version: version("1.21.4"),
        data_version: 4189,
        java_major: 21,
        resource_pack_format: Some(46),
        datapack_format: Some(61),
        capabilities: WorldCapabilities {
            support_level: SupportLevel::Full,
            read: true,
            edit: true,
            write: true,
            export: true,
            generate: true,
            assets: true,
        },
        notes: vec![
            "Block-state and biome writes are verified for classic Anvil 1.21.4.".to_owned(),
            "The client JAR is the source of the full block and item catalog.".to_owned(),
            "Heightmaps and light are preserved or invalidated, never recalculated.".to_owned(),
        ],
    })
    .expect("the baseline profile must be valid")
}

/// All profiles that this build can currently offer.
pub fn builtin_profiles() -> Vec<VersionProfile> {
    vec![java_classic_1_21_4()]
}

/// Returns the built-in profile that owns a Minecraft `DataVersion`.
pub fn profile_for_data_version(data_version: i32) -> Option<VersionProfile> {
    builtin_profiles()
        .into_iter()
        .find(|profile| profile.data_version == data_version)
}

/// Returns the built-in profile for an exact Minecraft version id.
pub fn profile_for_version(version: &VersionId) -> Option<VersionProfile> {
    builtin_profiles()
        .into_iter()
        .find(|profile| profile.minecraft_version == *version)
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
        ItemModelFormat, ReleaseType, VersionFamily, WorldLayout,
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
