//! Versioned Java structure (`.nbt`) adapter for DataVersion 4189.
//!
//! Schema (Minecraft Java structure file):
//! ```text
//! {
//!   DataVersion: Int,
//!   size: [Int; 3],
//!   palette: [ { Name: String, Properties?: { String: String } } ],
//!   blocks: [ { state: Int, pos: [Int; 3] } ],
//!   entities: [ ... ],   // not modeled by FuryBuilder
//!   author: String
//! }
//! ```
//!
//! FuryBuilder stores block states and properties only. Entities and block entities are
//! not modeled, so files containing them are reported as incompatible and refused for
//! import rather than silently losing data. Export always writes an empty entity list and
//! reports that native metadata (history, DSL, scene graph) is not part of this format.

use std::collections::BTreeMap;

use thiserror::Error;
use world_domain::BlockState;

use crate::nbt::{self, NbtError, NbtFile, NbtValue, TAG_COMPOUND, TAG_END, TAG_INT};
use crate::structure::{Structure, StructureBlock, StructureError, MAX_STRUCTURE_PALETTE};
use crate::version_support;

pub const JAVA_STRUCTURE_FORMAT: &str = "java_structure";
pub const JAVA_STRUCTURE_EXTENSION: &str = "nbt";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum JavaStructureError {
    #[error(transparent)]
    Nbt(#[from] NbtError),
    #[error("structure file is invalid: {0}")]
    InvalidFormat(String),
    #[error("structure DataVersion {found} is not supported; supported versions: {supported:?}")]
    UnsupportedDataVersion { found: i32, supported: Vec<i32> },
    #[error("structure contains unsupported content: {0}")]
    UnsupportedContent(String),
    #[error("structure limit exceeded: {0}")]
    LimitExceeded(String),
}

impl JavaStructureError {
    pub fn code(&self) -> &'static str {
        match self {
            JavaStructureError::Nbt(error) => error.code(),
            JavaStructureError::InvalidFormat(_) => "unsupported_format",
            JavaStructureError::UnsupportedDataVersion { .. } => "unsupported_version",
            JavaStructureError::UnsupportedContent(_) => "unsupported_format",
            JavaStructureError::LimitExceeded(_) => "limit_exceeded",
        }
    }
}

impl From<StructureError> for JavaStructureError {
    fn from(error: StructureError) -> Self {
        match error {
            StructureError::TooLarge { .. } => JavaStructureError::LimitExceeded(error.to_string()),
            other => JavaStructureError::InvalidFormat(other.to_string()),
        }
    }
}

/// Format-neutral inspection result used by both preview and strict import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureInspection {
    pub format: &'static str,
    pub data_version: i32,
    pub target_data_version: i32,
    pub size: [i32; 3],
    pub volume: u64,
    pub palette_size: usize,
    pub stored_blocks: usize,
    pub non_air_blocks: usize,
    pub entities: usize,
    pub block_entities: usize,
    pub unknown_tags: Vec<String>,
    pub warnings: Vec<String>,
    pub compatible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaStructure {
    pub structure: Structure,
    pub inspection: StructureInspection,
    pub author: Option<String>,
}

struct Parsed {
    structure: Structure,
    inspection: StructureInspection,
    author: Option<String>,
}

fn invalid(message: impl Into<String>) -> JavaStructureError {
    JavaStructureError::InvalidFormat(message.into())
}

fn int_list(value: &NbtValue, field: &str) -> Result<Vec<i32>, JavaStructureError> {
    let (kind, items) = value
        .as_list()
        .ok_or_else(|| invalid(format!("`{field}` must be a list")))?;
    if kind != TAG_INT {
        return Err(invalid(format!("`{field}` must be a list of Int")));
    }
    items
        .iter()
        .map(|item| {
            item.as_int()
                .ok_or_else(|| invalid(format!("`{field}` contains a non-Int element")))
        })
        .collect()
}

fn required<'a>(
    map: &'a BTreeMap<String, NbtValue>,
    key: &str,
    context: &str,
) -> Result<&'a NbtValue, JavaStructureError> {
    map.get(key)
        .ok_or_else(|| invalid(format!("{context} is missing `{key}`")))
}

fn allowed_keys(
    map: &BTreeMap<String, NbtValue>,
    allowed: &[&str],
    context: &str,
    unknown: &mut Vec<String>,
) {
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            unknown.push(format!("{context}.{key}"));
        }
    }
}

fn parse(bytes: &[u8]) -> Result<Parsed, JavaStructureError> {
    let file = nbt::read_gzip(bytes)?;
    let root = file
        .root
        .as_compound()
        .ok_or_else(|| invalid("root tag must be a compound"))?;
    let mut unknown_tags = Vec::new();
    allowed_keys(
        root,
        &[
            "DataVersion",
            "size",
            "palette",
            "blocks",
            "entities",
            "author",
        ],
        "root",
        &mut unknown_tags,
    );

    let data_version = required(root, "DataVersion", "root")?
        .as_int()
        .ok_or_else(|| invalid("`DataVersion` must be an Int"))?;
    let size_values = int_list(required(root, "size", "root")?, "size")?;
    if size_values.len() != 3 {
        return Err(invalid("`size` must contain exactly three Int values"));
    }
    let size = [size_values[0], size_values[1], size_values[2]];

    let palette_value = required(root, "palette", "root")?;
    let (palette_kind, palette_items) = palette_value
        .as_list()
        .ok_or_else(|| invalid("`palette` must be a list"))?;
    if palette_kind != TAG_COMPOUND {
        return Err(invalid("`palette` must be a list of compounds"));
    }
    if palette_items.is_empty() || palette_items.len() > MAX_STRUCTURE_PALETTE {
        return Err(invalid(format!(
            "palette must contain 1..={MAX_STRUCTURE_PALETTE} entries"
        )));
    }
    let mut palette = Vec::with_capacity(palette_items.len());
    for (index, item) in palette_items.iter().enumerate() {
        let entry = item
            .as_compound()
            .ok_or_else(|| invalid(format!("palette[{index}] must be a compound")))?;
        allowed_keys(
            entry,
            &["Name", "Properties"],
            &format!("palette[{index}]"),
            &mut unknown_tags,
        );
        let name = required(entry, "Name", &format!("palette[{index}]"))?
            .as_string()
            .ok_or_else(|| invalid(format!("palette[{index}].Name must be a String")))?;
        let mut properties = BTreeMap::new();
        if let Some(value) = entry.get("Properties") {
            let props = value.as_compound().ok_or_else(|| {
                invalid(format!("palette[{index}].Properties must be a compound"))
            })?;
            for (key, value) in props {
                let value = value.as_string().ok_or_else(|| {
                    invalid(format!(
                        "palette[{index}].Properties.{key} must be a String"
                    ))
                })?;
                properties.insert(key.clone(), value.to_owned());
            }
        }
        let state = BlockState::new(name, properties).map_err(|error| {
            invalid(format!(
                "palette[{index}] has an invalid block state: {error}"
            ))
        })?;
        palette.push(state);
    }

    let blocks_value = required(root, "blocks", "root")?;
    let (blocks_kind, block_items) = blocks_value
        .as_list()
        .ok_or_else(|| invalid("`blocks` must be a list"))?;
    // Minecraft writes an empty list with TAG_End; accept that canonical empty form as
    // well as an empty compound list. A non-empty list must contain compounds.
    if blocks_kind != TAG_COMPOUND && !(blocks_kind == TAG_END && block_items.is_empty()) {
        return Err(invalid("`blocks` must be a list of compounds"));
    }
    let mut blocks = Vec::with_capacity(block_items.len());
    let mut block_entities = 0usize;
    for (index, item) in block_items.iter().enumerate() {
        let entry = item
            .as_compound()
            .ok_or_else(|| invalid(format!("blocks[{index}] must be a compound")))?;
        allowed_keys(
            entry,
            &["state", "pos"],
            &format!("blocks[{index}]"),
            &mut unknown_tags,
        );
        if let Some(value) = entry.get("nbt") {
            // Block entity payloads are not modeled; count them and mark incompatible.
            if value.as_compound().is_some_and(|map| !map.is_empty()) {
                block_entities += 1;
            }
            unknown_tags.push(format!("blocks[{index}].nbt"));
        }
        let state = required(entry, "state", &format!("blocks[{index}]"))?
            .as_int()
            .ok_or_else(|| invalid(format!("blocks[{index}].state must be an Int")))?;
        if state < 0 || state as usize >= palette.len() {
            return Err(invalid(format!(
                "blocks[{index}].state {state} is outside the palette"
            )));
        }
        let pos_values = int_list(required(entry, "pos", &format!("blocks[{index}]"))?, "pos")?;
        if pos_values.len() != 3 {
            return Err(invalid(format!(
                "blocks[{index}].pos must contain exactly three Int values"
            )));
        }
        blocks.push(StructureBlock {
            pos: [pos_values[0], pos_values[1], pos_values[2]],
            palette_index: state as usize,
        });
    }

    let entities = match root.get("entities") {
        None => 0,
        Some(value) => {
            let (kind, items) = value
                .as_list()
                .ok_or_else(|| invalid("`entities` must be a list"))?;
            // Empty lists are encoded as TAG_End by the official game.
            if kind != TAG_COMPOUND && !(kind == TAG_END && items.is_empty()) {
                return Err(invalid("`entities` must be a list of compounds"));
            }
            items.len()
        }
    };
    let author = match root.get("author") {
        None => None,
        Some(value) => Some(
            value
                .as_string()
                .ok_or_else(|| invalid("`author` must be a String"))?
                .to_owned(),
        ),
    };

    let structure = Structure::new(data_version, size, palette, blocks)?;
    let volume = structure.volume()?;
    let non_air_blocks = structure.non_air_blocks();
    let target = version_support::profile_for(data_version)
        .unwrap_or_else(version_support::default_version_profile);
    let data_version_supported = target.data_version == data_version;
    let mut warnings = Vec::new();
    if !data_version_supported {
        warnings.push(format!(
            "DataVersion {data_version} is not supported; FuryBuilder writes {}.",
            target.data_version
        ));
    }
    if entities > 0 {
        warnings.push(format!(
            "{entities} entities are present but FuryBuilder does not model entities."
        ));
    }
    if block_entities > 0 {
        warnings.push(format!(
            "{block_entities} block entities are present but FuryBuilder does not model block entities."
        ));
    }
    if !unknown_tags.is_empty() {
        warnings.push(format!(
            "{} unknown tag(s) would be lost: {}",
            unknown_tags.len(),
            unknown_tags.join(", ")
        ));
    }
    let compatible =
        data_version_supported && entities == 0 && block_entities == 0 && unknown_tags.is_empty();
    let inspection = StructureInspection {
        format: JAVA_STRUCTURE_FORMAT,
        data_version,
        target_data_version: target.data_version,
        size,
        volume,
        palette_size: structure.palette.len(),
        stored_blocks: structure.blocks.len(),
        non_air_blocks,
        entities,
        block_entities,
        unknown_tags,
        warnings,
        compatible,
    };
    Ok(Parsed {
        structure,
        inspection,
        author,
    })
}

/// Permissive inspection for previews. Structural corruption still fails; unsupported but
/// well-formed content is reported through `inspection.compatible` and warnings.
pub fn inspect_java_structure(bytes: &[u8]) -> Result<StructureInspection, JavaStructureError> {
    Ok(parse(bytes)?.inspection)
}

/// Strict read used by insertion. Refuses unsupported versions and any content that would
/// be silently dropped.
pub fn read_java_structure(bytes: &[u8]) -> Result<JavaStructure, JavaStructureError> {
    let parsed = parse(bytes)?;
    if !version_support::supports_data_version(parsed.inspection.data_version) {
        return Err(JavaStructureError::UnsupportedDataVersion {
            found: parsed.inspection.data_version,
            supported: version_support::supported_data_versions(),
        });
    }
    if !parsed.inspection.compatible {
        return Err(JavaStructureError::UnsupportedContent(
            parsed.inspection.warnings.join("; "),
        ));
    }
    Ok(JavaStructure {
        structure: parsed.structure,
        inspection: parsed.inspection,
        author: parsed.author,
    })
}

/// Structural-only read for previews: validates NBT, palette, bounds and indices but does
/// not refuse unsupported DataVersions or content that would be dropped on import. Callers
/// must surface `inspection.compatible` and warnings and must use `read_java_structure` for
/// insertion.
pub fn read_java_structure_unchecked(bytes: &[u8]) -> Result<Structure, JavaStructureError> {
    Ok(parse(bytes)?.structure)
}

/// Writes a deterministic gzip-compressed Java structure for the structure's DataVersion.
pub fn write_java_structure(
    structure: &Structure,
    author: &str,
) -> Result<Vec<u8>, JavaStructureError> {
    if !version_support::supports_data_version(structure.data_version) {
        return Err(JavaStructureError::UnsupportedDataVersion {
            found: structure.data_version,
            supported: version_support::supported_data_versions(),
        });
    }
    structure.validate()?;
    let max_axis = 1024;
    if structure.size.iter().any(|axis| *axis > max_axis) {
        return Err(JavaStructureError::InvalidFormat(format!(
            "structure axis exceeds {max_axis}"
        )));
    }

    let palette = structure
        .palette
        .iter()
        .map(|state| {
            let mut entry = BTreeMap::new();
            entry.insert("Name".to_owned(), NbtValue::String(state.name().to_owned()));
            if !state.properties().is_empty() {
                let properties = state
                    .properties()
                    .iter()
                    .map(|(key, value)| (key.clone(), NbtValue::String(value.clone())))
                    .collect();
                entry.insert("Properties".to_owned(), NbtValue::Compound(properties));
            }
            NbtValue::Compound(entry)
        })
        .collect::<Vec<_>>();

    let mut blocks = structure.blocks.clone();
    blocks.sort_by_key(|block| block.pos);
    let blocks = blocks
        .into_iter()
        .map(|block| {
            NbtValue::compound([
                ("state", NbtValue::Int(block.palette_index as i32)),
                (
                    "pos",
                    NbtValue::int_list([block.pos[0], block.pos[1], block.pos[2]]),
                ),
            ])
        })
        .collect::<Vec<_>>();

    let root = NbtValue::compound([
        ("DataVersion", NbtValue::Int(structure.data_version)),
        (
            "size",
            NbtValue::int_list([structure.size[0], structure.size[1], structure.size[2]]),
        ),
        (
            "palette",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: palette,
            },
        ),
        (
            "blocks",
            NbtValue::List {
                kind: TAG_COMPOUND,
                items: blocks,
            },
        ),
        ("entities", NbtValue::empty_list(TAG_COMPOUND)),
        ("author", NbtValue::String(author.to_owned())),
    ]);
    nbt::write_gzip(&NbtFile::new("", root)).map_err(JavaStructureError::Nbt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_domain::{BlockBox, BlockPos, World};

    fn target_data_version() -> i32 {
        version_support::default_version_profile().data_version
    }

    fn sample_structure() -> Structure {
        let stairs: BlockState =
            "minecraft:oak_stairs[facing=west,half=top,shape=straight,waterlogged=false]"
                .parse()
                .unwrap();
        Structure::new(
            target_data_version(),
            [3, 2, 2],
            vec![BlockState::default(), stairs],
            vec![
                StructureBlock {
                    pos: [0, 0, 0],
                    palette_index: 1,
                },
                StructureBlock {
                    pos: [2, 1, 1],
                    palette_index: 1,
                },
            ],
        )
        .unwrap()
    }

    #[test]
    fn round_trips_states_sizes_and_properties() {
        let structure = sample_structure();
        let bytes = write_java_structure(&structure, "FuryBuilder").unwrap();
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
        let read = read_java_structure(&bytes).unwrap();
        assert_eq!(read.structure, structure);
        assert_eq!(read.author.as_deref(), Some("FuryBuilder"));
        assert_eq!(read.inspection.size, [3, 2, 2]);
        assert_eq!(read.inspection.non_air_blocks, 2);
        assert_eq!(read.inspection.palette_size, 2);
        assert!(read.inspection.compatible);
        assert_eq!(read.inspection.target_data_version, target_data_version());
    }

    #[test]
    fn export_is_deterministic() {
        let structure = sample_structure();
        assert_eq!(
            write_java_structure(&structure, "FuryBuilder").unwrap(),
            write_java_structure(&structure, "FuryBuilder").unwrap()
        );
    }

    #[test]
    fn captures_negative_world_regions() {
        let mut world = World::new_empty();
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(-2, 64, -2),
                &"minecraft:glass".parse().unwrap(),
            )
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(-2, 64, -2), BlockPos::new(0, 66, 0)).unwrap();
        let structure =
            Structure::from_world(&world, "minecraft:overworld", bounds, target_data_version())
                .unwrap();
        let bytes = write_java_structure(&structure, "FuryBuilder").unwrap();
        let read = read_java_structure(&bytes).unwrap();
        assert_eq!(read.structure, structure);
        assert_eq!(read.structure.blocks[0].pos, [0, 0, 0]);
        assert_eq!(
            read.structure.palette[read.structure.blocks[0].palette_index].name(),
            "minecraft:glass"
        );
    }

    #[test]
    fn refuses_unsupported_data_version() {
        let mut structure = sample_structure();
        structure.data_version = 9999;
        assert!(matches!(
            write_java_structure(&structure, "FuryBuilder"),
            Err(JavaStructureError::UnsupportedDataVersion { found: 9999, .. })
        ));
    }

    #[test]
    fn inspection_refuses_silent_loss_of_entities_and_unknown_tags() {
        let raw = {
            let file = NbtFile::new(
                "",
                NbtValue::compound([
                    ("DataVersion", NbtValue::Int(target_data_version())),
                    ("size", NbtValue::int_list([1, 1, 1])),
                    (
                        "palette",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "Name",
                                NbtValue::String("minecraft:stone".to_owned()),
                            )])],
                        },
                    ),
                    (
                        "blocks",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([
                                ("state", NbtValue::Int(0)),
                                ("pos", NbtValue::int_list([0, 0, 0])),
                            ])],
                        },
                    ),
                    (
                        "entities",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "nbt",
                                NbtValue::compound([(
                                    "id",
                                    NbtValue::String("minecraft:pig".to_owned()),
                                )]),
                            )])],
                        },
                    ),
                ]),
            );
            nbt::write_gzip(&file).unwrap()
        };
        let inspection = inspect_java_structure(&raw).unwrap();
        assert!(!inspection.compatible);
        assert_eq!(inspection.entities, 1);
        assert!(inspection
            .warnings
            .iter()
            .any(|warning| warning.contains("entities")));
        assert!(matches!(
            read_java_structure(&raw),
            Err(JavaStructureError::UnsupportedContent(_))
        ));
    }

    #[test]
    fn rejects_malformed_palette_blocks_and_positions() {
        let make = |blocks: Vec<NbtValue>| {
            let file = NbtFile::new(
                "",
                NbtValue::compound([
                    ("DataVersion", NbtValue::Int(target_data_version())),
                    ("size", NbtValue::int_list([1, 1, 1])),
                    (
                        "palette",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![NbtValue::compound([(
                                "Name",
                                NbtValue::String("minecraft:stone".to_owned()),
                            )])],
                        },
                    ),
                    (
                        "blocks",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: blocks,
                        },
                    ),
                ]),
            );
            nbt::write_gzip(&file).unwrap()
        };
        let bad_index = make(vec![NbtValue::compound([
            ("state", NbtValue::Int(4)),
            ("pos", NbtValue::int_list([0, 0, 0])),
        ])]);
        assert!(inspect_java_structure(&bad_index).is_err());

        let bad_pos = make(vec![NbtValue::compound([
            ("state", NbtValue::Int(0)),
            ("pos", NbtValue::int_list([0, 4, 0])),
        ])]);
        assert!(inspect_java_structure(&bad_pos).is_err());

        let missing_palette = {
            let file = NbtFile::new(
                "",
                NbtValue::compound([
                    ("DataVersion", NbtValue::Int(target_data_version())),
                    ("size", NbtValue::int_list([1, 1, 1])),
                    (
                        "palette",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![],
                        },
                    ),
                    (
                        "blocks",
                        NbtValue::List {
                            kind: TAG_COMPOUND,
                            items: vec![],
                        },
                    ),
                ]),
            );
            nbt::write_gzip(&file).unwrap()
        };
        assert!(inspect_java_structure(&missing_palette).is_err());
    }

    #[test]
    fn rejects_corrupt_and_non_gzip_input() {
        assert!(inspect_java_structure(b"not an nbt file").is_err());
        let truncated = write_java_structure(&sample_structure(), "FuryBuilder").unwrap();
        assert!(inspect_java_structure(&truncated[..10]).is_err());
    }
}
