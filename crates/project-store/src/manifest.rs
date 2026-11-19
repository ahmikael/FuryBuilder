use std::time::{SystemTime, UNIX_EPOCH};

use scene_model::{ParametricScene, Scene};
use serde::{Deserialize, Serialize};
use workspace_model::WorkspacePolicy;
use world_domain::{TARGET_DATA_VERSION, TARGET_MINECRAFT_VERSION};

use crate::error::ProjectError;

pub const PROJECT_SCHEMA_VERSION: u32 = 1;
pub const PROJECT_EXTENSION: &str = "furybuilder";
pub const PROJECT_FILE: &str = "project.json";
pub const MAX_PROJECT_NAME_CHARS: usize = 64;
pub const DEFAULT_LOAD_RADIUS: u8 = 1;
pub const MIN_LOAD_RADIUS: u8 = 1;
pub const MAX_LOAD_RADIUS: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationMode {
    Empty,
    Flat,
    Vanilla,
    Import,
}

impl CreationMode {
    pub fn is_available(self) -> bool {
        matches!(self, CreationMode::Empty | CreationMode::Flat)
    }

    pub fn label(self) -> &'static str {
        match self {
            CreationMode::Empty => "empty",
            CreationMode::Flat => "flat",
            CreationMode::Vanilla => "vanilla",
            CreationMode::Import => "import",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveKind {
    Manual,
    Autosave,
    Checkpoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Generated,
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub kind: SourceKind,
    pub immutable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl SourceInfo {
    pub fn generated() -> Self {
        Self {
            kind: SourceKind::Generated,
            immutable: true,
            reference: None,
        }
    }
}

/// One validated local data pack that participated in Vanilla World generation. This is
/// provenance for reproducing the original worldgen configuration, not a reference to a
/// still-existing external file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VanillaDatapack {
    pub name: String,
    pub sha256: String,
    pub size: u64,
    pub pack_format: u32,
}

/// Exact identity of the original generator and the requested area. It is stored in
/// `project.json` so a Vanilla World project never loses why its base looks the way it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VanillaProvenance {
    pub seed: String,
    pub preset: String,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    pub generate_structures: bool,
    pub datapacks: Vec<VanillaDatapack>,
    pub minecraft_version: String,
    pub data_version: i32,
    pub java_version: String,
    pub server_jar_sha1: String,
    pub run_id: String,
    pub generated_at_ms: u64,
    pub chunks: u64,
    pub region_files: u64,
    pub world_bytes: u64,
}

impl VanillaProvenance {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.seed.trim().is_empty()
            || self.seed.len() > 64
            || !self.seed.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            })
        {
            return Err(ProjectError::invalid_manifest(
                "vanilla seed must be 1..=64 ASCII letters, digits, '-', '_' or '.'",
            ));
        }
        if !matches!(
            self.preset.as_str(),
            "normal" | "flat" | "large_biomes" | "amplified"
        ) {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance has an unsupported preset",
            ));
        }
        if self.radius_chunks == 0 || self.radius_chunks > 16 {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance radius must be 1..=16 chunks",
            ));
        }
        if self.minecraft_version != TARGET_MINECRAFT_VERSION
            || self.data_version != TARGET_DATA_VERSION
        {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance does not match the supported target version",
            ));
        }
        if self.java_version.trim().is_empty() || self.java_version.len() > 64 {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance java version is empty or too long",
            ));
        }
        if self.server_jar_sha1.len() != 40
            || !self
                .server_jar_sha1
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance server SHA-1 must be 40 lowercase hex characters",
            ));
        }
        if self.run_id.len() > 128
            || !self
                .run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance run id is invalid",
            ));
        }
        if self.datapacks.len() > 16 {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance has too many data packs",
            ));
        }
        for datapack in &self.datapacks {
            if datapack.name.is_empty()
                || datapack.name.len() > 255
                || datapack.sha256.len() != 64
                || !datapack
                    .sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || datapack.pack_format == 0
            {
                return Err(ProjectError::invalid_manifest(
                    "vanilla provenance data pack is invalid",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorState {
    pub selected_dimension: String,
    pub load_radius: u8,
    /// Layers, masks and protected regions. Omitted when the default policy is used,
    /// so projects written before M9.13 stay byte-identical until the user changes it.
    #[serde(default, skip_serializing_if = "workspace_is_default")]
    pub workspace: WorkspacePolicy,
    /// Editor-only semantic objects (districts, buildings, roads). Omitted when empty so
    /// projects written before M9.14 remain byte-compatible.
    #[serde(default, skip_serializing_if = "scene_is_default")]
    pub scene: Scene,
    /// Parametric recipes and rollback baselines. Omitted while the catalog is empty.
    #[serde(default, skip_serializing_if = "parametric_is_default")]
    pub parametric: ParametricScene,
}

fn workspace_is_default(workspace: &WorkspacePolicy) -> bool {
    workspace == &WorkspacePolicy::default()
}

fn scene_is_default(scene: &Scene) -> bool {
    scene == &Scene::default()
}

fn parametric_is_default(parametric: &ParametricScene) -> bool {
    parametric == &ParametricScene::default()
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            selected_dimension: "minecraft:overworld".to_owned(),
            load_radius: DEFAULT_LOAD_RADIUS,
            workspace: WorkspacePolicy::default(),
            scene: Scene::default(),
            parametric: ParametricScene::default(),
        }
    }
}

impl EditorState {
    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.load_radius < MIN_LOAD_RADIUS || self.load_radius > MAX_LOAD_RADIUS {
            return Err(ProjectError::invalid_manifest(format!(
                "loadRadius must be {MIN_LOAD_RADIUS}..={MAX_LOAD_RADIUS}"
            )));
        }
        if self.selected_dimension.is_empty() || self.selected_dimension.len() > 128 {
            return Err(ProjectError::invalid_manifest(
                "selectedDimension is empty or too long",
            ));
        }
        self.workspace
            .validate()
            .map_err(|error| ProjectError::invalid_manifest(format!("workspace: {error}")))?;
        self.scene
            .validate()
            .map_err(|error| ProjectError::invalid_manifest(format!("scene: {error}")))?;
        self.parametric
            .validate()
            .map_err(|error| ProjectError::invalid_manifest(format!("parametric: {error}")))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub project_schema_version: u32,
    pub project_id: String,
    pub name: String,
    pub creation_mode: CreationMode,
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub world_revision: u64,
    /// Relative path below the project folder, always inside `data/`.
    pub world_file: String,
    /// SHA-256 of the snapshot bytes, lowercase hex.
    pub world_checksum: String,
    pub world_bytes: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub last_saved_at_ms: u64,
    pub last_save_kind: SaveKind,
    pub editor_state: EditorState,
    pub source: SourceInfo,
    /// Present only for `creation_mode = vanilla`; records the original generator identity,
    /// requested area and data packs. Optional so existing schema-1 projects still load.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vanilla: Option<VanillaProvenance>,
    pub created_by: String,
    pub app_version: String,
}

impl ProjectManifest {
    pub fn new(
        project_id: String,
        name: String,
        creation_mode: CreationMode,
        revision: u64,
        now_ms: u64,
    ) -> Self {
        Self {
            project_schema_version: PROJECT_SCHEMA_VERSION,
            project_id,
            name,
            creation_mode,
            target_minecraft_version: TARGET_MINECRAFT_VERSION.to_owned(),
            target_data_version: TARGET_DATA_VERSION,
            world_revision: revision,
            world_file: String::new(),
            world_checksum: String::new(),
            world_bytes: 0,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            last_saved_at_ms: now_ms,
            last_save_kind: SaveKind::Manual,
            editor_state: EditorState::default(),
            source: SourceInfo::generated(),
            vanilla: None,
            created_by: "FuryBuilder".to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        if self.project_schema_version != PROJECT_SCHEMA_VERSION {
            return Err(ProjectError::invalid_manifest(format!(
                "expected schema {}, got {}",
                PROJECT_SCHEMA_VERSION, self.project_schema_version
            )));
        }
        if self.project_id.is_empty() || self.project_id.len() > 128 {
            return Err(ProjectError::invalid_manifest(
                "projectId is empty or too long",
            ));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > MAX_PROJECT_NAME_CHARS {
            return Err(ProjectError::invalid_manifest("name is empty or too long"));
        }
        if self.target_minecraft_version != TARGET_MINECRAFT_VERSION
            || self.target_data_version != TARGET_DATA_VERSION
        {
            return Err(ProjectError::UnsupportedSchema {
                found: self.target_data_version.max(0) as u32,
                supported: TARGET_DATA_VERSION as u32,
            });
        }
        validate_relative_data_file(&self.world_file)?;
        if self.world_checksum.len() != 64
            || !self
                .world_checksum
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ProjectError::invalid_manifest(
                "worldChecksum must be a lowercase SHA-256 hex string",
            ));
        }
        self.editor_state.validate()?;
        if self.source.kind == SourceKind::Imported && self.source.reference.is_none() {
            return Err(ProjectError::invalid_manifest(
                "imported projects need a source reference",
            ));
        }
        if self.creation_mode == CreationMode::Vanilla {
            let vanilla = self.vanilla.as_ref().ok_or_else(|| {
                ProjectError::invalid_manifest("vanilla projects need generator provenance")
            })?;
            vanilla.validate()?;
        } else if self.vanilla.is_some() {
            return Err(ProjectError::invalid_manifest(
                "generator provenance is only valid for vanilla projects",
            ));
        }
        Ok(())
    }

    pub fn set_world_payload(&mut self, world_file: String, checksum: String, bytes: u64) {
        self.world_file = world_file;
        self.world_checksum = checksum;
        self.world_bytes = bytes;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub folder: String,
    pub project_id: String,
    pub name: String,
    pub creation_mode: CreationMode,
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub world_revision: u64,
    pub updated_at_ms: u64,
    pub last_saved_at_ms: u64,
    pub last_save_kind: SaveKind,
    pub bytes: u64,
    pub schema_version: u32,
    /// True when a previous session marker exists or the active snapshot needs recovery.
    pub recoverable: bool,
}

impl ProjectSummary {
    pub fn from_manifest(folder: &str, manifest: &ProjectManifest, recoverable: bool) -> Self {
        Self {
            folder: folder.to_owned(),
            project_id: manifest.project_id.clone(),
            name: manifest.name.clone(),
            creation_mode: manifest.creation_mode,
            target_minecraft_version: manifest.target_minecraft_version.clone(),
            target_data_version: manifest.target_data_version,
            world_revision: manifest.world_revision,
            updated_at_ms: manifest.updated_at_ms,
            last_saved_at_ms: manifest.last_saved_at_ms,
            last_save_kind: manifest.last_save_kind,
            bytes: manifest.world_bytes,
            schema_version: manifest.project_schema_version,
            recoverable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub summary: ProjectSummary,
    pub editor_state: EditorState,
    pub source: SourceInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vanilla: Option<VanillaProvenance>,
    pub created_at_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryInfo {
    pub reason: String,
    pub recovered_revision: u64,
    pub recovered_at_ms: u64,
    pub from_backup: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectList {
    pub projects: Vec<ProjectSummary>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    pub project: ProjectDetail,
    pub checksum: String,
    pub snapshot_file: String,
    pub bytes_written: u64,
    pub backup_created: bool,
    pub pruned_snapshots: usize,
}

/// The full open payload. `world` is not serializable on purpose; the IPC layer converts
/// it into the same `WorldInfo` used by every other command.
#[derive(Debug, Clone)]
pub struct OpenedProject {
    pub detail: ProjectDetail,
    pub world: world_domain::World,
    pub recovery: Option<RecoveryInfo>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

pub fn normalize_name(raw: &str) -> Result<String, ProjectError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidName(
            "name must not be empty".to_owned(),
        ));
    }
    if trimmed.chars().count() > MAX_PROJECT_NAME_CHARS {
        return Err(ProjectError::InvalidName(format!(
            "name must be at most {MAX_PROJECT_NAME_CHARS} characters"
        )));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(ProjectError::InvalidName(
            "name must not be a path component".to_owned(),
        ));
    }
    if trimmed
        .chars()
        .any(|character| character.is_control() || matches!(character, '\\' | '/' | ':'))
    {
        return Err(ProjectError::InvalidName(
            "name contains a path separator or control character".to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

/// Turns a display name into a single safe folder component.
pub fn folder_component(name: &str) -> String {
    let mut cleaned = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_control()
            || matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            )
        {
            cleaned.push('-');
        } else {
            cleaned.push(character);
        }
    }
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let cleaned = if cleaned.is_empty() {
        "project"
    } else {
        cleaned
    };
    format!("{cleaned}.{PROJECT_EXTENSION}")
}

pub fn validate_folder(folder: &str) -> Result<(), ProjectError> {
    if folder.is_empty()
        || folder.len() > 160
        || folder == "."
        || folder == ".."
        || folder.contains('/')
        || folder.contains('\\')
        || folder.contains(':')
        || folder.contains('\0')
    {
        return Err(ProjectError::InvalidPath(folder.to_owned()));
    }
    if !folder.ends_with(&format!(".{PROJECT_EXTENSION}"))
        || folder.len() <= PROJECT_EXTENSION.len() + 1
    {
        return Err(ProjectError::InvalidPath(format!(
            "project folder must end with .{PROJECT_EXTENSION}"
        )));
    }
    Ok(())
}

pub fn validate_relative_data_file(path: &str) -> Result<(), ProjectError> {
    let mut components = std::path::Path::new(path).components();
    let first = components.next();
    let second = components.next();
    if components.next().is_some() {
        return Err(ProjectError::invalid_manifest(
            "worldFile must be a path below data/",
        ));
    }
    let valid = matches!(
        (first, second),
        (
            Some(std::path::Component::Normal(dir)),
            Some(std::path::Component::Normal(file))
        ) if dir == "data" && file.to_string_lossy().starts_with("world-") && file.to_string_lossy().ends_with(".fwl")
    );
    if !valid {
        return Err(ProjectError::invalid_manifest(
            "worldFile must look like data/world-<revision>.fwl",
        ));
    }
    Ok(())
}

pub fn is_hex_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
