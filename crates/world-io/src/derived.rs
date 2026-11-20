//! Read-only inventory of Java 1.21.4 chunk-derived data (M8.8).
//!
//! A block edit does not live in isolation. A classic Java chunk also carries
//! heightmaps, light arrays, scheduled ticks, post-processing positions and block
//! entities, while entities and points of interest live in sidecar `entities/` and
//! `poi/` region containers. This module only **reports** those fields and their
//! fingerprints. It never rewrites a chunk, a sidecar container or the source world.
//!
//! The report is used to answer a narrow question before an export claims success:
//! which derived data was present in the source, which fields the verified writer
//! preserved as raw bytes, and which values it did not recalculate. The observations
//! are summarized in `docs/formats.md`; a full metadata
//! recomputation is a separate stage (M8.9).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use world_domain::{TARGET_DATA_VERSION, TARGET_MINECRAFT_VERSION};

use crate::anvil::{AnvilError, JavaWorld, JavaWorldDimension, RegionFile};
use crate::nbt::{NbtFile, NbtValue};

pub const MAX_DERIVED_CHUNKS: usize = 65_536;
pub const MAX_DERIVED_REGION_FILES: usize = 4_096;
pub const MAX_DERIVED_SIDECAR_RECORDS: usize = 65_536;
pub const MAX_DERIVED_SAMPLES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeightmapInfo {
    pub words: usize,
    pub sha256: String,
}

/// Stable fingerprints for the derived categories that a block edit can invalidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkFingerprints {
    pub heightmaps: Option<String>,
    pub sections: Option<String>,
    pub sky_light: Option<String>,
    pub block_light: Option<String>,
    pub block_entities: Option<String>,
    pub entities: Option<String>,
    pub block_ticks: Option<String>,
    pub fluid_ticks: Option<String>,
    pub post_processing: Option<String>,
    pub structures: Option<String>,
}

/// Bounded, read-only view of one 1.21.4 chunk's derived fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkDerivedData {
    pub x: i32,
    pub z: i32,
    pub data_version: i32,
    pub status: Option<String>,
    pub y_pos: Option<i32>,
    pub last_update: Option<i64>,
    pub inhabited_time: Option<i64>,
    pub sections: usize,
    pub sections_with_sky_light: usize,
    pub sections_with_block_light: usize,
    pub heightmaps: BTreeMap<String, HeightmapInfo>,
    pub block_entities: usize,
    pub entities: usize,
    pub block_ticks: usize,
    pub fluid_ticks: usize,
    pub post_processing: usize,
    pub structures: usize,
    pub light_on: Option<bool>,
    pub blending_data: bool,
    pub block_entity_ids: Vec<String>,
    pub entity_ids: Vec<String>,
    pub root_fields: BTreeMap<String, String>,
    pub section_field_counts: BTreeMap<String, usize>,
    pub fingerprints: ChunkFingerprints,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedCounters {
    pub chunks: usize,
    pub full_chunks: usize,
    pub root_fields: BTreeMap<String, usize>,
    pub section_fields: BTreeMap<String, usize>,
    pub heightmap_chunks: BTreeMap<String, usize>,
    pub heightmap_words: BTreeMap<String, usize>,
    pub chunks_with_block_entities: usize,
    pub block_entities: usize,
    pub chunks_with_entity_list: usize,
    pub entities_in_chunks: usize,
    pub chunks_with_block_ticks: usize,
    pub block_ticks: usize,
    pub chunks_with_fluid_ticks: usize,
    pub fluid_ticks: usize,
    pub chunks_with_post_processing: usize,
    pub post_processing: usize,
    pub chunks_with_sky_light: usize,
    pub sections_with_sky_light: usize,
    pub chunks_with_block_light: usize,
    pub sections_with_block_light: usize,
    pub chunks_with_any_light: usize,
    pub chunks_with_light_on: usize,
    pub chunks_with_structures: usize,
    pub structures: usize,
    pub chunks_with_blending_data: usize,
}

impl DerivedCounters {
    fn add(&mut self, chunk: &ChunkDerivedData) {
        self.chunks += 1;
        if chunk.status.as_deref() == Some("minecraft:full") {
            self.full_chunks += 1;
        }
        for key in chunk.root_fields.keys() {
            *self.root_fields.entry(key.clone()).or_default() += 1;
        }
        for (key, count) in &chunk.section_field_counts {
            *self.section_fields.entry(key.clone()).or_default() += count;
        }
        for (kind, info) in &chunk.heightmaps {
            *self.heightmap_chunks.entry(kind.clone()).or_default() += 1;
            *self.heightmap_words.entry(kind.clone()).or_default() += info.words;
        }
        if chunk.block_entities > 0 {
            self.chunks_with_block_entities += 1;
            self.block_entities += chunk.block_entities;
        }
        if chunk.entities > 0 {
            self.chunks_with_entity_list += 1;
            self.entities_in_chunks += chunk.entities;
        }
        if chunk.block_ticks > 0 {
            self.chunks_with_block_ticks += 1;
            self.block_ticks += chunk.block_ticks;
        }
        if chunk.fluid_ticks > 0 {
            self.chunks_with_fluid_ticks += 1;
            self.fluid_ticks += chunk.fluid_ticks;
        }
        if chunk.post_processing > 0 {
            self.chunks_with_post_processing += 1;
            self.post_processing += chunk.post_processing;
        }
        if chunk.sections_with_sky_light > 0 {
            self.chunks_with_sky_light += 1;
            self.sections_with_sky_light += chunk.sections_with_sky_light;
        }
        if chunk.sections_with_block_light > 0 {
            self.chunks_with_block_light += 1;
            self.sections_with_block_light += chunk.sections_with_block_light;
        }
        if chunk.sections_with_sky_light > 0 || chunk.sections_with_block_light > 0 {
            self.chunks_with_any_light += 1;
        }
        if chunk.light_on == Some(true) {
            self.chunks_with_light_on += 1;
        }
        if chunk.structures > 0 {
            self.chunks_with_structures += 1;
            self.structures += chunk.structures;
        }
        if chunk.blending_data {
            self.chunks_with_blending_data += 1;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionDerivedData {
    pub id: String,
    pub folder: String,
    pub counters: DerivedCounters,
}

/// One `poi/` or `entities/` container family for a single dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarScan {
    pub kind: String,
    pub dimension: String,
    pub folder: String,
    pub exists: bool,
    pub files: usize,
    pub empty_files: usize,
    pub allocated_records: usize,
    pub parsed_records: usize,
    pub entries: usize,
    pub sample_ids: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DerivedDataReport {
    pub minecraft_version: &'static str,
    pub data_version: i32,
    pub layout: &'static str,
    pub counters: DerivedCounters,
    pub dimensions: Vec<DimensionDerivedData>,
    pub sidecars: Vec<SidecarScan>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum DerivedDataError {
    #[error(transparent)]
    Anvil(#[from] AnvilError),
    #[error("invalid derived data: {0}")]
    InvalidChunk(String),
    #[error("derived data version {found} is not supported; supported version is {supported}")]
    UnsupportedVersion { found: i32, supported: i32 },
    #[error("unsafe derived data path: {0}")]
    UnsafePath(String),
    #[error("derived data limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("derived data I/O failed: {0}")]
    Io(String),
}

impl DerivedDataError {
    pub fn code(&self) -> &'static str {
        match self {
            DerivedDataError::Anvil(error) => error.code(),
            DerivedDataError::InvalidChunk(_) => "invalid_argument",
            DerivedDataError::UnsupportedVersion { .. } => "unsupported_version",
            DerivedDataError::UnsafePath(_) => "unsafe_path",
            DerivedDataError::LimitExceeded(_) => "limit_exceeded",
            DerivedDataError::Io(_) => "io_error",
        }
    }
}

fn invalid(message: impl Into<String>) -> DerivedDataError {
    DerivedDataError::InvalidChunk(message.into())
}

/// Inspects the derived fields of one already-decoded chunk.
///
/// The function is strict about known fields: a known key with an unexpected type is an
/// error, not a silent zero. Unknown fields only appear in `root_fields` and are left
/// exactly where they are. `DataVersion` must be the pinned 1.21.4 value.
pub fn inspect_chunk_derived(
    nbt: &NbtFile,
    x: i32,
    z: i32,
) -> Result<ChunkDerivedData, DerivedDataError> {
    let root = nbt
        .root
        .as_compound()
        .ok_or_else(|| invalid(format!("chunk ({x}, {z}) root is not a compound")))?;
    let data_version = match root.get("DataVersion") {
        Some(NbtValue::Int(value)) => *value,
        Some(_) => {
            return Err(invalid(format!(
                "chunk ({x}, {z}) DataVersion is not an int"
            )))
        }
        None => return Err(invalid(format!("chunk ({x}, {z}) has no DataVersion"))),
    };
    if data_version != TARGET_DATA_VERSION {
        return Err(DerivedDataError::UnsupportedVersion {
            found: data_version,
            supported: TARGET_DATA_VERSION,
        });
    }

    let mut root_fields = BTreeMap::new();
    for (key, value) in root {
        root_fields.insert(key.clone(), tag_name(value).to_owned());
    }

    let status = optional_string(root, "Status", x, z)?;
    let y_pos = optional_int(root, "yPos", x, z)?;
    let last_update = optional_long(root, "LastUpdate", x, z)?;
    let inhabited_time = optional_long(root, "InhabitedTime", x, z)?;

    let SectionInspection {
        sections,
        field_counts: section_field_counts,
        sections_with_sky_light,
        sections_with_block_light,
        sky_light,
        block_light,
    } = inspect_sections(root, x, z)?;

    let heightmaps = inspect_heightmaps(root, x, z)?;

    let block_entities_list = list_value(root, "block_entities", x, z)?;
    let block_entity_ids = sample_ids(block_entities_list, "id");
    let entities_list = list_value(root, "entities", x, z)?;
    let entity_ids = sample_ids(entities_list, "id");

    let block_ticks = list_len(root, "block_ticks", x, z)?;
    let fluid_ticks = list_len(root, "fluid_ticks", x, z)?;
    let post_processing = any_list_len(root, "PostProcessing", x, z)?;

    let structures = match root.get("structures") {
        None => 0,
        Some(NbtValue::Compound(value)) => match value.get("starts") {
            None => 0,
            Some(NbtValue::Compound(starts)) => starts.len(),
            Some(_) => {
                return Err(invalid(format!(
                    "chunk ({x}, {z}) structures.starts is not a compound"
                )))
            }
        },
        Some(_) => {
            return Err(invalid(format!(
                "chunk ({x}, {z}) structures is not a compound"
            )))
        }
    };

    let light_on = match root.get("isLightOn") {
        None => None,
        Some(NbtValue::Byte(value)) => Some(*value != 0),
        Some(_) => return Err(invalid(format!("chunk ({x}, {z}) isLightOn is not a byte"))),
    };
    let blending_data = root.contains_key("blending_data");

    let fingerprints = ChunkFingerprints {
        heightmaps: root.get("Heightmaps").map(hash_value),
        sections: root.get("sections").map(hash_value),
        sky_light: sky_light.map(|hasher| hex(hasher.finalize())),
        block_light: block_light.map(|hasher| hex(hasher.finalize())),
        block_entities: root.get("block_entities").map(hash_value),
        entities: root.get("entities").map(hash_value),
        block_ticks: root.get("block_ticks").map(hash_value),
        fluid_ticks: root.get("fluid_ticks").map(hash_value),
        post_processing: root.get("PostProcessing").map(hash_value),
        structures: root.get("structures").map(hash_value),
    };

    Ok(ChunkDerivedData {
        x,
        z,
        data_version,
        status,
        y_pos,
        last_update,
        inhabited_time,
        sections,
        sections_with_sky_light,
        sections_with_block_light,
        heightmaps,
        block_entities: block_entities_list.map_or(0, <[NbtValue]>::len),
        entities: entities_list.map_or(0, <[NbtValue]>::len),
        block_ticks,
        fluid_ticks,
        post_processing,
        structures,
        light_on,
        blending_data,
        block_entity_ids,
        entity_ids,
        root_fields,
        section_field_counts,
        fingerprints,
    })
}

struct SectionInspection {
    sections: usize,
    field_counts: BTreeMap<String, usize>,
    sections_with_sky_light: usize,
    sections_with_block_light: usize,
    sky_light: Option<Sha256>,
    block_light: Option<Sha256>,
}

fn inspect_sections(
    root: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
) -> Result<SectionInspection, DerivedDataError> {
    let value = root
        .get("sections")
        .ok_or_else(|| invalid(format!("chunk ({x}, {z}) has no sections list")))?;
    let (kind, items) = value
        .as_list()
        .ok_or_else(|| invalid(format!("chunk ({x}, {z}) sections is not a list")))?;
    if kind != crate::nbt::TAG_COMPOUND && !items.is_empty() {
        return Err(invalid(format!(
            "chunk ({x}, {z}) sections is not a list of compounds"
        )));
    }
    let mut counts = BTreeMap::new();
    let mut sky_sections = 0usize;
    let mut block_sections = 0usize;
    let mut sky_hash: Option<Sha256> = None;
    let mut block_hash: Option<Sha256> = None;
    for item in items {
        let section = item
            .as_compound()
            .ok_or_else(|| invalid(format!("chunk ({x}, {z}) has a non-compound section")))?;
        let y = match section.get("Y") {
            Some(NbtValue::Byte(value)) => *value as i32,
            Some(_) => return Err(invalid(format!("chunk ({x}, {z}) section Y is not a byte"))),
            None => return Err(invalid(format!("chunk ({x}, {z}) section has no Y"))),
        };
        for key in section.keys() {
            *counts.entry(key.clone()).or_default() += 1;
        }
        if let Some(value) = section.get("SkyLight") {
            match value {
                NbtValue::ByteArray(bytes) => {
                    sky_sections += 1;
                    let hasher = sky_hash.get_or_insert_with(Sha256::new);
                    hasher.update(y.to_be_bytes());
                    hasher.update((bytes.len() as u64).to_be_bytes());
                    hasher.update(bytes);
                }
                _ => {
                    return Err(invalid(format!(
                        "chunk ({x}, {z}) section {y} SkyLight is not a byte array"
                    )))
                }
            }
        }
        if let Some(value) = section.get("BlockLight") {
            match value {
                NbtValue::ByteArray(bytes) => {
                    block_sections += 1;
                    let hasher = block_hash.get_or_insert_with(Sha256::new);
                    hasher.update(y.to_be_bytes());
                    hasher.update((bytes.len() as u64).to_be_bytes());
                    hasher.update(bytes);
                }
                _ => {
                    return Err(invalid(format!(
                        "chunk ({x}, {z}) section {y} BlockLight is not a byte array"
                    )))
                }
            }
        }
    }
    Ok(SectionInspection {
        sections: items.len(),
        field_counts: counts,
        sections_with_sky_light: sky_sections,
        sections_with_block_light: block_sections,
        sky_light: sky_hash,
        block_light: block_hash,
    })
}

fn inspect_heightmaps(
    root: &BTreeMap<String, NbtValue>,
    x: i32,
    z: i32,
) -> Result<BTreeMap<String, HeightmapInfo>, DerivedDataError> {
    let Some(value) = root.get("Heightmaps") else {
        return Ok(BTreeMap::new());
    };
    let map = value
        .as_compound()
        .ok_or_else(|| invalid(format!("chunk ({x}, {z}) Heightmaps is not a compound")))?;
    let mut heightmaps = BTreeMap::new();
    for (name, value) in map {
        let words = match value {
            NbtValue::LongArray(words) => words,
            _ => {
                return Err(invalid(format!(
                    "chunk ({x}, {z}) Heightmaps.{name} is not a long array"
                )))
            }
        };
        heightmaps.insert(
            name.clone(),
            HeightmapInfo {
                words: words.len(),
                sha256: hash_long_array(words),
            },
        );
    }
    Ok(heightmaps)
}

/// Scans every allocated block chunk and both sidecar families for the whole world.
///
/// The world is opened read-only by the caller. Sidecar containers are parsed with the
/// generic region reader so missing `xPos`, `Status` or `sections` do not reject a
/// perfectly valid `poi/` or `entities/` record.
pub fn analyze_java_world_derived(
    world: &JavaWorld,
) -> Result<DerivedDataReport, DerivedDataError> {
    if world.level.data_version != TARGET_DATA_VERSION {
        return Err(DerivedDataError::UnsupportedVersion {
            found: world.level.data_version,
            supported: TARGET_DATA_VERSION,
        });
    }

    let mut counters = DerivedCounters::default();
    let mut dimensions = Vec::new();
    for dimension in &world.dimensions {
        let mut dimension_counters = DerivedCounters::default();
        for path in list_region_files(&dimension.region_folder)? {
            let region = RegionFile::open(&path)?;
            for entry in region.entries() {
                if counters.chunks >= MAX_DERIVED_CHUNKS {
                    return Err(DerivedDataError::LimitExceeded(format!(
                        "world has more than {MAX_DERIVED_CHUNKS} chunks"
                    )));
                }
                let nbt = region.read_nbt(entry.x, entry.z)?;
                let chunk = inspect_chunk_derived(&nbt, entry.x, entry.z)?;
                dimension_counters.add(&chunk);
                counters.add(&chunk);
            }
        }
        dimensions.push(DimensionDerivedData {
            id: dimension.id.clone(),
            folder: dimension.folder.display().to_string(),
            counters: dimension_counters,
        });
    }

    let mut sidecars = Vec::new();
    for dimension in &world.dimensions {
        for kind in ["poi", "entities"] {
            sidecars.push(scan_sidecar(kind, dimension)?);
        }
    }

    let mut warnings = Vec::new();
    if counters.chunks_with_block_entities > 0 {
        warnings.push(
            "block entity records are preserved as raw data; placing or removing block-entity blocks is not semantically supported yet"
                .to_owned(),
        );
    }
    if counters.chunks_with_block_ticks > 0 || counters.chunks_with_fluid_ticks > 0 {
        warnings.push(
            "scheduled block/fluid ticks are preserved but not recomputed after block edits"
                .to_owned(),
        );
    }
    if counters.chunks_with_post_processing > 0 {
        warnings.push(
            "PostProcessing positions and light arrays are preserved; edited chunks are not relit"
                .to_owned(),
        );
    }
    if counters.chunks_with_any_light < counters.full_chunks {
        warnings.push(format!(
            "{} of {} full chunks have no SkyLight/BlockLight arrays",
            counters.full_chunks - counters.chunks_with_any_light,
            counters.full_chunks
        ));
    }
    if !counters.heightmap_chunks.is_empty() {
        warnings.push(
            "heightmaps are preserved as raw data and are not recalculated after block edits"
                .to_owned(),
        );
    }
    if counters.chunks_with_structures > 0 {
        warnings.push(
            "structure references are preserved as raw data and are not updated by block edits"
                .to_owned(),
        );
    }
    if counters.chunks_with_entity_list > 0 {
        warnings.push(
            "legacy chunk entities field is present; 1.17+ normally stores entities in entities/ sidecars"
                .to_owned(),
        );
    }
    for sidecar in &sidecars {
        if sidecar.entries > 0 {
            warnings.push(format!(
                "{} sidecar holds {} entries and is copied byte for byte, never updated by block edits",
                sidecar.kind, sidecar.entries
            ));
        }
    }

    Ok(DerivedDataReport {
        minecraft_version: TARGET_MINECRAFT_VERSION,
        data_version: TARGET_DATA_VERSION,
        layout: "classic_anvil",
        counters,
        dimensions,
        sidecars,
        warnings,
    })
}

fn scan_sidecar(
    kind: &str,
    dimension: &JavaWorldDimension,
) -> Result<SidecarScan, DerivedDataError> {
    let folder = dimension.folder.join(kind);
    let metadata = match fs::symlink_metadata(&folder) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SidecarScan {
                kind: kind.to_owned(),
                dimension: dimension.id.clone(),
                folder: folder.display().to_string(),
                exists: false,
                files: 0,
                empty_files: 0,
                allocated_records: 0,
                parsed_records: 0,
                entries: 0,
                sample_ids: Vec::new(),
                notes: vec!["folder is missing".to_owned()],
            });
        }
        Err(error) => {
            return Err(DerivedDataError::Io(format!(
                "reading {}: {error}",
                folder.display()
            )))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(DerivedDataError::UnsafePath(folder.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(DerivedDataError::InvalidChunk(format!(
            "{} is not a directory",
            folder.display()
        )));
    }

    let mut scan = SidecarScan {
        kind: kind.to_owned(),
        dimension: dimension.id.clone(),
        folder: folder.display().to_string(),
        exists: true,
        files: 0,
        empty_files: 0,
        allocated_records: 0,
        parsed_records: 0,
        entries: 0,
        sample_ids: Vec::new(),
        notes: Vec::new(),
    };
    let mut samples = BTreeSet::new();
    for path in list_region_files(&folder)? {
        scan.files += 1;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            DerivedDataError::Io(format!("reading {}: {error}", path.display()))
        })?;
        if metadata.file_type().is_symlink() {
            return Err(DerivedDataError::UnsafePath(path.display().to_string()));
        }
        if metadata.len() == 0 {
            scan.empty_files += 1;
            continue;
        }
        let region = RegionFile::open(&path)?;
        for entry in region.entries() {
            scan.allocated_records += 1;
            if scan.allocated_records > MAX_DERIVED_SIDECAR_RECORDS {
                return Err(DerivedDataError::LimitExceeded(format!(
                    "{} has more than {MAX_DERIVED_SIDECAR_RECORDS} sidecar records",
                    folder.display()
                )));
            }
            let nbt = region.read_nbt(entry.x, entry.z)?;
            let root = nbt.root.as_compound().ok_or_else(|| {
                invalid(format!(
                    "{} chunk ({}, {}) root is not a compound",
                    path.display(),
                    entry.x,
                    entry.z
                ))
            })?;
            scan.parsed_records += 1;
            if kind == "entities" {
                match root.get("Entities") {
                    None => scan.notes.push(format!(
                        "entity record ({}, {}) has no Entities list",
                        entry.x, entry.z
                    )),
                    Some(NbtValue::List { kind, items })
                        if *kind == crate::nbt::TAG_COMPOUND || items.is_empty() =>
                    {
                        scan.entries += items.len();
                        collect_ids(items, "id", &mut samples);
                    }
                    Some(_) => {
                        return Err(invalid(format!(
                            "entity record ({}, {}) Entities is not a list of compounds",
                            entry.x, entry.z
                        )))
                    }
                }
            } else {
                match root.get("Sections") {
                    None => scan.notes.push(format!(
                        "POI record ({}, {}) has no Sections",
                        entry.x, entry.z
                    )),
                    Some(NbtValue::Compound(sections)) => {
                        for section in sections.values() {
                            collect_poi_section(
                                section,
                                entry.x,
                                entry.z,
                                &mut scan,
                                &mut samples,
                            )?;
                        }
                    }
                    Some(NbtValue::List { kind, items })
                        if *kind == crate::nbt::TAG_COMPOUND || items.is_empty() =>
                    {
                        for section in items {
                            collect_poi_section(
                                section,
                                entry.x,
                                entry.z,
                                &mut scan,
                                &mut samples,
                            )?;
                        }
                    }
                    Some(_) => {
                        return Err(invalid(format!(
                            "POI record ({}, {}) Sections is neither a compound nor a list",
                            entry.x, entry.z
                        )))
                    }
                }
            }
        }
    }
    scan.sample_ids = samples.into_iter().take(MAX_DERIVED_SAMPLES).collect();
    Ok(scan)
}

fn collect_poi_section(
    section: &NbtValue,
    x: i32,
    z: i32,
    scan: &mut SidecarScan,
    samples: &mut BTreeSet<String>,
) -> Result<(), DerivedDataError> {
    let section = section
        .as_compound()
        .ok_or_else(|| invalid(format!("POI record ({x}, {z}) has a non-compound section")))?;
    match section.get("Records") {
        None => Ok(()),
        Some(NbtValue::List { kind, items })
            if *kind == crate::nbt::TAG_COMPOUND || items.is_empty() =>
        {
            scan.entries += items.len();
            collect_ids(items, "type", samples);
            Ok(())
        }
        Some(_) => Err(invalid(format!(
            "POI record ({x}, {z}) section Records is not a list of compounds"
        ))),
    }
}

fn collect_ids(items: &[NbtValue], key: &str, samples: &mut BTreeSet<String>) {
    for item in items {
        if let Some(value) = item
            .as_compound()
            .and_then(|compound| compound.get(key))
            .and_then(NbtValue::as_string)
        {
            if samples.len() < MAX_DERIVED_SAMPLES {
                samples.insert(value.to_owned());
            }
        }
    }
}

fn sample_ids(items: Option<&[NbtValue]>, key: &str) -> Vec<String> {
    let mut samples = BTreeSet::new();
    if let Some(items) = items {
        collect_ids(items, key, &mut samples);
    }
    samples.into_iter().collect()
}

fn list_value<'a>(
    root: &'a BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<Option<&'a [NbtValue]>, DerivedDataError> {
    match root.get(key) {
        None => Ok(None),
        Some(value) => {
            let (kind, items) = value
                .as_list()
                .ok_or_else(|| invalid(format!("chunk ({x}, {z}) {key} is not a list")))?;
            if kind != crate::nbt::TAG_COMPOUND && !items.is_empty() {
                return Err(invalid(format!(
                    "chunk ({x}, {z}) {key} is not a list of compounds"
                )));
            }
            Ok(Some(items))
        }
    }
}

fn list_len(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<usize, DerivedDataError> {
    Ok(list_value(root, key, x, z)?.map_or(0, <[NbtValue]>::len))
}

/// `PostProcessing` stores positions in a nested list shape, not compound records.
fn any_list_len(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<usize, DerivedDataError> {
    match root.get(key) {
        None => Ok(0),
        Some(value) => value
            .as_list()
            .map(|(_, items)| items.len())
            .ok_or_else(|| invalid(format!("chunk ({x}, {z}) {key} is not a list"))),
    }
}

fn optional_string(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<Option<String>, DerivedDataError> {
    match root.get(key) {
        None => Ok(None),
        Some(NbtValue::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(invalid(format!("chunk ({x}, {z}) {key} is not a string"))),
    }
}

fn optional_int(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<Option<i32>, DerivedDataError> {
    match root.get(key) {
        None => Ok(None),
        Some(NbtValue::Int(value)) => Ok(Some(*value)),
        Some(_) => Err(invalid(format!("chunk ({x}, {z}) {key} is not an int"))),
    }
}

fn optional_long(
    root: &BTreeMap<String, NbtValue>,
    key: &str,
    x: i32,
    z: i32,
) -> Result<Option<i64>, DerivedDataError> {
    match root.get(key) {
        None => Ok(None),
        Some(NbtValue::Long(value)) => Ok(Some(*value)),
        Some(_) => Err(invalid(format!("chunk ({x}, {z}) {key} is not a long"))),
    }
}

fn tag_name(value: &NbtValue) -> &'static str {
    match value {
        NbtValue::Byte(_) => "byte",
        NbtValue::Short(_) => "short",
        NbtValue::Int(_) => "int",
        NbtValue::Long(_) => "long",
        NbtValue::Float(_) => "float",
        NbtValue::Double(_) => "double",
        NbtValue::ByteArray(_) => "byteArray",
        NbtValue::String(_) => "string",
        NbtValue::List { .. } => "list",
        NbtValue::Compound(_) => "compound",
        NbtValue::IntArray(_) => "intArray",
        NbtValue::LongArray(_) => "longArray",
    }
}

fn hash_value(value: &NbtValue) -> String {
    let mut hasher = Sha256::new();
    hash_into(&mut hasher, value);
    hex(hasher.finalize())
}

fn hash_into(hasher: &mut Sha256, value: &NbtValue) {
    hasher.update([value.tag_id()]);
    match value {
        NbtValue::Byte(value) => hasher.update([*value as u8]),
        NbtValue::Short(value) => hasher.update(value.to_be_bytes()),
        NbtValue::Int(value) => hasher.update(value.to_be_bytes()),
        NbtValue::Long(value) => hasher.update(value.to_be_bytes()),
        NbtValue::Float(value) => hasher.update(value.to_bits().to_be_bytes()),
        NbtValue::Double(value) => hasher.update(value.to_bits().to_be_bytes()),
        NbtValue::ByteArray(values) => {
            hasher.update((values.len() as u64).to_be_bytes());
            hasher.update(values);
        }
        NbtValue::String(value) => {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value.as_bytes());
        }
        NbtValue::List { kind, items } => {
            hasher.update([*kind]);
            hasher.update((items.len() as u64).to_be_bytes());
            for item in items {
                hash_into(hasher, item);
            }
        }
        NbtValue::Compound(values) => {
            hasher.update((values.len() as u64).to_be_bytes());
            for (key, value) in values {
                hasher.update((key.len() as u64).to_be_bytes());
                hasher.update(key.as_bytes());
                hash_into(hasher, value);
            }
        }
        NbtValue::IntArray(values) => {
            hasher.update((values.len() as u64).to_be_bytes());
            for value in values {
                hasher.update(value.to_be_bytes());
            }
        }
        NbtValue::LongArray(values) => {
            hasher.update((values.len() as u64).to_be_bytes());
            for value in values {
                hasher.update(value.to_be_bytes());
            }
        }
    }
}

fn hash_long_array(values: &[i64]) -> String {
    let mut hasher = Sha256::new();
    hasher.update((values.len() as u64).to_be_bytes());
    for value in values {
        hasher.update(value.to_be_bytes());
    }
    hex(hasher.finalize())
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn list_region_files(folder: &Path) -> Result<Vec<PathBuf>, DerivedDataError> {
    let metadata = match fs::symlink_metadata(folder) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(DerivedDataError::Io(format!(
                "reading {}: {error}",
                folder.display()
            )))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(DerivedDataError::UnsafePath(folder.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(DerivedDataError::InvalidChunk(format!(
            "{} is not a directory",
            folder.display()
        )));
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(folder)
        .map_err(|error| DerivedDataError::Io(format!("reading {}: {error}", folder.display())))?
    {
        let entry = entry.map_err(|error| {
            DerivedDataError::Io(format!("reading {}: {error}", folder.display()))
        })?;
        if files.len() >= MAX_DERIVED_REGION_FILES {
            return Err(DerivedDataError::LimitExceeded(format!(
                "{} has more than {MAX_DERIVED_REGION_FILES} region files",
                folder.display()
            )));
        }
        let file_type = entry.file_type().map_err(|error| {
            DerivedDataError::Io(format!("reading {}: {error}", entry.path().display()))
        })?;
        if file_type.is_symlink() {
            return Err(DerivedDataError::UnsafePath(
                entry.path().display().to_string(),
            ));
        }
        if !file_type.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('r') && name.ends_with(".mca") {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_names_are_stable() {
        assert_eq!(tag_name(&NbtValue::Int(1)), "int");
        assert_eq!(tag_name(&NbtValue::LongArray(vec![1])), "longArray");
    }

    #[test]
    fn fingerprint_is_order_independent_for_sorted_compounds() {
        let left = NbtValue::compound([
            ("a", NbtValue::Int(1)),
            ("b", NbtValue::String("x".to_owned())),
        ]);
        let right = NbtValue::compound([
            ("b", NbtValue::String("x".to_owned())),
            ("a", NbtValue::Int(1)),
        ]);
        assert_eq!(hash_value(&left), hash_value(&right));
        assert_ne!(hash_value(&left), hash_value(&NbtValue::Int(1)));
    }
}
