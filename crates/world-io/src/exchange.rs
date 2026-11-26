//! Verified export/import orchestration on top of the Java structure adapter.
//!
//! `prepare_export` captures a world region, serializes it, then decodes it again and
//! compares both the semantic structure and the deterministic re-encoding. Import helpers
//! do the same in reverse, so a command can never report success for bytes that would not
//! round-trip. Nothing here writes files; `project_store` owns atomic publishing.

use std::collections::BTreeMap;

use minecraft_version::DimensionProfile;
use serde::{Deserialize, Serialize};
use world_domain::{BlockBox, BlockPos, World};

use crate::java_structure::{
    import_profile, parse, read_java_structure, write_java_structure, JavaStructure,
    JavaStructureError, StructureInspection,
};
use crate::structure::{Structure, StructureError};

/// Upper bound on non-empty sections scanned when deriving content bounds. 8,192 sections
/// is 33.5 million block checks, which keeps an accidental full-world scan bounded.
pub const MAX_SCAN_SECTIONS: usize = 8_192;
/// Upper bound on palette entries returned to the UI preview.
pub const MAX_PALETTE_PREVIEW: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaletteCount {
    pub state: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureImportPreview {
    pub format: String,
    pub data_version: i32,
    pub target_data_version: Option<i32>,
    pub target_minecraft_version: Option<String>,
    pub dimensions: Vec<DimensionProfile>,
    pub size: [i32; 3],
    pub volume: u64,
    pub palette_size: usize,
    /// Largest palette entries by block count, bounded by `MAX_PALETTE_PREVIEW`.
    pub palette: Vec<PaletteCount>,
    pub stored_blocks: usize,
    pub non_air_blocks: usize,
    pub entities: usize,
    pub block_entities: usize,
    pub unknown_tags: Vec<String>,
    pub warnings: Vec<String>,
    pub compatible: bool,
}

/// A captured and verified export. `bytes` are exactly what should be published.
#[derive(Debug, Clone)]
pub struct ExportBundle {
    pub structure: Structure,
    pub bytes: Vec<u8>,
    pub inspection: StructureInspection,
    pub warnings: Vec<String>,
    pub bounds: BlockBox,
}

pub fn palette_counts(structure: &Structure) -> Vec<PaletteCount> {
    let mut counts = vec![0u64; structure.palette.len()];
    for block in &structure.blocks {
        if let Some(count) = counts.get_mut(block.palette_index) {
            *count += 1;
        }
    }
    let mut entries: Vec<PaletteCount> = structure
        .palette
        .iter()
        .zip(counts)
        .map(|(state, count)| PaletteCount {
            state: state.to_string(),
            count,
        })
        .collect();
    entries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.state.cmp(&right.state))
    });
    entries.truncate(MAX_PALETTE_PREVIEW);
    entries
}

pub fn import_preview(bytes: &[u8]) -> Result<StructureImportPreview, JavaStructureError> {
    let parsed = parse(bytes)?;
    Ok(preview_from_structure(
        &parsed.structure,
        &parsed.inspection,
    ))
}

/// Builds a UI preview from an already-parsed structure and inspection. Used by import
/// commands so a file is not parsed twice.
pub fn preview_from_structure(
    structure: &Structure,
    inspection: &StructureInspection,
) -> StructureImportPreview {
    let target = import_profile(inspection.data_version);
    StructureImportPreview {
        format: inspection.format.to_owned(),
        data_version: inspection.data_version,
        target_data_version: inspection.target_data_version,
        target_minecraft_version: target.map(|profile| profile.minecraft_version.to_string()),
        dimensions: target.map_or_else(Vec::new, |profile| profile.dimension_profiles.clone()),
        size: inspection.size,
        volume: inspection.volume,
        palette_size: inspection.palette_size,
        palette: palette_counts(structure),
        stored_blocks: inspection.stored_blocks,
        non_air_blocks: inspection.non_air_blocks,
        entities: inspection.entities,
        block_entities: inspection.block_entities,
        unknown_tags: inspection.unknown_tags.clone(),
        warnings: inspection.warnings.clone(),
        compatible: inspection.compatible,
    }
}

/// Strict import with round-trip verification: parse, re-encode, decode again and compare.
/// Returns an error rather than accepting bytes whose canonical form differs.
pub fn read_verified_import(bytes: &[u8]) -> Result<JavaStructure, JavaStructureError> {
    let parsed = read_java_structure(bytes)?;
    let encoded = write_java_structure(&parsed.structure, "FuryBuilder")?;
    let reparsed = read_java_structure(&encoded)?;
    if reparsed.structure != parsed.structure {
        return Err(JavaStructureError::InvalidFormat(
            "structure does not survive a deterministic re-encode".to_owned(),
        ));
    }
    Ok(parsed)
}

/// Captures an inclusive region, serializes it, and verifies the bytes by re-reading and
/// re-encoding them. `None` bounds use the content bounds of the dimension.
pub fn prepare_export(
    world: &World,
    dimension: &str,
    bounds: Option<BlockBox>,
    author: &str,
) -> Result<ExportBundle, JavaStructureError> {
    let bounds = match bounds {
        Some(bounds) => bounds,
        None => content_bounds(world, dimension)
            .map_err(JavaStructureError::from)?
            .ok_or_else(|| {
                JavaStructureError::InvalidFormat(format!(
                    "dimension `{dimension}` has no non-air content to export"
                ))
            })?,
    };
    let structure = Structure::from_world(world, dimension, bounds, world.version().data_version())
        .map_err(JavaStructureError::from)?;
    let bytes = write_java_structure(&structure, author)?;
    let read = read_java_structure(&bytes)?;
    if read.structure != structure {
        return Err(JavaStructureError::InvalidFormat(
            "exported bytes decoded to a different structure".to_owned(),
        ));
    }
    let reencoded = write_java_structure(&read.structure, author)?;
    if reencoded != bytes {
        return Err(JavaStructureError::InvalidFormat(
            "exported bytes are not deterministically stable".to_owned(),
        ));
    }
    let mut warnings = vec![
        "Java structure files store block states only. FuryBuilder objects, build DSL, undo history and scene metadata are not included.".to_owned(),
        "Entities and block entities are not modeled by FuryBuilder and are not exported.".to_owned(),
        "Missing (air) positions are stored sparsely; Minecraft treats them as air when the structure is placed with the structure block.".to_owned(),
    ];
    if structure.non_air_blocks() == 0 {
        warnings.push("The selected region contains no non-air blocks.".to_owned());
    }
    Ok(ExportBundle {
        structure,
        bytes,
        inspection: read.inspection,
        warnings,
        bounds,
    })
}

/// Finds the inclusive bounds of every non-air block in a dimension by inspecting stored
/// sections only. Returns `None` for an empty dimension.
pub fn content_bounds(world: &World, dimension: &str) -> Result<Option<BlockBox>, StructureError> {
    let dimension = world
        .dimension(dimension)
        .map_err(|error| StructureError::World(error.to_string()))?;
    let mut min: Option<BlockPos> = None;
    let mut max: Option<BlockPos> = None;
    let mut scanned = 0usize;
    for (chunk_pos, chunk) in dimension.chunks() {
        for (section_y, section) in chunk.sections() {
            if section.non_air_count() == 0 {
                continue;
            }
            scanned += 1;
            if scanned > MAX_SCAN_SECTIONS {
                return Err(StructureError::World(format!(
                    "content spans more than {MAX_SCAN_SECTIONS} sections; select an explicit region"
                )));
            }
            let base_x = chunk_pos.x * 16;
            let base_y = section_y * 16;
            let base_z = chunk_pos.z * 16;
            for y in 0..16u8 {
                for z in 0..16u8 {
                    for x in 0..16u8 {
                        let Some(state) = section.get(x, y, z) else {
                            continue;
                        };
                        if state.is_air() {
                            continue;
                        }
                        let pos = BlockPos::new(
                            base_x + i32::from(x),
                            base_y + i32::from(y),
                            base_z + i32::from(z),
                        );
                        min = Some(match min {
                            Some(current) => BlockPos::new(
                                current.x.min(pos.x),
                                current.y.min(pos.y),
                                current.z.min(pos.z),
                            ),
                            None => pos,
                        });
                        max = Some(match max {
                            Some(current) => BlockPos::new(
                                current.x.max(pos.x),
                                current.y.max(pos.y),
                                current.z.max(pos.z),
                            ),
                            None => pos,
                        });
                    }
                }
            }
        }
    }
    match (min, max) {
        (Some(min), Some(max)) => BlockBox::new(min, max)
            .map(Some)
            .map_err(|error| StructureError::World(error.to_string())),
        _ => Ok(None),
    }
}

/// Counts stored blocks per palette index (used by previews and documentation tests).
pub fn palette_usage(structure: &Structure) -> BTreeMap<String, u64> {
    let mut usage = BTreeMap::new();
    for block in &structure.blocks {
        if let Some(state) = structure.palette.get(block.palette_index) {
            *usage.entry(state.to_string()).or_insert(0) += 1;
        }
    }
    usage
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_version_profile, JAVA_STRUCTURE_FORMAT};
    use minecraft_version::{builtin_profiles, VersionFamily};
    use world_domain::BlockState;

    fn sample_world() -> World {
        let mut world = World::new_empty();
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(-5, 60, -7),
                &"minecraft:stone".parse().unwrap(),
            )
            .unwrap();
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(3, 65, 2),
                &"minecraft:oak_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]"
                    .parse()
                    .unwrap(),
            )
            .unwrap();
        world
    }

    #[test]
    fn content_bounds_ignore_air_and_cover_negative_positions() {
        let world = sample_world();
        let bounds = content_bounds(&world, "minecraft:overworld")
            .unwrap()
            .unwrap();
        assert_eq!(bounds.min, BlockPos::new(-5, 60, -7));
        assert_eq!(bounds.max, BlockPos::new(3, 65, 2));
        assert!(content_bounds(&world, "minecraft:overworld")
            .unwrap()
            .is_some());
    }

    #[test]
    fn prepare_export_is_verified_and_deterministic() {
        let world = sample_world();
        let first = prepare_export(&world, "minecraft:overworld", None, "FuryBuilder").unwrap();
        let second = prepare_export(&world, "minecraft:overworld", None, "FuryBuilder").unwrap();
        assert_eq!(first.bytes, second.bytes);
        assert_eq!(first.structure, second.structure);
        assert_eq!(first.structure.non_air_blocks(), 2);
        assert!(first
            .warnings
            .iter()
            .any(|warning| warning.contains("Entities and block entities")));
        let read = read_verified_import(&first.bytes).unwrap();
        assert_eq!(read.structure, first.structure);
    }

    #[test]
    fn each_flattened_release_round_trips_a_structure_in_its_exact_profile() {
        let profiles = builtin_profiles()
            .iter()
            .filter(|profile| profile.family == VersionFamily::Flattened);
        for profile in profiles {
            let mut world = World::for_profile(profile);
            let dimension = "minecraft:overworld";
            let position = BlockPos::new(-1, 255, 2);
            let state: BlockState =
                "minecraft:oak_stairs[facing=east,half=top,shape=straight,waterlogged=false]"
                    .parse()
                    .unwrap();
            world.set_block(dimension, position, &state).unwrap();

            let export = prepare_export(&world, dimension, None, "FuryBuilder").unwrap();
            let preview = import_preview(&export.bytes).unwrap();
            assert_eq!(preview.data_version, profile.data_version);
            assert_eq!(preview.target_data_version, Some(profile.data_version));
            assert_eq!(
                preview.target_minecraft_version.as_deref(),
                Some(profile.minecraft_version.as_str())
            );
            assert_eq!(preview.dimensions, profile.dimension_profiles);
            assert!(preview.compatible);
            let imported = read_verified_import(&export.bytes).unwrap();
            assert_eq!(imported.structure, export.structure);
            assert_eq!(
                imported
                    .structure
                    .to_world_changes(position, false)
                    .unwrap(),
                vec![(position, state)]
            );
        }
    }

    #[test]
    fn export_verification_is_independent_of_world_iteration_order() {
        let mut world = World::new_empty();
        for pos in [
            BlockPos::new(0, 65, 0),
            BlockPos::new(0, 64, 1),
            BlockPos::new(0, 64, 0),
            BlockPos::new(1, 64, 0),
        ] {
            world
                .set_block(
                    "minecraft:overworld",
                    pos,
                    &"minecraft:stone".parse().unwrap(),
                )
                .unwrap();
        }
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 65, 1)).unwrap();
        let bundle = prepare_export(&world, "minecraft:overworld", Some(bounds), "FuryBuilder")
            .expect("capture order must not break semantic verification");
        assert_eq!(bundle.structure.non_air_blocks(), 4);
        assert!(bundle
            .structure
            .blocks
            .windows(2)
            .all(|pair| pair[0].pos <= pair[1].pos));
    }

    #[test]
    fn import_preview_counts_states_and_reports_the_exact_target() {
        let world = sample_world();
        let bundle = prepare_export(&world, "minecraft:overworld", None, "FuryBuilder").unwrap();
        let preview = import_preview(&bundle.bytes).unwrap();
        let target = default_version_profile();
        assert_eq!(preview.format, JAVA_STRUCTURE_FORMAT);
        assert_eq!(preview.target_data_version, Some(target.data_version));
        assert_eq!(
            preview.target_minecraft_version.as_deref(),
            Some(target.minecraft_version.as_str())
        );
        assert_eq!(preview.non_air_blocks, 2);
        assert_eq!(
            preview.palette.iter().map(|entry| entry.count).sum::<u64>(),
            2
        );
        assert!(preview.compatible);
    }

    #[test]
    fn empty_dimension_has_no_bounds_and_cannot_export() {
        let world = World::new_empty();
        assert!(content_bounds(&world, "minecraft:overworld")
            .unwrap()
            .is_none());
        assert!(prepare_export(&world, "minecraft:overworld", None, "FuryBuilder").is_err());
    }

    #[test]
    fn air_only_region_exports_a_valid_empty_structure() {
        let world = sample_world();
        let bounds = BlockBox::new(BlockPos::new(20, 70, 20), BlockPos::new(21, 71, 21)).unwrap();
        let bundle =
            prepare_export(&world, "minecraft:overworld", Some(bounds), "FuryBuilder").unwrap();
        assert_eq!(bundle.structure.non_air_blocks(), 0);
        assert!(bundle
            .warnings
            .iter()
            .any(|warning| warning.contains("no non-air blocks")));
        let palette = palette_counts(&bundle.structure);
        assert_eq!(palette[0].state, BlockState::default().to_string());
    }
}
