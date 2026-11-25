//! Machine-readable support matrix for Java world data (M8.13).
//!
//! The matrix is deliberately conservative: it describes what this build actually
//! exposes, not what the underlying Java format could theoretically contain. A
//! category marked `preserved_raw` may survive a write, but FuryBuilder does not
//! interpret or recalculate it. The Import World wizard renders the same data.
//!
//! Keep this module in sync with [`crate::region_write::VERIFIED_WRITE_FIELDS`],
//! [`crate::world_selection`] and [`crate::derived`]. The tests at the bottom of this
//! file are the drift guard.

use serde::{Deserialize, Serialize};

use crate::version_support;

pub const SUPPORT_MATRIX_SCHEMA_VERSION: u32 = 1;

/// One supported, detected or out-of-scope Java world family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSupport {
    pub id: String,
    pub minecraft_versions: String,
    pub data_versions: String,
    pub layout: String,
    pub status: String,
    pub read: String,
    pub edit: String,
    pub write: String,
    pub passthrough: String,
    pub note: String,
}

/// One data category inside the supported classic world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySupport {
    pub id: String,
    pub label: String,
    pub read: String,
    pub edit: String,
    pub write: String,
    pub passthrough: String,
    pub note: String,
}

/// The complete matrix returned by the import analysis and shown in the wizard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportMatrix {
    pub schema_version: u32,
    pub versions: Vec<VersionSupport>,
    pub categories: Vec<CategorySupport>,
}

/// Builds the current support matrix.
pub fn support_matrix() -> SupportMatrix {
    SupportMatrix {
        schema_version: SUPPORT_MATRIX_SCHEMA_VERSION,
        versions: versions(),
        categories: categories()
            .iter()
            .map(|row| CategorySupport {
                id: row.id.to_owned(),
                label: row.label.to_owned(),
                read: row.read.to_owned(),
                edit: row.edit.to_owned(),
                write: row.write.to_owned(),
                passthrough: row.passthrough.to_owned(),
                note: row.note.to_owned(),
            })
            .collect(),
    }
}

struct CategoryRow {
    id: &'static str,
    label: &'static str,
    read: &'static str,
    edit: &'static str,
    write: &'static str,
    passthrough: &'static str,
    note: &'static str,
}

fn versions() -> Vec<VersionSupport> {
    let target = version_support::default_version_profile();
    vec![
        VersionSupport {
            id: format!("{}-classic", target.id),
            minecraft_versions: target.minecraft_version.to_string(),
            data_versions: target.data_version.to_string(),
            layout: "classic_anvil".to_owned(),
            status: "supported".to_owned(),
            read: "supported_subset".to_owned(),
            edit: "block_states_and_biomes".to_owned(),
            write: "verified_block_states_and_biomes".to_owned(),
            passthrough: "byte_for_byte".to_owned(),
            note: format!(
                "The only profile with verified decoding, block/biome editing, region writing and a complete-folder export plus official-server fixture: Minecraft {} (DataVersion {}).",
                target.minecraft_version, target.data_version
            ),
        },
        VersionSupport {
            id: "java-1.21.5-1.21.11-classic".to_owned(),
            minecraft_versions: "1.21.5-1.21.11".to_owned(),
            data_versions: "4325-4671".to_owned(),
            layout: "classic_anvil".to_owned(),
            status: "detected_unsupported".to_owned(),
            read: "version_refused".to_owned(),
            edit: "none".to_owned(),
            write: "none".to_owned(),
            passthrough: "none".to_owned(),
            note: format!(
                "Classic folder markers can be recognised, but the fixed-version reader refuses every DataVersion except {}.",
                target.data_version
            ),
        },
        VersionSupport {
            id: "java-26.1-26.3-split".to_owned(),
            minecraft_versions: "26.1-26.3".to_owned(),
            data_versions: "4786-5023".to_owned(),
            layout: "split_26_1_plus".to_owned(),
            status: "detected_unsupported".to_owned(),
            read: "layout_and_region_headers".to_owned(),
            edit: "none".to_owned(),
            write: "none".to_owned(),
            passthrough: "none".to_owned(),
            note: "Inspects level.dat, world generation settings, dimension folders, player storage paths and terrain region headers. Chunk payloads, project import, passthrough and export are not supported.".to_owned(),
        },
        VersionSupport {
            id: "java-26.4-snapshots".to_owned(),
            minecraft_versions: "26.4 snapshots".to_owned(),
            data_versions: "5119-5130+".to_owned(),
            layout: "split_26_1_plus".to_owned(),
            status: "out_of_scope".to_owned(),
            read: "none".to_owned(),
            edit: "none".to_owned(),
            write: "none".to_owned(),
            passthrough: "none".to_owned(),
            note: "Moving snapshot target; not supported.".to_owned(),
        },
        VersionSupport {
            id: "pre-1.13-legacy".to_owned(),
            minecraft_versions: "pre-1.13".to_owned(),
            data_versions: "varies".to_owned(),
            layout: "legacy_mcr".to_owned(),
            status: "detected_unsupported".to_owned(),
            read: "legacy_marker".to_owned(),
            edit: "none".to_owned(),
            write: "none".to_owned(),
            passthrough: "none".to_owned(),
            note: "Legacy .mcr markers are detected and refused; no McRegion chunk parser or writer exists.".to_owned(),
        },
        VersionSupport {
            id: "bedrock".to_owned(),
            minecraft_versions: "Bedrock Edition".to_owned(),
            data_versions: "n/a".to_owned(),
            layout: "bedrock".to_owned(),
            status: "out_of_scope".to_owned(),
            read: "none".to_owned(),
            edit: "none".to_owned(),
            write: "none".to_owned(),
            passthrough: "none".to_owned(),
            note: "Bedrock is a different format family and is out of scope for the Java Anvil adapter.".to_owned(),
        },
    ]
}

fn categories() -> &'static [CategoryRow] {
    &[
        CategoryRow {
            id: "level_metadata",
            label: "level.dat metadata",
            read: "supported",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "DataVersion, Version.Id, LevelName and WorldGenSettings are validated; level.dat is never rewritten by the block writer.",
        },
        CategoryRow {
            id: "region_containers",
            label: "Region containers",
            read: "supported",
            edit: "chunk_records_only",
            write: "verified_replaced_regions",
            passthrough: "byte_for_byte",
            note: "Untouched region files and untouched records are copied byte for byte; only dirty regions are rebuilt.",
        },
        CategoryRow {
            id: "block_states",
            label: "Block states and palettes",
            read: "supported",
            edit: "supported",
            write: "verified",
            passthrough: "byte_for_byte",
            note: "The only editable field set is sections[].block_states in fully generated minecraft:full chunks.",
        },
        CategoryRow {
            id: "biomes",
            label: "Biomes",
            read: "supported",
            edit: "supported",
            write: "verified",
            passthrough: "byte_for_byte",
            note: "4x4x4 biome cells are decoded, editable and verified through overlay/region/export; untouched cells and unknown section tags keep their source values.",
        },
        CategoryRow {
            id: "block_entities",
            label: "Block entities",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Counts are reported; records are preserved. Missing block entities are not synthesized, so placing chests/signs/barrels is not semantically supported.",
        },
        CategoryRow {
            id: "entities_in_chunks",
            label: "Chunk entities",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Legacy chunk entity lists are counted and preserved, not edited.",
        },
        CategoryRow {
            id: "entities_sidecar",
            label: "entities/ sidecars",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Sidecar containers are read schema-neutrally and copied byte for byte.",
        },
        CategoryRow {
            id: "scheduled_ticks",
            label: "Block and fluid ticks",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Stale ticks are preserved; new blocks do not receive scheduled ticks.",
        },
        CategoryRow {
            id: "heightmaps",
            label: "Heightmaps",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserve_or_invalidate",
            passthrough: "byte_for_byte",
            note: "Preserve keeps raw values; VanillaRebuild removes Heightmaps so the target engine rebuilds them.",
        },
        CategoryRow {
            id: "lighting",
            label: "SkyLight / BlockLight / isLightOn",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserve_or_invalidate",
            passthrough: "byte_for_byte",
            note: "Preserve keeps raw light; VanillaRebuild removes section light arrays and sets isLightOn to 0 for the engine.",
        },
        CategoryRow {
            id: "post_processing",
            label: "PostProcessing queue",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserve_or_invalidate",
            passthrough: "byte_for_byte",
            note: "Preserve keeps queued positions; VanillaRebuild removes them because they describe pre-edit geometry.",
        },
        CategoryRow {
            id: "poi",
            label: "poi/ sidecars",
            read: "inventoried",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "FuryBuilder never rewrites POI. One barrel POI was added by a server in M8.8, which is not a general guarantee.",
        },
        CategoryRow {
            id: "structures",
            label: "Structures and references",
            read: "not_exposed",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Raw starts/references are preserved; block edits do not update structure metadata.",
        },
        CategoryRow {
            id: "unknown_chunk_tags",
            label: "Unknown chunk tags",
            read: "preserved_raw",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Unknown tags are compared with the source and must stay unchanged.",
        },
        CategoryRow {
            id: "unknown_files",
            label: "Unknown files and directories",
            read: "preserved_raw",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Unknown metadata and folders are copied, not interpreted or dropped.",
        },
        CategoryRow {
            id: "playerdata_data_datapacks",
            label: "playerdata/, data/, datapacks/",
            read: "preserved_raw",
            edit: "not_exposed",
            write: "preserved_raw",
            passthrough: "byte_for_byte",
            note: "Copied as whole files/folders; no semantic validation of their internals is claimed.",
        },
        CategoryRow {
            id: "custom_dimensions",
            label: "Custom dimensions",
            read: "refused",
            edit: "not_supported",
            write: "not_supported",
            passthrough: "refused",
            note: "Classic worlds declaring custom dimensions are refused instead of silently ignoring them.",
        },
        CategoryRow {
            id: "session_lock",
            label: "session.lock",
            read: "detected",
            edit: "not_supported",
            write: "skipped",
            passthrough: "skipped",
            note: "A transient game lock; the only file deliberately skipped by passthrough and export.",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_version_is_supported_and_it_matches_the_catalog() {
        let matrix = support_matrix();
        let supported = matrix
            .versions
            .iter()
            .filter(|version| version.status == "supported")
            .collect::<Vec<_>>();
        let target = version_support::default_version_profile();
        assert_eq!(supported.len(), 1);
        assert_eq!(
            supported[0].minecraft_versions,
            target.minecraft_version.as_str()
        );
        assert_eq!(supported[0].data_versions, target.data_version.to_string());
        assert_eq!(supported[0].layout, "classic_anvil");
    }

    #[test]
    fn every_other_version_claims_no_block_writes() {
        let matrix = support_matrix();
        for version in &matrix.versions {
            if version.status == "supported" {
                continue;
            }
            assert_eq!(version.write, "none", "{}", version.id);
            assert_eq!(version.edit, "none", "{}", version.id);
        }
    }

    #[test]
    fn block_states_and_biomes_are_the_only_verified_write_categories() {
        let matrix = support_matrix();
        for id in ["block_states", "biomes"] {
            let row = matrix
                .categories
                .iter()
                .find(|category| category.id == id)
                .unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(row.edit, "supported", "{id}");
            assert_eq!(row.write, "verified", "{id}");
        }
        for category in &matrix.categories {
            if category.id == "block_states" || category.id == "biomes" {
                continue;
            }
            assert_ne!(category.write, "verified", "{}", category.id);
        }
    }

    #[test]
    fn every_row_has_a_note_and_the_derived_policy_is_explicit() {
        let matrix = support_matrix();
        for version in &matrix.versions {
            assert!(!version.note.is_empty(), "{}", version.id);
        }
        for category in &matrix.categories {
            assert!(!category.note.is_empty(), "{}", category.id);
        }
        for id in ["heightmaps", "lighting", "post_processing"] {
            let row = matrix
                .categories
                .iter()
                .find(|category| category.id == id)
                .unwrap_or_else(|| panic!("missing {id}"));
            assert_eq!(row.write, "preserve_or_invalidate", "{id}");
        }
    }

    #[test]
    fn matrix_serializes_with_camel_case_fields() {
        let value = serde_json::to_value(support_matrix()).expect("serialize matrix");
        assert_eq!(value["schemaVersion"], SUPPORT_MATRIX_SCHEMA_VERSION);
        assert!(value["versions"][0].get("minecraftVersions").is_some());
        assert!(value["categories"][0].get("passthrough").is_some());
    }
}
