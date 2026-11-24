use std::time::{SystemTime, UNIX_EPOCH};

use component_model::ComponentMode;
use minecraft_version::{builtin_profiles, profile_for_version, VersionId, VersionProfile};
use scene_model::{ParametricScene, Scene};
use serde::{Deserialize, Serialize};
use workspace_model::WorkspacePolicy;
use world_domain::WorldVersion;

use crate::error::ProjectError;

/// The only project schema FuryBuilder reads or writes. Older drafts are not migrated.
pub const PROJECT_SCHEMA_VERSION: u32 = 2;
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
    pub fn validate(&self, target: &WorldVersion) -> Result<(), ProjectError> {
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
        if self.minecraft_version != target.minecraft_version()
            || self.data_version != target.data_version()
        {
            return Err(ProjectError::invalid_manifest(
                "vanilla provenance does not match the project target version",
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

/// Exact Minecraft target of a project. The profile id and all derived facts are fixed
/// when the project is created, so a later catalog update can never silently retarget it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTarget {
    pub minecraft_version: VersionId,
    pub data_version: i32,
    pub profile_id: String,
    pub java_major: u32,
    pub resource_pack_format: Option<u32>,
    pub datapack_format: Option<u32>,
}

impl ProjectTarget {
    pub fn from_profile(profile: &VersionProfile) -> Self {
        Self {
            minecraft_version: profile.minecraft_version.clone(),
            data_version: profile.data_version,
            profile_id: profile.id.clone(),
            java_major: profile.java.recommended_major,
            resource_pack_format: profile.resource_pack_format,
            datapack_format: profile.datapack_format,
        }
    }

    /// Resolves the built-in profile for a world version. Unknown versions are refused:
    /// a project target is never guessed or silently replaced by the default profile.
    pub fn for_world_version(version: &WorldVersion) -> Result<Self, ProjectError> {
        let version_id: VersionId = version.minecraft_version().parse().map_err(|error| {
            ProjectError::invalid_manifest(format!("invalid target Minecraft version: {error}"))
        })?;
        let profile = profile_for_version(&version_id)
            .filter(|profile| {
                profile.capabilities.write && profile.data_version == version.data_version()
            })
            .ok_or_else(|| {
                ProjectError::invalid_manifest(format!(
                    "no writable version profile matches Minecraft {} (DataVersion {})",
                    version.minecraft_version(),
                    version.data_version()
                ))
            })?;
        Ok(Self::from_profile(&profile))
    }

    pub fn validate(&self) -> Result<(), ProjectError> {
        let profile = builtin_profiles()
            .iter()
            .find(|profile| profile.id == self.profile_id)
            .ok_or_else(|| {
                ProjectError::invalid_manifest(format!(
                    "target profile `{}` is not built into this FuryBuilder",
                    self.profile_id
                ))
            })?;
        if !profile.capabilities.write || profile.pack_format_minors.is_some() {
            return Err(ProjectError::UnsupportedWorldVersion(profile.id.clone()));
        }
        if self.minecraft_version != profile.minecraft_version {
            return Err(ProjectError::invalid_manifest(format!(
                "target Minecraft version `{}` does not match profile `{}` ({})",
                self.minecraft_version, profile.id, profile.minecraft_version
            )));
        }
        if self.data_version != profile.data_version {
            return Err(ProjectError::invalid_manifest(format!(
                "target DataVersion {} does not match profile `{}` ({})",
                self.data_version, profile.id, profile.data_version
            )));
        }
        if self.java_major != profile.java.recommended_major {
            return Err(ProjectError::invalid_manifest(format!(
                "target Java major {} does not match profile `{}` ({})",
                self.java_major, profile.id, profile.java.recommended_major
            )));
        }
        if self.resource_pack_format != profile.resource_pack_format {
            return Err(ProjectError::invalid_manifest(format!(
                "target resource pack format {:?} does not match profile `{}` ({:?})",
                self.resource_pack_format, profile.id, profile.resource_pack_format
            )));
        }
        if self.datapack_format != profile.datapack_format {
            return Err(ProjectError::invalid_manifest(format!(
                "target datapack format {:?} does not match profile `{}` ({:?})",
                self.datapack_format, profile.id, profile.datapack_format
            )));
        }
        Ok(())
    }

    pub fn world_version(&self) -> Result<WorldVersion, ProjectError> {
        WorldVersion::new(self.minecraft_version.to_string(), self.data_version)
            .map_err(|error| ProjectError::invalid_manifest(error.to_string()))
    }
}

/// Logical reference to a Java runtime. The path is resolved from the global component
/// cache; a project never stores an absolute path as its source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaComponent {
    pub mode: ComponentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// One resource or data pack in the asset stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsPack {
    /// Logical path below the asset cache root. Never absolute and never a traversal path.
    pub path: String,
    pub sha1: String,
}

/// Logical reference to the client JAR and the derived block/item catalog used by a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetsComponent {
    pub mode: ComponentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_jar_sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pack_stack: Vec<AssetsPack>,
}

/// Logical reference to the official server JAR used for Vanilla World generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerComponent {
    pub mode: ComponentMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option<String>,
}

/// Components resolved for a project at creation. Absent optional entries claim no identity;
/// a placeholder is always explicit and never claims a verified identity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectComponents {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<JavaComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<AssetsComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerComponent>,
}

impl ProjectComponents {
    pub fn validate(&self, target: &ProjectTarget) -> Result<(), ProjectError> {
        if let Some(java) = &self.java {
            java.validate()?;
        }
        if let Some(assets) = &self.assets {
            assets.validate()?;
        }
        if let Some(server) = &self.server {
            server.validate(target)?;
        }
        Ok(())
    }
}

impl JavaComponent {
    fn validate(&self) -> Result<(), ProjectError> {
        match self.mode {
            ComponentMode::Managed | ComponentMode::Manual => {
                let id = self.id.as_deref().ok_or_else(|| {
                    invalid_component("an installed Java component needs a logical id")
                })?;
                validate_component_id("java.id", id)?;
                let sha256 = self.sha256.as_deref().ok_or_else(|| {
                    invalid_component("an installed Java component needs a SHA-256")
                })?;
                validate_hex("java.sha256", sha256, 64)?;
            }
            ComponentMode::Placeholder => {
                if self.id.is_some() || self.sha256.is_some() {
                    return Err(invalid_component(
                        "placeholder Java component must not claim a verified identity",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl AssetsComponent {
    fn validate(&self) -> Result<(), ProjectError> {
        match self.mode {
            ComponentMode::Managed | ComponentMode::Manual => {
                let sha1 = self.client_jar_sha1.as_deref().ok_or_else(|| {
                    invalid_component("an installed asset component needs the client JAR SHA-1")
                })?;
                validate_hex("assets.clientJarSha1", sha1, 40)?;
                match (&self.catalog_id, &self.catalog_hash) {
                    (Some(id), Some(hash)) => {
                        validate_component_id("assets.catalogId", id)?;
                        validate_hex("assets.catalogHash", hash, 64)?;
                    }
                    (None, None) => {
                        return Err(invalid_component(
                            "an installed asset component needs a catalog id and hash",
                        ))
                    }
                    _ => {
                        return Err(invalid_component(
                            "asset catalog id and hash must be stored together",
                        ))
                    }
                }
            }
            ComponentMode::Placeholder => {
                if self.client_jar_sha1.is_some()
                    || self.catalog_id.is_some()
                    || self.catalog_hash.is_some()
                    || !self.pack_stack.is_empty()
                {
                    return Err(invalid_component(
                        "placeholder asset component must not claim a verified identity",
                    ));
                }
            }
        }
        if self.pack_stack.len() > 64 {
            return Err(invalid_component(
                "asset pack stack is limited to 64 entries",
            ));
        }
        for pack in &self.pack_stack {
            validate_logical_relative_path("assets.packStack.path", &pack.path)?;
            validate_hex("assets.packStack.sha1", &pack.sha1, 40)?;
        }
        Ok(())
    }
}

impl ServerComponent {
    fn validate(&self, target: &ProjectTarget) -> Result<(), ProjectError> {
        match self.mode {
            ComponentMode::Managed | ComponentMode::Manual => {
                let version = self.version.as_ref().ok_or_else(|| {
                    invalid_component("an installed server component needs its version")
                })?;
                if version != &target.minecraft_version {
                    return Err(invalid_component(format!(
                        "server version `{version}` does not match the project target `{}`",
                        target.minecraft_version
                    )));
                }
                let sha1 = self.sha1.as_deref().ok_or_else(|| {
                    invalid_component("an installed server component needs a SHA-1")
                })?;
                validate_hex("server.sha1", sha1, 40)?;
            }
            ComponentMode::Placeholder => {
                if self.version.is_some() || self.sha1.is_some() {
                    return Err(invalid_component(
                        "placeholder server component must not claim a verified identity",
                    ));
                }
            }
        }
        Ok(())
    }
}

fn invalid_component(message: impl Into<String>) -> ProjectError {
    ProjectError::invalid_manifest(format!("components: {}", message.into()))
}

fn validate_component_id(field: &str, value: &str) -> Result<(), ProjectError> {
    if value.trim().is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || value.contains('/')
        || value.contains('\\')
        || value.contains(':')
    {
        return Err(invalid_component(format!(
            "{field} must be a logical id, not a path"
        )));
    }
    Ok(())
}

fn validate_logical_relative_path(field: &str, value: &str) -> Result<(), ProjectError> {
    let valid = !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..");
    if !valid {
        return Err(invalid_component(format!(
            "{field} must be a relative logical path"
        )));
    }
    Ok(())
}

fn validate_hex(field: &str, value: &str, length: usize) -> Result<(), ProjectError> {
    if value.len() != length
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid_component(format!(
            "{field} must be {length} lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub project_schema_version: u32,
    pub project_id: String,
    pub name: String,
    pub creation_mode: CreationMode,
    pub target: ProjectTarget,
    pub components: ProjectComponents,
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
        target: ProjectTarget,
        revision: u64,
        now_ms: u64,
    ) -> Self {
        Self {
            project_schema_version: PROJECT_SCHEMA_VERSION,
            project_id,
            name,
            creation_mode,
            target,
            components: ProjectComponents::default(),
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
        self.target.validate()?;
        self.components.validate(&self.target)?;
        let target_version = self.target.world_version()?;
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
            vanilla.validate(&target_version)?;
            if let Some(server) = &self.components.server {
                if server.sha1.as_deref() != Some(vanilla.server_jar_sha1.as_str()) {
                    return Err(invalid_component(
                        "server SHA-1 must match the Vanilla generation provenance",
                    ));
                }
            }
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
            target_minecraft_version: manifest.target.minecraft_version.to_string(),
            target_data_version: manifest.target.data_version,
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
    pub target: ProjectTarget,
    pub components: ProjectComponents,
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
    /// Revision of the world actually opened by the caller.
    pub recovered_revision: u64,
    pub recovered_at_ms: u64,
    pub from_backup: bool,
    /// True only when a newer autosave passed checksum and round-trip verification and was
    /// loaded into memory. A damaged autosave is reported with `false` and never blocks the
    /// last saved revision from opening.
    pub available: bool,
    /// Revision recorded in the autosave metadata, even when it could not be applied.
    pub autosave_revision: Option<u64>,
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
        || folder.trim() != folder
        || folder == "."
        || folder == ".."
        || folder.contains('/')
        || folder.contains('\\')
        || folder.contains(':')
        || folder.contains('\0')
        || folder.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | '"' | '|' | '?' | '*')
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_target() -> ProjectTarget {
        ProjectTarget::from_profile(&minecraft_version::java_classic_1_21_4())
    }

    fn fixture_components() -> ProjectComponents {
        ProjectComponents {
            java: Some(JavaComponent {
                mode: ComponentMode::Managed,
                id: Some("temurin-21.0.5+11".to_owned()),
                sha256: Some("a".repeat(64)),
            }),
            assets: Some(AssetsComponent {
                mode: ComponentMode::Managed,
                client_jar_sha1: Some("b".repeat(40)),
                catalog_id: Some("java-1.21.4-deadbeef".to_owned()),
                catalog_hash: Some("c".repeat(64)),
                pack_stack: vec![AssetsPack {
                    path: "resourcepacks/fury.zip".to_owned(),
                    sha1: "d".repeat(40),
                }],
            }),
            server: Some(ServerComponent {
                mode: ComponentMode::Managed,
                version: Some("1.21.4".parse().expect("version")),
                sha1: Some("e".repeat(40)),
            }),
        }
    }

    fn fixture_manifest(components: ProjectComponents) -> ProjectManifest {
        let mut manifest = ProjectManifest::new(
            "project-1".to_owned(),
            "Schema".to_owned(),
            CreationMode::Empty,
            fixture_target(),
            1,
            1,
        );
        manifest.components = components;
        manifest.set_world_payload(
            "data/world-00000000000000000001.fwl".to_owned(),
            "f".repeat(64),
            42,
        );
        manifest
    }

    #[test]
    fn v2_manifest_persists_target_and_components() {
        let manifest = fixture_manifest(fixture_components());
        manifest.validate().expect("valid schema v2 manifest");

        let value = serde_json::to_value(&manifest).expect("serialize");
        assert_eq!(value["projectSchemaVersion"], 2);
        assert_eq!(value["target"]["minecraftVersion"], "1.21.4");
        assert_eq!(value["target"]["dataVersion"], 4189);
        assert_eq!(value["target"]["profileId"], "java-classic-1.21.4");
        assert_eq!(value["target"]["javaMajor"], 21);
        assert_eq!(value["target"]["resourcePackFormat"], 46);
        assert_eq!(value["target"]["datapackFormat"], 61);
        assert_eq!(value["components"]["java"]["mode"], "managed");
        assert_eq!(
            value["components"]["assets"]["clientJarSha1"],
            "b".repeat(40)
        );
        assert_eq!(
            value["components"]["server"]["version"],
            serde_json::Value::from("1.21.4")
        );
        assert!(value.get("targetMinecraftVersion").is_none());
        assert!(value.get("targetDataVersion").is_none());

        let parsed: ProjectManifest = serde_json::from_value(value).expect("deserialize");
        assert_eq!(parsed, manifest);
    }

    #[test]
    fn target_must_match_the_builtin_profile_exactly() {
        let mut target = fixture_target();
        target.profile_id = "java-classic-9.9.9".to_owned();
        assert!(target
            .validate()
            .unwrap_err()
            .to_string()
            .contains("profile"));

        let mut target = fixture_target();
        target.data_version = 1234;
        assert!(target
            .validate()
            .unwrap_err()
            .to_string()
            .contains("DataVersion"));

        let mut target = fixture_target();
        target.java_major = 17;
        assert!(target
            .validate()
            .unwrap_err()
            .to_string()
            .contains("Java major"));

        let mut target = fixture_target();
        target.datapack_format = Some(62);
        assert!(target
            .validate()
            .unwrap_err()
            .to_string()
            .contains("datapack format"));
    }

    #[test]
    fn unknown_world_versions_are_not_retargeted_to_the_default_profile() {
        let version = WorldVersion::new("1.20.6", 3839).expect("world version");
        assert!(ProjectTarget::for_world_version(&version).is_err());
    }

    #[test]
    fn managed_components_require_verified_identity() {
        let mut java = fixture_components();
        java.java.as_mut().expect("java").sha256 = None;
        assert!(fixture_manifest(java)
            .validate()
            .unwrap_err()
            .to_string()
            .contains("SHA-256"));

        let mut assets = fixture_components();
        assets.assets.as_mut().expect("assets").client_jar_sha1 = None;
        assert!(fixture_manifest(assets)
            .validate()
            .unwrap_err()
            .to_string()
            .contains("client JAR SHA-1"));

        let mut server = fixture_components();
        server.server.as_mut().expect("server").version = Some("1.21.3".parse().expect("version"));
        assert!(fixture_manifest(server)
            .validate()
            .unwrap_err()
            .to_string()
            .contains("does not match the project target"));
    }

    #[test]
    fn asset_pack_paths_are_logical_and_relative() {
        for path in [
            "/absolute.zip",
            "../escape.zip",
            "packs\\win.zip",
            "C:/packs/x.zip",
        ] {
            let mut components = fixture_components();
            components.assets.as_mut().expect("assets").pack_stack[0].path = path.to_owned();
            assert!(
                fixture_manifest(components).validate().is_err(),
                "path `{path}` must be refused"
            );
        }
    }

    #[test]
    fn placeholders_carry_no_verified_identity() {
        let placeholder = ProjectComponents {
            java: Some(JavaComponent {
                mode: ComponentMode::Placeholder,
                id: None,
                sha256: None,
            }),
            assets: Some(AssetsComponent {
                mode: ComponentMode::Placeholder,
                client_jar_sha1: None,
                catalog_id: None,
                catalog_hash: None,
                pack_stack: Vec::new(),
            }),
            server: Some(ServerComponent {
                mode: ComponentMode::Placeholder,
                version: None,
                sha1: None,
            }),
        };
        fixture_manifest(placeholder)
            .validate()
            .expect("an explicit placeholder is valid");

        let mut claimed = fixture_components();
        claimed.server.as_mut().expect("server").mode = ComponentMode::Placeholder;
        assert!(fixture_manifest(claimed).validate().is_err());
    }
}
