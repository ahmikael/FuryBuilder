//! A bounded, project-local library of reusable Java structure (`.nbt`) templates.
//!
//! Templates saved from the current selection and structures imported by the user live
//! in the project folder under `templates/`. Every entry is verified on write and again
//! on read: byte limit, SHA-256, target `DataVersion`, palette and block validation. The
//! index is small JSON; the payloads are ordinary `.nbt` files, so they remain portable.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use world_domain::{BlockBox, World, TARGET_DATA_VERSION};
use world_io::{Structure, StructureInspection};

pub const LIBRARY_SCHEMA_VERSION: u32 = 1;
pub const INDEX_FILE: &str = "index.json";
pub const OBJECTS_DIR: &str = "objects";
pub const MAX_TEMPLATES: usize = 128;
pub const MAX_TEMPLATE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
pub const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSource {
    Selection,
    Import,
}

impl TemplateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            TemplateSource::Selection => "selection",
            TemplateSource::Import => "import",
        }
    }
}

/// One verified template in the library. `file` is always a relative path below
/// `objects/`; ids are UUIDs, so a name can never become a path component.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEntry {
    pub id: String,
    pub name: String,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub data_version: i32,
    pub size: [i32; 3],
    pub non_air_blocks: u64,
    pub palette_size: usize,
    pub source: TemplateSource,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryIndex {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default)]
    entries: Vec<TemplateEntry>,
}

fn default_schema_version() -> u32 {
    LIBRARY_SCHEMA_VERSION
}

impl Default for LibraryIndex {
    fn default() -> Self {
        Self {
            schema_version: LIBRARY_SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("template library path `{0}` is not safe")]
    UnsafePath(String),
    #[error("template library I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("template library index is invalid: {0}")]
    InvalidIndex(String),
    #[error("template `{0}` was not found")]
    NotFound(String),
    #[error("template name must be 1..={0} characters")]
    InvalidName(usize),
    #[error("template library holds {found} entries, limit is {limit}")]
    TooManyTemplates { found: usize, limit: usize },
    #[error("template is {bytes} bytes, limit is {limit}")]
    TemplateTooLarge { bytes: u64, limit: u64 },
    #[error("template library would use {bytes} bytes, limit is {limit}")]
    LibraryTooLarge { bytes: u64, limit: u64 },
    #[error("template payload failed verification: {0}")]
    InvalidStructure(String),
    #[error("template `{id}` failed checksum verification")]
    ChecksumMismatch { id: String },
    #[error("template `{id}` changed size after it was verified")]
    SizeMismatch { id: String },
}

impl LibraryError {
    pub fn code(&self) -> &'static str {
        match self {
            LibraryError::TooManyTemplates { .. }
            | LibraryError::TemplateTooLarge { .. }
            | LibraryError::LibraryTooLarge { .. } => "limit_exceeded",
            LibraryError::NotFound(_) => "not_found",
            LibraryError::ChecksumMismatch { .. }
            | LibraryError::SizeMismatch { .. }
            | LibraryError::InvalidIndex(_)
            | LibraryError::InvalidStructure(_)
            | LibraryError::UnsafePath(_) => "invalid_argument",
            LibraryError::InvalidName(_) => "invalid_argument",
            LibraryError::Io(_) => "internal",
        }
    }
}

/// Project-local library rooted at the project's `templates/` directory.
#[derive(Debug, Clone)]
pub struct StructureLibrary {
    root: PathBuf,
}

impl StructureLibrary {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    fn objects_dir(&self) -> PathBuf {
        self.root.join(OBJECTS_DIR)
    }

    fn load_index(&self) -> Result<LibraryIndex, LibraryError> {
        let path = self.index_path();
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LibraryIndex::default());
            }
            Err(error) => return Err(error.into()),
        };
        if metadata.len() > MAX_INDEX_BYTES {
            return Err(LibraryError::InvalidIndex(format!(
                "index is {} bytes, limit is {MAX_INDEX_BYTES}",
                metadata.len()
            )));
        }
        let bytes = fs::read(&path)?;
        let index: LibraryIndex = serde_json::from_slice(&bytes)
            .map_err(|error| LibraryError::InvalidIndex(error.to_string()))?;
        self.validate_index(&index)?;
        Ok(index)
    }

    fn validate_index(&self, index: &LibraryIndex) -> Result<(), LibraryError> {
        if index.schema_version != LIBRARY_SCHEMA_VERSION {
            return Err(LibraryError::InvalidIndex(format!(
                "schema version {} is not supported",
                index.schema_version
            )));
        }
        if index.entries.len() > MAX_TEMPLATES {
            return Err(LibraryError::TooManyTemplates {
                found: index.entries.len(),
                limit: MAX_TEMPLATES,
            });
        }
        let mut ids = BTreeSet::new();
        let mut files = BTreeSet::new();
        let mut total = 0u64;
        for entry in &index.entries {
            if Uuid::parse_str(&entry.id).is_err() {
                return Err(LibraryError::InvalidIndex(format!(
                    "template id `{}` is not a UUID",
                    entry.id
                )));
            }
            if !ids.insert(entry.id.as_str()) {
                return Err(LibraryError::InvalidIndex(format!(
                    "template id `{}` is duplicated",
                    entry.id
                )));
            }
            let expected_file = format!("{OBJECTS_DIR}/{}.nbt", entry.id);
            if entry.file != expected_file {
                return Err(LibraryError::InvalidIndex(format!(
                    "template `{}` points outside the objects directory",
                    entry.id
                )));
            }
            if !files.insert(entry.file.as_str()) {
                return Err(LibraryError::InvalidIndex(format!(
                    "template file `{}` is duplicated",
                    entry.file
                )));
            }
            if entry.bytes > MAX_TEMPLATE_BYTES {
                return Err(LibraryError::TemplateTooLarge {
                    bytes: entry.bytes,
                    limit: MAX_TEMPLATE_BYTES,
                });
            }
            if entry.sha256.len() != 64
                || !entry
                    .sha256
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(LibraryError::InvalidIndex(format!(
                    "template `{}` has an invalid checksum",
                    entry.id
                )));
            }
            total = total.saturating_add(entry.bytes);
        }
        if total > MAX_TOTAL_BYTES {
            return Err(LibraryError::LibraryTooLarge {
                bytes: total,
                limit: MAX_TOTAL_BYTES,
            });
        }
        Ok(())
    }

    fn write_index(&self, index: &LibraryIndex) -> Result<(), LibraryError> {
        self.validate_index(index)?;
        fs::create_dir_all(&self.root)?;
        let bytes = serde_json::to_vec_pretty(index)
            .map_err(|error| LibraryError::InvalidIndex(error.to_string()))?;
        write_atomic(&self.index_path(), &bytes)
    }

    pub fn list(&self) -> Result<Vec<TemplateEntry>, LibraryError> {
        let mut entries = self.load_index()?.entries;
        entries.sort_by(|left, right| {
            right
                .created_at_ms
                .cmp(&left.created_at_ms)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    }

    pub fn get(&self, id: &str) -> Result<TemplateEntry, LibraryError> {
        if Uuid::parse_str(id).is_err() {
            return Err(LibraryError::NotFound(id.to_owned()));
        }
        self.load_index()?
            .entries
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| LibraryError::NotFound(id.to_owned()))
    }

    /// Saves raw Java structure bytes as a new template. The bytes are fully parsed and
    /// verified for the target `DataVersion`; entities and block entities are rejected by
    /// the existing import adapter.
    pub fn save_bytes(
        &self,
        name: &str,
        bytes: &[u8],
        source: TemplateSource,
    ) -> Result<TemplateEntry, LibraryError> {
        let name = normalize_name(name)?;
        if bytes.len() as u64 > MAX_TEMPLATE_BYTES {
            return Err(LibraryError::TemplateTooLarge {
                bytes: bytes.len() as u64,
                limit: MAX_TEMPLATE_BYTES,
            });
        }
        let parsed = world_io::read_verified_import(bytes).map_err(|error| {
            LibraryError::InvalidStructure(format!("{}: {}", error.code(), error))
        })?;
        let mut index = self.load_index()?;
        if index.entries.len() >= MAX_TEMPLATES {
            return Err(LibraryError::TooManyTemplates {
                found: index.entries.len() + 1,
                limit: MAX_TEMPLATES,
            });
        }
        let total = index.entries.iter().fold(bytes.len() as u64, |sum, entry| {
            sum.saturating_add(entry.bytes)
        });
        if total > MAX_TOTAL_BYTES {
            return Err(LibraryError::LibraryTooLarge {
                bytes: total,
                limit: MAX_TOTAL_BYTES,
            });
        }
        let id = Uuid::new_v4().to_string();
        let file = format!("{OBJECTS_DIR}/{id}.nbt");
        let entry =
            entry_from_inspection(id, name, file, bytes, &parsed.inspection, source, now_ms());
        fs::create_dir_all(self.objects_dir())?;
        write_atomic(&self.root.join(&entry.file), bytes)?;
        index.entries.push(entry.clone());
        if let Err(error) = self.write_index(&index) {
            let _ = fs::remove_file(self.root.join(&entry.file));
            return Err(error);
        }
        Ok(entry)
    }

    /// Saves a bounded selection from the authoritative world as a new template.
    pub fn save_from_world(
        &self,
        name: &str,
        world: &World,
        dimension: &str,
        bounds: BlockBox,
        source: TemplateSource,
    ) -> Result<TemplateEntry, LibraryError> {
        let structure = Structure::from_world(world, dimension, bounds, TARGET_DATA_VERSION)
            .map_err(|error| LibraryError::InvalidStructure(error.to_string()))?;
        let bytes = world_io::write_java_structure(&structure, "FuryBuilder")
            .map_err(|error| LibraryError::InvalidStructure(error.to_string()))?;
        self.save_bytes(name, &bytes, source)
    }

    /// Reads a stored payload after size and checksum verification, without parsing it.
    /// Callers that need `StructureInspection` re-run the verified import adapter.
    pub fn load_bytes(&self, id: &str) -> Result<(Vec<u8>, TemplateEntry), LibraryError> {
        let entry = self.get(id)?;
        let path = self.root.join(&entry.file);
        let metadata = fs::metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                LibraryError::NotFound(id.to_owned())
            } else {
                LibraryError::Io(error)
            }
        })?;
        if metadata.len() != entry.bytes {
            return Err(LibraryError::SizeMismatch { id: id.to_owned() });
        }
        if metadata.len() > MAX_TEMPLATE_BYTES {
            return Err(LibraryError::TemplateTooLarge {
                bytes: metadata.len(),
                limit: MAX_TEMPLATE_BYTES,
            });
        }
        let bytes = fs::read(&path)?;
        if checksum(&bytes) != entry.sha256 {
            return Err(LibraryError::ChecksumMismatch { id: id.to_owned() });
        }
        Ok((bytes, entry))
    }

    /// Reads and re-verifies a stored template. A changed file, truncated payload or
    /// checksum mismatch fails closed instead of returning partially valid blocks.
    pub fn load(&self, id: &str) -> Result<(Structure, TemplateEntry), LibraryError> {
        let entry = self.get(id)?;
        let path = self.root.join(&entry.file);
        let metadata = fs::metadata(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                LibraryError::NotFound(id.to_owned())
            } else {
                LibraryError::Io(error)
            }
        })?;
        if metadata.len() != entry.bytes {
            return Err(LibraryError::SizeMismatch { id: id.to_owned() });
        }
        if metadata.len() > MAX_TEMPLATE_BYTES {
            return Err(LibraryError::TemplateTooLarge {
                bytes: metadata.len(),
                limit: MAX_TEMPLATE_BYTES,
            });
        }
        let bytes = fs::read(&path)?;
        if checksum(&bytes) != entry.sha256 {
            return Err(LibraryError::ChecksumMismatch { id: id.to_owned() });
        }
        let structure = world_io::read_java_structure(&bytes)
            .map_err(|error| LibraryError::InvalidStructure(error.to_string()))?;
        if structure.structure.size != entry.size {
            return Err(LibraryError::InvalidStructure(format!(
                "template `{id}` size no longer matches its index entry"
            )));
        }
        Ok((structure.structure, entry))
    }

    pub fn delete(&self, id: &str) -> Result<TemplateEntry, LibraryError> {
        let mut index = self.load_index()?;
        let position = index
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| LibraryError::NotFound(id.to_owned()))?;
        let entry = index.entries.remove(position);
        self.write_index(&index)?;
        match fs::remove_file(self.root.join(&entry.file)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(entry)
    }
}

fn normalize_name(name: &str) -> Result<String, LibraryError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(LibraryError::InvalidName(MAX_NAME_CHARS));
    }
    Ok(trimmed.to_owned())
}

fn entry_from_inspection(
    id: String,
    name: String,
    file: String,
    bytes: &[u8],
    inspection: &StructureInspection,
    source: TemplateSource,
    created_at_ms: u64,
) -> TemplateEntry {
    TemplateEntry {
        id,
        name,
        file,
        bytes: bytes.len() as u64,
        sha256: checksum(bytes),
        data_version: inspection.data_version,
        size: inspection.size,
        non_air_blocks: inspection.non_air_blocks as u64,
        palette_size: inspection.palette_size,
        source,
        created_at_ms,
    }
}

pub fn checksum(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), LibraryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| LibraryError::UnsafePath(path.display().to_string()))?;
    let temp = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    fs::write(&temp, bytes)?;
    // Windows refuses to replace an existing destination with `rename`; removing the
    // old index first leaves the library readable because payloads are written earlier.
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        LibraryError::Io(error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_domain::{BlockPos, BlockState};

    fn stone() -> BlockState {
        BlockState::new("minecraft:stone", Default::default()).expect("state")
    }

    fn example_bytes() -> Vec<u8> {
        let structure = Structure::new(
            TARGET_DATA_VERSION,
            [2, 1, 2],
            vec![stone()],
            vec![
                world_io::StructureBlock {
                    pos: [0, 0, 0],
                    palette_index: 0,
                },
                world_io::StructureBlock {
                    pos: [1, 0, 0],
                    palette_index: 0,
                },
                world_io::StructureBlock {
                    pos: [0, 0, 1],
                    palette_index: 0,
                },
                world_io::StructureBlock {
                    pos: [1, 0, 1],
                    palette_index: 0,
                },
            ],
        )
        .expect("structure");
        world_io::write_java_structure(&structure, "Test").expect("bytes")
    }

    fn library() -> (tempfile::TempDir, StructureLibrary) {
        let dir = tempfile::tempdir().expect("tempdir");
        let library = StructureLibrary::new(dir.path().join("templates"));
        (dir, library)
    }

    #[test]
    fn saves_lists_loads_and_deletes_verified_templates() {
        let (_dir, library) = library();
        let entry = library
            .save_bytes("Small floor", &example_bytes(), TemplateSource::Selection)
            .expect("save");
        assert_eq!(entry.name, "Small floor");
        assert_eq!(entry.size, [2, 1, 2]);
        assert_eq!(entry.non_air_blocks, 4);
        assert_eq!(entry.source, TemplateSource::Selection);
        assert_eq!(entry.data_version, TARGET_DATA_VERSION);
        assert!(entry.file.starts_with("objects/"));

        let listed = library.list().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, entry.id);

        let (structure, loaded) = library.load(&entry.id).expect("load");
        assert_eq!(loaded, entry);
        assert_eq!(structure.size, [2, 1, 2]);
        assert_eq!(structure.non_air_blocks(), 4);

        let deleted = library.delete(&entry.id).expect("delete");
        assert_eq!(deleted.id, entry.id);
        assert!(library.list().expect("list").is_empty());
        assert!(matches!(
            library.load(&entry.id),
            Err(LibraryError::NotFound(_))
        ));
    }

    #[test]
    fn saves_a_world_selection() {
        let (_dir, library) = library();
        let mut world = World::new_empty();
        world
            .set_block("minecraft:overworld", BlockPos::new(0, 64, 0), &stone())
            .expect("set");
        world
            .set_block("minecraft:overworld", BlockPos::new(1, 64, 0), &stone())
            .expect("set");
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)).expect("box");
        let entry = library
            .save_from_world(
                "Selection",
                &world,
                "minecraft:overworld",
                bounds,
                TemplateSource::Selection,
            )
            .expect("save from world");
        assert_eq!(entry.non_air_blocks, 2);
        assert_eq!(entry.size, [2, 1, 1]);
    }

    #[test]
    fn rejects_invalid_payloads_and_names() {
        let (_dir, library) = library();
        assert!(matches!(
            library.save_bytes("Bad", b"not nbt", TemplateSource::Import),
            Err(LibraryError::InvalidStructure(_))
        ));
        assert!(matches!(
            library.save_bytes("   ", &example_bytes(), TemplateSource::Import),
            Err(LibraryError::InvalidName(_))
        ));
        let long = "x".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(
            library.save_bytes(&long, &example_bytes(), TemplateSource::Import),
            Err(LibraryError::InvalidName(_))
        ));
    }

    #[test]
    fn changed_payload_fails_checksum_verification() {
        let (_dir, library) = library();
        let entry = library
            .save_bytes("Verified", &example_bytes(), TemplateSource::Import)
            .expect("save");
        let path = library.root().join(&entry.file);
        let mut bytes = fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&path, &bytes).expect("tamper");
        assert!(matches!(
            library.load(&entry.id),
            Err(LibraryError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn truncated_payload_fails_size_verification() {
        let (_dir, library) = library();
        let entry = library
            .save_bytes("Verified", &example_bytes(), TemplateSource::Import)
            .expect("save");
        let path = library.root().join(&entry.file);
        fs::write(&path, b"short").expect("truncate");
        assert!(matches!(
            library.load(&entry.id),
            Err(LibraryError::SizeMismatch { .. })
        ));
    }

    #[test]
    fn index_rejects_path_traversal_and_duplicate_ids() {
        let (_dir, library) = library();
        fs::create_dir_all(library.root()).expect("root");
        let id = Uuid::new_v4().to_string();
        let traversal = format!(
            r#"{{"schemaVersion":1,"entries":[{{"id":"{id}","name":"Escape","file":"../../outside.nbt","bytes":10,"sha256":"{}","dataVersion":4189,"size":[1,1,1],"nonAirBlocks":1,"paletteSize":1,"source":"import","createdAtMs":0}}]}}"#,
            "a".repeat(64)
        );
        fs::write(library.index_path(), traversal).expect("index");
        assert!(matches!(library.list(), Err(LibraryError::InvalidIndex(_))));

        let duplicate = format!(
            r#"{{"schemaVersion":1,"entries":[
                {{"id":"{id}","name":"One","file":"objects/{id}.nbt","bytes":10,"sha256":"{}","dataVersion":4189,"size":[1,1,1],"nonAirBlocks":1,"paletteSize":1,"source":"import","createdAtMs":0}},
                {{"id":"{id}","name":"Two","file":"objects/{id}.nbt","bytes":10,"sha256":"{}","dataVersion":4189,"size":[1,1,1],"nonAirBlocks":1,"paletteSize":1,"source":"import","createdAtMs":0}}
            ]}}"#,
            "a".repeat(64),
            "a".repeat(64)
        );
        fs::write(library.index_path(), duplicate).expect("index");
        assert!(matches!(library.list(), Err(LibraryError::InvalidIndex(_))));
    }

    #[test]
    fn unknown_and_malformed_ids_are_not_used_as_paths() {
        let (_dir, library) = library();
        assert!(matches!(
            library.get("../project.json"),
            Err(LibraryError::NotFound(_))
        ));
        assert!(matches!(
            library.load("not-a-uuid"),
            Err(LibraryError::NotFound(_))
        ));
    }
}
