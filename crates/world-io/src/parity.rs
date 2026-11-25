//! Reference-chunk parity between the original generator output and the editable world.
//!
//! The original verified server writes a Java world. FuryBuilder imports that world into the
//! editable `world_domain::World`, so an untouched import can be checked against the
//! generated region files without re-running the generator. This module compares block
//! states in a bounded, deterministic sample and reports every disagreement with absolute
//! coordinates.
//!
//! Biomes are deliberately **not** claimed: the editable world model does not store them.
//! They remain in the immutable source copy, and the report lists biome verification as an
//! explicit limitation. The caller supplies the already-validated reference chunks; the
//! generated settings comparison lives in the desktop adapter because it needs the project
//! provenance and the generation report.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BlockState, ChunkPos, World, SECTION_SIZE};

use crate::anvil::{AnvilError, JavaWorld, RegionFile};
use crate::java_chunk::DecodedChunk;

/// Maximum number of reference chunks compared in one check.
pub const MAX_PARITY_CHUNKS: usize = 64;
/// Maximum number of detailed block differences kept in a report.
pub const MAX_PARITY_MISMATCHES: usize = 20;
/// Hard bound for compared block positions in one check.
pub const MAX_PARITY_BLOCKS: u64 = 50_000_000;
/// The generation request always targets the classic overworld region folder.
pub const PARITY_DIMENSION: &str = "minecraft:overworld";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParityOptions {
    pub max_chunks: usize,
    pub max_mismatches: usize,
    pub max_blocks: u64,
}

impl Default for ParityOptions {
    fn default() -> Self {
        Self {
            max_chunks: MAX_PARITY_CHUNKS,
            max_mismatches: MAX_PARITY_MISMATCHES,
            max_blocks: MAX_PARITY_BLOCKS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParityStatus {
    Matched,
    Mismatched,
    Refused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParityChunkRef {
    pub x: i32,
    pub z: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParityMismatch {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub section_y: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub reference: String,
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParityReport {
    pub status: ParityStatus,
    pub reference_root: String,
    pub dimension: String,
    pub requested_chunks: usize,
    pub sampled_chunks: usize,
    pub checked_chunks: usize,
    pub checked_sections: usize,
    pub checked_blocks: u64,
    pub mismatch_count: u64,
    pub mismatches: Vec<ParityMismatch>,
    pub missing_reference_chunks: Vec<ParityChunkRef>,
    pub missing_project_chunks: Vec<ParityChunkRef>,
    pub refusals: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParityError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error("reference-chunk parity could not be checked: {0}")]
    Invalid(String),
    #[error("reference-chunk parity exceeded the bound: {0}")]
    Limit(String),
}

impl ParityError {
    pub fn code(&self) -> &'static str {
        match self {
            ParityError::Anvil(error) => error.code(),
            ParityError::Invalid(_) => "invalid_argument",
            ParityError::Limit(_) => "limit_exceeded",
        }
    }
}

/// Returns the requested chunks, or a deterministic evenly spaced subset when the area
/// exceeds `max_chunks`. The result is sorted by chunk coordinate and always includes the
/// center chunk.
pub fn reference_chunks(
    center_x: i32,
    center_z: i32,
    radius_chunks: u32,
    max_chunks: usize,
) -> Vec<ChunkPos> {
    if max_chunks == 0 {
        return Vec::new();
    }
    // The generator validates radius 1..=16; clamp defensively so an untrusted value can
    // never allocate an unbounded sample list here.
    let radius = radius_chunks.min(16) as i64;
    let center = ChunkPos::new(center_x.div_euclid(16), center_z.div_euclid(16));
    let side = (radius * 2 + 1) as usize;
    let mut all = Vec::with_capacity(side * side);
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            all.push(ChunkPos::new(center.x + dx as i32, center.z + dz as i32));
        }
    }
    if all.len() <= max_chunks {
        all.sort();
        return all;
    }

    // Choose the largest square grid that fits the budget, then walk it in row-major
    // order. This avoids randomness and keeps repeated checks identical.
    let mut step = 1usize;
    while (side.div_ceil(step)).saturating_mul(side.div_ceil(step)) > max_chunks {
        step += 1;
    }
    let mut sampled = Vec::new();
    let mut z = 0usize;
    while z < side {
        let mut x = 0usize;
        while x < side && sampled.len() < max_chunks {
            sampled.push(all[z * side + x]);
            x += step;
        }
        z += step;
    }
    if !sampled.contains(&center) {
        if sampled.len() >= max_chunks {
            sampled.pop();
        }
        sampled.push(center);
    }
    sampled.sort();
    sampled.dedup();
    sampled
}

/// Reads the reference world and compares the sampled chunks against the editable world.
///
/// The reference world is opened read-only. A missing reference chunk or a chunk that is
/// not fully generated refuses the check instead of comparing an incomplete generator
/// output. A missing project chunk is a real difference: the reference blocks are compared
/// against air.
pub fn compare_java_world_to_world(
    root: impl AsRef<Path>,
    chunks: &[ChunkPos],
    world: &World,
    options: ParityOptions,
) -> Result<ParityReport, ParityError> {
    if chunks.is_empty() {
        return Err(ParityError::Invalid(
            "no reference chunks were selected for the parity check".to_owned(),
        ));
    }
    if chunks.len() > options.max_chunks {
        return Err(ParityError::Limit(format!(
            "{} reference chunks exceed the {}-chunk check limit",
            chunks.len(),
            options.max_chunks
        )));
    }
    if options.max_blocks == 0 {
        return Err(ParityError::Invalid(
            "the block comparison budget must be greater than zero".to_owned(),
        ));
    }

    let root = root.as_ref();
    let reference = JavaWorld::open(root)?;
    let format = crate::classic_anvil::format::ClassicChunkFormat::for_profile(&reference.profile)?;
    let dimension = reference.dimension(PARITY_DIMENSION).ok_or_else(|| {
        ParityError::Invalid(format!(
            "the reference world has no {PARITY_DIMENSION} classic region folder"
        ))
    })?;
    let existing_regions: BTreeSet<(i32, i32)> = reference
        .region_coordinates(PARITY_DIMENSION)?
        .into_iter()
        .collect();

    let mut report = ParityReport {
        status: ParityStatus::Matched,
        reference_root: root.display().to_string(),
        dimension: PARITY_DIMENSION.to_owned(),
        requested_chunks: chunks.len(),
        sampled_chunks: 0,
        checked_chunks: 0,
        checked_sections: 0,
        checked_blocks: 0,
        mismatch_count: 0,
        mismatches: Vec::new(),
        missing_reference_chunks: Vec::new(),
        missing_project_chunks: Vec::new(),
        refusals: Vec::new(),
        limitations: vec![
            "block states are compared; biomes are preserved in the immutable source copy but are not modelled by the editable world".to_owned(),
            "only the requested overworld reference chunks are compared; spawn chunks and other dimensions are outside this sample".to_owned(),
            "a match is limited to the sampled chunks and the recorded generator settings; it is not a proof of general vanilla parity".to_owned(),
        ],
    };

    // Read every reference chunk first. Refusing an incomplete reference avoids comparing a
    // valid project against a truncated generator output.
    let mut regions: BTreeMap<(i32, i32), RegionFile> = BTreeMap::new();
    let mut decoded_chunks: Vec<DecodedChunk> = Vec::new();
    for position in chunks {
        let region_pos = position.region_pos();
        let key = (region_pos.x, region_pos.z);
        if !existing_regions.contains(&key) {
            report.missing_reference_chunks.push(ParityChunkRef {
                x: position.x,
                z: position.z,
            });
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) = regions.entry(key) {
            entry.insert(reference.read_region(PARITY_DIMENSION, key.0, key.1)?);
        }
        let region = regions.get(&key).ok_or_else(|| {
            ParityError::Invalid("reference region cache is inconsistent".to_owned())
        })?;
        if !region
            .entries()
            .iter()
            .any(|entry| entry.x == position.x && entry.z == position.z)
        {
            report.missing_reference_chunks.push(ParityChunkRef {
                x: position.x,
                z: position.z,
            });
            continue;
        }
        let chunk = region.read_chunk(position.x, position.z)?;
        let decoded = chunk.decode(dimension, &reference.profile)?;
        if !format.is_full(decoded.status.as_deref()) {
            report.refusals.push(format!(
                "reference chunk ({}, {}) has status `{}` instead of `{}`",
                position.x,
                position.z,
                decoded.status.as_deref().unwrap_or("missing"),
                format.full_status
            ));
            continue;
        }
        decoded_chunks.push(decoded);
    }
    report.sampled_chunks = decoded_chunks.len();
    if report.requested_chunks > report.sampled_chunks {
        report.limitations.push(format!(
            "the requested area exceeds the {}-chunk budget; a deterministic subset was compared",
            options.max_chunks
        ));
    }

    if !report.missing_reference_chunks.is_empty() {
        report.refusals.push(format!(
            "{} requested reference chunk(s) are absent from the generated world",
            report.missing_reference_chunks.len()
        ));
    }
    if !report.refusals.is_empty() {
        report.status = ParityStatus::Refused;
        return Ok(report);
    }

    let mut mismatch_count = 0u64;
    let mut mismatches = Vec::new();
    let mut checked_sections = 0usize;
    let mut checked_blocks = 0u64;
    let mut missing_project_chunks = Vec::new();
    for decoded in &decoded_chunks {
        let comparison = compare_decoded_chunk(
            decoded,
            world,
            options.max_mismatches,
            &mut mismatches,
            &mut mismatch_count,
        );
        checked_sections = checked_sections.saturating_add(comparison.checked_sections);
        checked_blocks = checked_blocks
            .checked_add(comparison.checked_blocks)
            .ok_or_else(|| ParityError::Limit("compared block counter overflow".to_owned()))?;
        if checked_blocks > options.max_blocks {
            return Err(ParityError::Limit(format!(
                "the comparison exceeded the {} block-position budget",
                options.max_blocks
            )));
        }
        if comparison.missing_project_chunk {
            missing_project_chunks.push(ParityChunkRef {
                x: decoded.x,
                z: decoded.z,
            });
        }
    }

    report.checked_chunks = decoded_chunks.len();
    report.checked_sections = checked_sections;
    report.checked_blocks = checked_blocks;
    report.mismatch_count = mismatch_count;
    report.mismatches = mismatches;
    report.missing_project_chunks = missing_project_chunks;
    if !report.missing_project_chunks.is_empty() {
        report.refusals.push(format!(
            "{} reference chunk(s) have no matching editable chunk",
            report.missing_project_chunks.len()
        ));
    }
    report.status = if report.mismatch_count == 0 && report.missing_project_chunks.is_empty() {
        ParityStatus::Matched
    } else {
        ParityStatus::Mismatched
    };
    Ok(report)
}

struct ChunkComparison {
    checked_sections: usize,
    checked_blocks: u64,
    missing_project_chunk: bool,
}

/// Compares one decoded reference chunk with the same chunk in the editable world.
///
/// Only sections that are non-empty on at least one side are visited. A section missing on
/// the project side reads as air, so any reference block in it becomes a difference.
fn compare_decoded_chunk(
    decoded: &DecodedChunk,
    world: &World,
    max_mismatches: usize,
    mismatches: &mut Vec<ParityMismatch>,
    mismatch_count: &mut u64,
) -> ChunkComparison {
    let chunk_pos = ChunkPos::new(decoded.x, decoded.z);
    let project_chunk = world
        .dimension(PARITY_DIMENSION)
        .ok()
        .and_then(|dimension| dimension.chunk(chunk_pos));
    let mut section_ys: BTreeSet<i32> = BTreeSet::new();
    for (y, section) in &decoded.sections {
        if !section.blocks.is_empty() {
            section_ys.insert(*y);
        }
    }
    if let Some(chunk) = project_chunk {
        for (y, section) in chunk.sections() {
            if !section.is_empty() {
                section_ys.insert(*y);
            }
        }
    }

    let air = BlockState::default();
    let mut checked_sections = 0usize;
    let mut checked_blocks = 0u64;
    for section_y in section_ys {
        let reference_section = decoded
            .sections
            .get(&section_y)
            .map(|section| &section.blocks)
            .filter(|section| !section.is_empty());
        let project_section = project_chunk
            .and_then(|chunk| chunk.section(section_y))
            .filter(|section| !section.is_empty());
        checked_sections += 1;
        checked_blocks =
            checked_blocks.saturating_add((SECTION_SIZE * SECTION_SIZE * SECTION_SIZE) as u64);
        if reference_section == project_section {
            continue;
        }
        for local_y in 0..SECTION_SIZE {
            for local_z in 0..SECTION_SIZE {
                for local_x in 0..SECTION_SIZE {
                    let expected = reference_section
                        .and_then(|section| {
                            section.get(local_x as u8, local_y as u8, local_z as u8)
                        })
                        .unwrap_or(&air);
                    let actual = project_section
                        .and_then(|section| {
                            section.get(local_x as u8, local_y as u8, local_z as u8)
                        })
                        .unwrap_or(&air);
                    if expected == actual {
                        continue;
                    }
                    *mismatch_count = mismatch_count.saturating_add(1);
                    if mismatches.len() < max_mismatches {
                        let base_y = section_y * SECTION_SIZE;
                        mismatches.push(ParityMismatch {
                            chunk_x: decoded.x,
                            chunk_z: decoded.z,
                            section_y,
                            x: decoded.x * SECTION_SIZE + local_x,
                            y: base_y + local_y,
                            z: decoded.z * SECTION_SIZE + local_z,
                            reference: expected.to_string(),
                            project: actual.to_string(),
                        });
                    }
                }
            }
        }
    }

    ChunkComparison {
        checked_sections,
        checked_blocks,
        missing_project_chunk: project_chunk.map(|chunk| chunk.is_empty()).unwrap_or(true)
            && decoded
                .sections
                .values()
                .any(|section| !section.blocks.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use world_domain::{ChunkPos, Section};

    use crate::java_chunk::{BiomeSection, DecodedSection};
    use crate::version_support;

    fn section_with(edits: &[(u8, u8, u8, &str)]) -> Section {
        let mut section = Section::empty();
        for (x, y, z, name) in edits {
            let state: BlockState = name.parse().expect("valid test block");
            section
                .set(*x, *y, *z, &state)
                .expect("valid local coordinate");
        }
        section
    }

    fn decoded(x: i32, z: i32, section_y: i32, blocks: Section) -> DecodedChunk {
        let biomes = BiomeSection::from_palette(vec!["minecraft:plains".to_owned()], vec![0; 64])
            .expect("valid biome section");
        DecodedChunk {
            x,
            z,
            data_version: version_support::default_version_profile().data_version,
            status: Some("minecraft:full".to_owned()),
            y_pos: -4,
            sections: BTreeMap::from([(
                section_y,
                DecodedSection {
                    y: section_y,
                    blocks,
                    biomes,
                },
            )]),
            missing_sections: Vec::new(),
            block_entities: 0,
            entities: 0,
        }
    }

    fn world_with(chunk_x: i32, chunk_z: i32, section_y: i32, blocks: Section) -> World {
        let mut world = World::new_empty();
        world
            .install_section(
                PARITY_DIMENSION,
                ChunkPos::new(chunk_x, chunk_z),
                section_y,
                Some(blocks),
            )
            .expect("install section");
        world
    }

    fn compare(
        decoded: &DecodedChunk,
        world: &World,
        max_mismatches: usize,
    ) -> (ChunkComparison, Vec<ParityMismatch>, u64) {
        let mut mismatches = Vec::new();
        let mut mismatch_count = 0;
        let comparison = compare_decoded_chunk(
            decoded,
            world,
            max_mismatches,
            &mut mismatches,
            &mut mismatch_count,
        );
        (comparison, mismatches, mismatch_count)
    }

    #[test]
    fn identical_chunks_match_without_differences() {
        let blocks = section_with(&[
            (0, 0, 0, "minecraft:stone"),
            (15, 15, 15, "minecraft:dirt[snowy=false]"),
        ]);
        let decoded = decoded(4, -3, 2, blocks.clone());
        let world = world_with(4, -3, 2, blocks);
        let (comparison, mismatches, count) = compare(&decoded, &world, 20);
        assert_eq!(comparison.checked_sections, 1);
        assert_eq!(comparison.checked_blocks, 4096);
        assert!(!comparison.missing_project_chunk);
        assert_eq!(count, 0);
        assert!(mismatches.is_empty());
    }

    #[test]
    fn changed_block_is_reported_with_absolute_coordinates() {
        let reference = section_with(&[(3, 4, 5, "minecraft:stone")]);
        let decoded = decoded(-2, 7, -1, reference);
        let project = section_with(&[(3, 4, 5, "minecraft:dirt")]);
        let world = world_with(-2, 7, -1, project);
        let (_, mismatches, count) = compare(&decoded, &world, 20);
        assert_eq!(count, 1);
        assert_eq!(mismatches.len(), 1);
        let mismatch = &mismatches[0];
        assert_eq!(mismatch.chunk_x, -2);
        assert_eq!(mismatch.chunk_z, 7);
        assert_eq!(mismatch.section_y, -1);
        assert_eq!(mismatch.x, -2 * 16 + 3);
        assert_eq!(mismatch.y, -16 + 4);
        assert_eq!(mismatch.z, 7 * 16 + 5);
        assert_eq!(mismatch.reference, "minecraft:stone");
        assert_eq!(mismatch.project, "minecraft:dirt");
    }

    #[test]
    fn missing_project_chunk_is_a_difference_and_detail_is_bounded() {
        let mut edits = Vec::new();
        for x in 0..8u8 {
            edits.push((x, 0, 0, "minecraft:stone"));
        }
        let decoded = decoded(0, 0, 0, section_with(&edits));
        let empty = World::new_empty();
        let (comparison, mismatches, count) = compare(&decoded, &empty, 3);
        assert!(comparison.missing_project_chunk);
        assert_eq!(count, 8);
        assert_eq!(mismatches.len(), 3);
        assert!(mismatches.iter().all(|item| !item.project.is_empty()));
        assert!(mismatches
            .iter()
            .all(|item| item.project == "minecraft:air"));
    }

    #[test]
    fn extra_project_blocks_are_detected() {
        let decoded = decoded(0, 0, 0, Section::empty());
        let project = section_with(&[(1, 2, 3, "minecraft:stone")]);
        let world = world_with(0, 0, 0, project);
        let (comparison, mismatches, count) = compare(&decoded, &world, 20);
        assert_eq!(comparison.checked_sections, 1);
        assert_eq!(count, 1);
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].reference, "minecraft:air");
        assert_eq!(mismatches[0].project, "minecraft:stone");
    }

    #[test]
    fn sampling_is_deterministic_and_bounded() {
        let small = reference_chunks(4500, 4500, 1, MAX_PARITY_CHUNKS);
        assert_eq!(small.len(), 9);
        assert!(small.contains(&ChunkPos::new(281, 281)));
        assert!(small.windows(2).all(|pair| pair[0] < pair[1]));

        let large = reference_chunks(-1000, 2000, 16, MAX_PARITY_CHUNKS);
        assert!(large.len() <= MAX_PARITY_CHUNKS);
        assert!(large.contains(&ChunkPos::new(-63, 125)));
        assert_eq!(large, reference_chunks(-1000, 2000, 16, MAX_PARITY_CHUNKS));
        assert!(large.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
