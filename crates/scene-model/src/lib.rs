//! Editor-only semantic objects layered over the authoritative voxel world.
//!
//! Districts, buildings, roads and landmarks are project metadata: they never change
//! Minecraft blocks by themselves. The scene is independent of Tauri and the renderer so
//! the UI and MCP can inspect exactly the same catalog.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BlockBox, BlockPos, BlockState};

pub const SCENE_SCHEMA_VERSION: u32 = 1;
pub const PARAMETRIC_SCHEMA_VERSION: u32 = 1;
pub const MAX_OBJECTS: usize = 1024;
pub const MAX_PARAMETRIC_OBJECTS: usize = 64;
pub const MAX_PARAMETRIC_BLOCKS: usize = 32_768;
pub const MAX_TOTAL_PARAMETRIC_BLOCKS: usize = 262_144;
pub const MAX_PARAMETRIC_SIZE: u32 = 32;
pub const HOUSE_GENERATOR_VERSION: u32 = 1;
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_ID_CHARS: usize = 64;
pub const MAX_DIMENSION_CHARS: usize = 128;
pub const MAX_DEPTH: usize = 8;
pub const MAX_TAGS: usize = 16;
pub const MAX_TAG_KEY_CHARS: usize = 32;
pub const MAX_TAG_VALUE_CHARS: usize = 128;
pub const MAX_NOTE_CHARS: usize = 1024;

const FALLBACK_ID: &str = "object";

fn default_schema_version() -> u32 {
    SCENE_SCHEMA_VERSION
}

/// The semantic category of an object. Values are open to tags (`style`, `owner`, ...);
/// this enum only fixes the categories the editor and agents can rely on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    District,
    Building,
    Road,
    Entrance,
    Landmark,
    Other,
}

impl ObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::District => "district",
            Self::Building => "building",
            Self::Road => "road",
            Self::Entrance => "entrance",
            Self::Landmark => "landmark",
            Self::Other => "other",
        }
    }
}

/// One named, dimension-scoped semantic object. Bounds are inclusive world coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticObject {
    pub id: String,
    pub name: String,
    pub kind: ObjectKind,
    pub dimension: String,
    pub bounds: BlockBox,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tags: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SceneError {
    #[error("scene schema {found} is not supported; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("scene has {found} objects, limit is {limit}")]
    TooManyObjects { found: usize, limit: usize },
    #[error("invalid object id `{0}`")]
    InvalidId(String),
    #[error("duplicate object id `{0}`")]
    DuplicateId(String),
    #[error("object name must be 1..={0} characters")]
    InvalidName(usize),
    #[error("object dimension must be 1..={0} characters")]
    InvalidDimension(usize),
    #[error("object `{0}` was not found")]
    NotFound(String),
    #[error("parent object `{0}` was not found")]
    UnknownParent(String),
    #[error("object `{object}` must use its parent's dimension `{parent_dimension}`")]
    ParentDimension {
        object: String,
        parent_dimension: String,
    },
    #[error("object hierarchy contains a cycle at `{0}`")]
    ParentCycle(String),
    #[error("object `{object}` is nested {depth} levels deep, limit is {limit}")]
    DepthExceeded {
        object: String,
        depth: usize,
        limit: usize,
    },
    #[error("object `{0}` still has children; remove or reparent them first")]
    HasChildren(String),
    #[error("object `{object}` has {found} tags, limit is {limit}")]
    TooManyTags {
        object: String,
        found: usize,
        limit: usize,
    },
    #[error("object `{object}` has an empty or oversized tag key/value")]
    InvalidTag { object: String },
    #[error("object `{object}` note is longer than {limit} characters")]
    NoteTooLong { object: String, limit: usize },
}

impl SceneError {
    /// Stable machine-readable code shared with the application error mapping.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema { .. } => "unsupported_version",
            Self::TooManyObjects { .. }
            | Self::DepthExceeded { .. }
            | Self::TooManyTags { .. }
            | Self::NoteTooLong { .. } => "limit_exceeded",
            Self::NotFound(_) | Self::UnknownParent(_) => "not_found",
            Self::HasChildren(_) => "conflict",
            Self::InvalidId(_)
            | Self::DuplicateId(_)
            | Self::InvalidName(_)
            | Self::InvalidDimension(_)
            | Self::ParentDimension { .. }
            | Self::ParentCycle(_)
            | Self::InvalidTag { .. } => "invalid_argument",
        }
    }
}

/// The persisted semantic layer of a project. It is intentionally separate from the
/// voxel world and from the workspace edit policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub objects: Vec<SemanticObject>,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            schema_version: SCENE_SCHEMA_VERSION,
            objects: Vec::new(),
        }
    }
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn objects(&self) -> &[SemanticObject] {
        &self.objects
    }

    pub fn get(&self, id: &str) -> Option<&SemanticObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn children(&self, id: &str) -> Vec<&SemanticObject> {
        self.objects
            .iter()
            .filter(|object| object.parent_id.as_deref() == Some(id))
            .collect()
    }

    /// Adds an object and returns it. The id is derived from the name and uniquified;
    /// names never become file paths. The parent must already exist in the same dimension.
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        name: &str,
        kind: ObjectKind,
        dimension: &str,
        bounds: BlockBox,
        parent_id: Option<String>,
        tags: BTreeMap<String, String>,
        note: Option<String>,
    ) -> Result<&SemanticObject, SceneError> {
        if self.objects.len() >= MAX_OBJECTS {
            return Err(SceneError::TooManyObjects {
                found: self.objects.len() + 1,
                limit: MAX_OBJECTS,
            });
        }
        let name = normalise_name(name)?;
        let dimension = normalise_dimension(dimension)?;
        let note = normalise_note(note)?;
        validate_tags(&name, &tags)?;
        if let Some(parent_id) = parent_id.as_deref() {
            let parent = self
                .get(parent_id)
                .ok_or_else(|| SceneError::UnknownParent(parent_id.to_owned()))?;
            if parent.dimension != dimension {
                return Err(SceneError::ParentDimension {
                    object: name.clone(),
                    parent_dimension: parent.dimension.clone(),
                });
            }
            let depth = self.depth(parent_id)? + 1;
            if depth > MAX_DEPTH {
                return Err(SceneError::DepthExceeded {
                    object: name.clone(),
                    depth,
                    limit: MAX_DEPTH,
                });
            }
        }
        let id = self.unique_id(&slugify(&name));
        let object = SemanticObject {
            id,
            name,
            kind,
            dimension,
            bounds,
            parent_id,
            tags,
            note,
        };
        self.objects.push(object);
        self.objects
            .last()
            .ok_or(SceneError::NotFound("object".to_owned()))
    }

    /// Removes a leaf object. Parents with children are refused so a hierarchy cannot be
    /// silently detached by deleting an ancestor.
    pub fn remove(&mut self, id: &str) -> Result<SemanticObject, SceneError> {
        if !self.children(id).is_empty() {
            return Err(SceneError::HasChildren(id.to_owned()));
        }
        let index = self
            .objects
            .iter()
            .position(|object| object.id == id)
            .ok_or_else(|| SceneError::NotFound(id.to_owned()))?;
        Ok(self.objects.remove(index))
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<&SemanticObject, SceneError> {
        let name = normalise_name(name)?;
        let object = self
            .objects
            .iter_mut()
            .find(|object| object.id == id)
            .ok_or_else(|| SceneError::NotFound(id.to_owned()))?;
        object.name = name;
        Ok(object)
    }

    pub fn depth(&self, id: &str) -> Result<usize, SceneError> {
        let object = self
            .get(id)
            .ok_or_else(|| SceneError::NotFound(id.to_owned()))?;
        let mut seen = BTreeSet::new();
        seen.insert(object.id.as_str());
        let mut depth = 1usize;
        let mut cursor = object;
        while let Some(parent_id) = cursor.parent_id.as_deref() {
            if !seen.insert(parent_id) {
                return Err(SceneError::ParentCycle(object.id.clone()));
            }
            let parent = self
                .get(parent_id)
                .ok_or_else(|| SceneError::UnknownParent(parent_id.to_owned()))?;
            if parent.dimension != object.dimension {
                return Err(SceneError::ParentDimension {
                    object: object.id.clone(),
                    parent_dimension: parent.dimension.clone(),
                });
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(SceneError::DepthExceeded {
                    object: object.id.clone(),
                    depth,
                    limit: MAX_DEPTH,
                });
            }
            cursor = parent;
        }
        Ok(depth)
    }

    /// Full fail-closed validation used when opening or saving a project.
    pub fn validate(&self) -> Result<(), SceneError> {
        if self.schema_version != SCENE_SCHEMA_VERSION {
            return Err(SceneError::UnsupportedSchema {
                found: self.schema_version,
                expected: SCENE_SCHEMA_VERSION,
            });
        }
        if self.objects.len() > MAX_OBJECTS {
            return Err(SceneError::TooManyObjects {
                found: self.objects.len(),
                limit: MAX_OBJECTS,
            });
        }
        let mut ids = BTreeSet::new();
        for object in &self.objects {
            if !valid_id(&object.id) {
                return Err(SceneError::InvalidId(object.id.clone()));
            }
            if !ids.insert(object.id.as_str()) {
                return Err(SceneError::DuplicateId(object.id.clone()));
            }
            if object.name.trim() != object.name || object.name.chars().count() > MAX_NAME_CHARS {
                return Err(SceneError::InvalidName(MAX_NAME_CHARS));
            }
            if object.name.is_empty() {
                return Err(SceneError::InvalidName(MAX_NAME_CHARS));
            }
            if object.dimension.trim() != object.dimension
                || object.dimension.is_empty()
                || object.dimension.chars().count() > MAX_DIMENSION_CHARS
            {
                return Err(SceneError::InvalidDimension(MAX_DIMENSION_CHARS));
            }
            if let Some(note) = &object.note {
                if note.chars().count() > MAX_NOTE_CHARS {
                    return Err(SceneError::NoteTooLong {
                        object: object.id.clone(),
                        limit: MAX_NOTE_CHARS,
                    });
                }
            }
            validate_tags(&object.id, &object.tags)?;
        }
        // Every parent chain is validated separately, which also detects cycles, unknown
        // parents, cross-dimension links and depth overflow.
        for object in &self.objects {
            self.depth(&object.id)?;
        }
        Ok(())
    }

    fn unique_id(&self, base: &str) -> String {
        if self.get(base).is_none() {
            return base.to_owned();
        }
        for suffix in 2..=MAX_OBJECTS + 1 {
            let suffix = format!("-{suffix}");
            let keep = MAX_ID_CHARS.saturating_sub(suffix.len());
            let prefix = base
                .get(..keep.min(base.len()))
                .unwrap_or(base)
                .trim_end_matches('-');
            let candidate = format!("{prefix}{suffix}");
            if self.get(&candidate).is_none() {
                return candidate;
            }
        }
        // Unreachable while MAX_OBJECTS bounds the catalog, but fail closed with a legal id.
        format!("{FALLBACK_ID}-{}", self.objects.len() + 1)
    }
}

// ---------------------------------------------------------------------------
// Parametric scene graph (M9.15)
// ---------------------------------------------------------------------------

fn default_parametric_schema() -> u32 {
    PARAMETRIC_SCHEMA_VERSION
}

/// Deterministic generators understood by the parametric scene graph. The version is
/// stored next to the generator so a persisted recipe always has an explicit meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorKind {
    House,
}

impl GeneratorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::House => "house",
        }
    }
}

/// Quarter-turn rotation around the vertical axis. Positions and common directional
/// block-state properties are rotated together.
/// What a transform or rebake does when the current blocks no longer match the previous
/// generator output. `Override` is an explicit user choice; the default is fail-closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParametricConflictPolicy {
    #[default]
    Fail,
    Override,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    pub fn degrees(self) -> u16 {
        match self {
            Self::None => 0,
            Self::Cw90 => 90,
            Self::Cw180 => 180,
            Self::Cw270 => 270,
        }
    }

    fn swap_axes(self) -> bool {
        matches!(self, Self::Cw90 | Self::Cw270)
    }
}

/// Parameters of the version-1 house generator. Material strings are canonical Minecraft
/// block states; the generator validates them before producing any blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HouseParams {
    pub width: u32,
    pub depth: u32,
    /// Wall height in blocks. Total height is `height + 2` including floor and roof.
    pub height: u32,
    pub wall: String,
    pub floor: String,
    pub roof: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
}

impl HouseParams {
    pub fn validate(&self) -> Result<(), ParametricError> {
        if !(3..=MAX_PARAMETRIC_SIZE).contains(&self.width)
            || !(3..=MAX_PARAMETRIC_SIZE).contains(&self.depth)
            || !(2..=MAX_PARAMETRIC_SIZE).contains(&self.height)
        {
            return Err(ParametricError::InvalidParams(format!(
                "house size must be width/depth 3..={MAX_PARAMETRIC_SIZE} and height 2..={MAX_PARAMETRIC_SIZE}"
            )));
        }
        for (label, value) in [
            ("wall", &self.wall),
            ("floor", &self.floor),
            ("roof", &self.roof),
        ] {
            let state = parse_material(label, value)?;
            if state.is_air() {
                return Err(ParametricError::InvalidMaterial(format!(
                    "house {label} material must not be air"
                )));
            }
        }
        if let Some(window) = &self.window {
            parse_material("window", window)?;
        }
        Ok(())
    }

    fn block_count(&self) -> u64 {
        let footprint = u64::from(self.width) * u64::from(self.depth);
        let wall_height = u64::from(self.height);
        let perimeter = 2 * (u64::from(self.width) + u64::from(self.depth)) - 4;
        footprint
            .saturating_mul(2)
            .saturating_add(perimeter.saturating_mul(wall_height))
    }
}

fn parse_material(label: &str, value: &str) -> Result<BlockState, ParametricError> {
    value
        .parse::<BlockState>()
        .map_err(|error| ParametricError::InvalidMaterial(format!("{label} `{value}`: {error}")))
}

/// One position written by a generator. The generator only describes final states; it
/// never reads or writes the world itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedBlock {
    pub pos: BlockPos,
    pub state: BlockState,
}

/// The block states that existed before the first application of an object. It is stored
/// with palette compression so rollback and rebake can restore unrelated pre-existing
/// blocks instead of assuming air.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParametricBaseline {
    #[serde(default)]
    pub positions: Vec<BlockPos>,
    #[serde(default)]
    pub palette: Vec<BlockState>,
    #[serde(default)]
    pub indices: Vec<u32>,
}

impl ParametricBaseline {
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn encode(blocks: &BTreeMap<BlockPos, BlockState>) -> Result<Self, ParametricError> {
        if blocks.len() > MAX_PARAMETRIC_BLOCKS {
            return Err(ParametricError::TooManyBlocks {
                requested: blocks.len() as u64,
                limit: MAX_PARAMETRIC_BLOCKS,
            });
        }
        let mut palette = Vec::new();
        let mut palette_indices: BTreeMap<BlockState, u32> = BTreeMap::new();
        let mut positions = Vec::with_capacity(blocks.len());
        let mut indices = Vec::with_capacity(blocks.len());
        for (pos, state) in blocks {
            let index = match palette_indices.get(state) {
                Some(index) => *index,
                None => {
                    let index = u32::try_from(palette.len()).map_err(|_| {
                        ParametricError::BaselineMismatch(
                            "baseline palette exceeds the supported index range".to_owned(),
                        )
                    })?;
                    palette.push(state.clone());
                    palette_indices.insert(state.clone(), index);
                    index
                }
            };
            positions.push(*pos);
            indices.push(index);
        }
        Ok(Self {
            positions,
            palette,
            indices,
        })
    }

    pub fn decode(&self) -> Result<BTreeMap<BlockPos, BlockState>, ParametricError> {
        self.validate_inner(None)?;
        let mut blocks = BTreeMap::new();
        for (position, index) in self.positions.iter().zip(&self.indices) {
            let state = self
                .palette
                .get(*index as usize)
                .ok_or_else(|| {
                    ParametricError::BaselineMismatch(format!(
                        "baseline palette index {index} is out of range"
                    ))
                })?
                .clone();
            blocks.insert(*position, state);
        }
        Ok(blocks)
    }

    fn validate_inner(&self, bounds: Option<BlockBox>) -> Result<(), ParametricError> {
        if self.positions.len() != self.indices.len() {
            return Err(ParametricError::BaselineMismatch(
                "baseline positions and indices have different lengths".to_owned(),
            ));
        }
        if self.positions.len() > MAX_PARAMETRIC_BLOCKS {
            return Err(ParametricError::TooManyBlocks {
                requested: self.positions.len() as u64,
                limit: MAX_PARAMETRIC_BLOCKS,
            });
        }
        if self.positions.is_empty() {
            return Err(ParametricError::BaselineMismatch(
                "baseline must contain at least one generated position".to_owned(),
            ));
        }
        if self.palette.is_empty() {
            return Err(ParametricError::BaselineMismatch(
                "baseline palette must not be empty".to_owned(),
            ));
        }
        let mut seen = BTreeSet::new();
        for (position, index) in self.positions.iter().zip(&self.indices) {
            if !seen.insert(*position) {
                return Err(ParametricError::BaselineMismatch(format!(
                    "baseline position {position:?} is duplicated"
                )));
            }
            if let Some(bounds) = bounds {
                if !bounds.contains(*position) {
                    return Err(ParametricError::BaselineMismatch(format!(
                        "baseline position {position:?} lies outside the object bounds"
                    )));
                }
            }
            if *index as usize >= self.palette.len() {
                return Err(ParametricError::BaselineMismatch(format!(
                    "baseline palette index {index} is out of range"
                )));
            }
        }
        Ok(())
    }
}

/// A persisted parametric object. The generator recipe and baseline are authoritative;
/// the block overlay can always be regenerated from the recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParametricObject {
    pub id: String,
    pub name: String,
    pub generator: GeneratorKind,
    pub generator_version: u32,
    pub dimension: String,
    pub origin: BlockPos,
    pub rotation: Rotation,
    pub params: HouseParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    #[serde(default)]
    pub applied_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "ParametricBaseline::is_empty")]
    pub baseline: ParametricBaseline,
}

/// UI/MCP-facing metadata without the block baseline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParametricObjectSummary {
    pub id: String,
    pub name: String,
    pub generator: GeneratorKind,
    pub generator_version: u32,
    pub dimension: String,
    pub bounds: BlockBox,
    pub origin: BlockPos,
    pub rotation: Rotation,
    pub params: HouseParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
    pub applied_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_operation_id: Option<String>,
    pub generated_blocks: u64,
}

/// Input for creating or replacing a parametric object. The id is assigned by the
/// catalog from the name and never supplied by a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParametricSpec {
    pub name: String,
    pub generator: GeneratorKind,
    pub generator_version: u32,
    pub dimension: String,
    pub origin: BlockPos,
    pub rotation: Rotation,
    pub params: HouseParams,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<String>,
}

impl ParametricSpec {
    pub fn validate(&self) -> Result<(), ParametricError> {
        validate_generator(self.generator, self.generator_version)?;
        normalise_parametric_name(&self.name)?;
        normalise_parametric_dimension(&self.dimension)?;
        self.params.validate()
    }
}

impl ParametricObject {
    pub fn bounds(&self) -> Result<BlockBox, ParametricError> {
        let (width, depth) = self.footprint();
        let max_x = self
            .origin
            .x
            .checked_add(i32::try_from(width - 1).map_err(|_| {
                ParametricError::InvalidParams(
                    "house width exceeds the coordinate range".to_owned(),
                )
            })?)
            .ok_or_else(|| ParametricError::InvalidParams("house bounds overflow X".to_owned()))?;
        let max_z = self
            .origin
            .z
            .checked_add(i32::try_from(depth - 1).map_err(|_| {
                ParametricError::InvalidParams(
                    "house depth exceeds the coordinate range".to_owned(),
                )
            })?)
            .ok_or_else(|| ParametricError::InvalidParams("house bounds overflow Z".to_owned()))?;
        let max_y = self
            .origin
            .y
            .checked_add(i32::try_from(self.params.height + 1).map_err(|_| {
                ParametricError::InvalidParams(
                    "house height exceeds the coordinate range".to_owned(),
                )
            })?)
            .ok_or_else(|| ParametricError::InvalidParams("house bounds overflow Y".to_owned()))?;
        BlockBox::new(self.origin, BlockPos::new(max_x, max_y, max_z))
            .map_err(|error| ParametricError::InvalidParams(error.to_string()))
    }

    fn footprint(&self) -> (u32, u32) {
        if self.rotation.swap_axes() {
            (self.params.depth, self.params.width)
        } else {
            (self.params.width, self.params.depth)
        }
    }

    /// Deterministic block overlay for the current recipe. Positions are sorted by Y, X
    /// then Z so repeated generation produces byte-identical operations.
    pub fn generate(&self) -> Result<Vec<GeneratedBlock>, ParametricError> {
        validate_generator(self.generator, self.generator_version)?;
        self.params.validate()?;
        let wall = parse_material("wall", &self.params.wall)?;
        let floor = parse_material("floor", &self.params.floor)?;
        let roof = parse_material("roof", &self.params.roof)?;
        let window = self
            .params
            .window
            .as_deref()
            .map(|value| parse_material("window", value))
            .transpose()?;
        let air = "minecraft:air".parse::<BlockState>().expect("air is valid");
        let width = self.params.width;
        let depth = self.params.depth;
        let height = self.params.height;
        let door_x = width / 2;
        let window_level = 1 + height / 2;
        let mut local: BTreeMap<(u32, u32, u32), BlockState> = BTreeMap::new();

        for x in 0..width {
            for z in 0..depth {
                local.insert((x, 0, z), floor.clone());
                local.insert((x, height + 1, z), roof.clone());
            }
        }
        for level in 1..=height {
            for x in 0..width {
                for z in 0..depth {
                    if x != 0 && x != width - 1 && z != 0 && z != depth - 1 {
                        continue;
                    }
                    let is_door = z == 0 && x == door_x && level <= 2;
                    let is_corner = (x == 0 || x == width - 1) && (z == 0 || z == depth - 1);
                    let is_window = level == window_level
                        && !is_corner
                        && !is_door
                        && (x + z).is_multiple_of(2);
                    let state = if is_door {
                        air.clone()
                    } else if is_window {
                        window.clone().unwrap_or_else(|| wall.clone())
                    } else {
                        wall.clone()
                    };
                    local.insert((x, level, z), state);
                }
            }
        }

        let mut blocks = BTreeMap::new();
        for ((x, y, z), state) in local {
            let (rx, rz) = rotate_local(x, z, width, depth, self.rotation);
            let pos = BlockPos::new(
                checked_offset(self.origin.x, rx, "X")?,
                checked_offset(self.origin.y, y, "Y")?,
                checked_offset(self.origin.z, rz, "Z")?,
            );
            blocks.insert(pos, rotate_block_state(&state, self.rotation));
        }
        Ok(blocks
            .into_iter()
            .map(|(pos, state)| GeneratedBlock { pos, state })
            .collect())
    }

    /// Metadata, recipe and baseline validation. Opening a project runs this fail-closed.
    pub fn validate(&self) -> Result<(), ParametricError> {
        self.validate_shape()?;
        let bounds = self.bounds()?;
        self.baseline.validate_inner(Some(bounds))?;
        let generated = self.generate()?;
        if generated.len() > MAX_PARAMETRIC_BLOCKS {
            return Err(ParametricError::TooManyBlocks {
                requested: generated.len() as u64,
                limit: MAX_PARAMETRIC_BLOCKS,
            });
        }
        let generated_positions: BTreeSet<BlockPos> =
            generated.iter().map(|block| block.pos).collect();
        let baseline_positions: BTreeSet<BlockPos> =
            self.baseline.positions.iter().copied().collect();
        if generated_positions != baseline_positions {
            return Err(ParametricError::BaselineMismatch(format!(
                "object `{}` baseline does not match its generator output",
                self.id
            )));
        }
        Ok(())
    }

    pub fn validate_shape(&self) -> Result<(), ParametricError> {
        if !valid_id(&self.id) {
            return Err(ParametricError::InvalidId(self.id.clone()));
        }
        if self.name.trim() != self.name
            || self.name.is_empty()
            || self.name.chars().count() > MAX_NAME_CHARS
        {
            return Err(ParametricError::InvalidName(MAX_NAME_CHARS));
        }
        if self.dimension.trim() != self.dimension
            || self.dimension.is_empty()
            || self.dimension.chars().count() > MAX_DIMENSION_CHARS
        {
            return Err(ParametricError::InvalidDimension(MAX_DIMENSION_CHARS));
        }
        if let Some(layer) = &self.layer_id {
            if !valid_id(layer) {
                return Err(ParametricError::InvalidId(layer.clone()));
            }
        }
        validate_generator(self.generator, self.generator_version)?;
        self.params.validate()?;
        let generated = self.params.block_count();
        if generated > MAX_PARAMETRIC_BLOCKS as u64 {
            return Err(ParametricError::TooManyBlocks {
                requested: generated,
                limit: MAX_PARAMETRIC_BLOCKS,
            });
        }
        self.bounds()?;
        Ok(())
    }

    pub fn summary(&self) -> Result<ParametricObjectSummary, ParametricError> {
        Ok(ParametricObjectSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            generator: self.generator,
            generator_version: self.generator_version,
            dimension: self.dimension.clone(),
            bounds: self.bounds()?,
            origin: self.origin,
            rotation: self.rotation,
            params: self.params.clone(),
            layer_id: self.layer_id.clone(),
            applied_revision: self.applied_revision,
            last_operation_id: self.last_operation_id.clone(),
            generated_blocks: self.baseline.len() as u64,
        })
    }
}

fn validate_generator(generator: GeneratorKind, version: u32) -> Result<(), ParametricError> {
    if generator != GeneratorKind::House || version != HOUSE_GENERATOR_VERSION {
        return Err(ParametricError::UnsupportedGenerator {
            generator: generator.as_str().to_owned(),
            version,
        });
    }
    Ok(())
}

fn checked_offset(base: i32, offset: u32, axis: &str) -> Result<i32, ParametricError> {
    base.checked_add(
        i32::try_from(offset).map_err(|_| {
            ParametricError::InvalidParams(format!("house offset overflows {axis}"))
        })?,
    )
    .ok_or_else(|| ParametricError::InvalidParams(format!("house bounds overflow {axis}")))
}

fn rotate_local(x: u32, z: u32, width: u32, depth: u32, rotation: Rotation) -> (u32, u32) {
    match rotation {
        Rotation::None => (x, z),
        Rotation::Cw90 => (depth - 1 - z, x),
        Rotation::Cw180 => (width - 1 - x, depth - 1 - z),
        Rotation::Cw270 => (z, width - 1 - x),
    }
}

/// Rotates common directional properties together with positions. Unknown properties are
/// preserved; unsupported orientation schemes are not guessed.
fn rotate_block_state(state: &BlockState, rotation: Rotation) -> BlockState {
    if rotation == Rotation::None {
        return state.clone();
    }
    let mut properties = state.properties().clone();
    if let Some(facing) = properties.get("facing").cloned() {
        let turned = match rotation {
            Rotation::None => facing,
            Rotation::Cw90 => rotate_facing(&facing, 1),
            Rotation::Cw180 => rotate_facing(&facing, 2),
            Rotation::Cw270 => rotate_facing(&facing, 3),
        };
        properties.insert("facing".to_owned(), turned);
    }
    if rotation.swap_axes() {
        if let Some(axis) = properties.get("axis").cloned() {
            if axis == "x" {
                properties.insert("axis".to_owned(), "z".to_owned());
            } else if axis == "z" {
                properties.insert("axis".to_owned(), "x".to_owned());
            }
        }
    }
    if let Some(value) = properties.get("rotation").cloned() {
        if let Ok(value) = value.parse::<u8>() {
            let steps = rotation.degrees() / 90;
            let turned = (u16::from(value) + steps * 4) % 16;
            properties.insert("rotation".to_owned(), turned.to_string());
        }
    }
    BlockState::new(state.name(), properties).unwrap_or_else(|_| state.clone())
}

fn rotate_facing(facing: &str, quarter_turns: u16) -> String {
    let order = ["north", "east", "south", "west"];
    let Some(index) = order.iter().position(|value| *value == facing) else {
        return facing.to_owned();
    };
    order[(index + quarter_turns as usize) % order.len()].to_owned()
}

/// A catalog of parametric objects. It is separate from the semantic `Scene` and the
/// workspace edit policy, and is persisted in the project manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParametricScene {
    #[serde(default = "default_parametric_schema")]
    pub schema_version: u32,
    #[serde(default)]
    pub objects: Vec<ParametricObject>,
}

impl Default for ParametricScene {
    fn default() -> Self {
        Self {
            schema_version: PARAMETRIC_SCHEMA_VERSION,
            objects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParametricError {
    #[error("parametric schema {found} is not supported; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("parametric scene has {found} objects, limit is {limit}")]
    TooManyObjects { found: usize, limit: usize },
    #[error("invalid parametric object id `{0}`")]
    InvalidId(String),
    #[error("duplicate parametric object id `{0}`")]
    DuplicateId(String),
    #[error("object name must be 1..={0} characters")]
    InvalidName(usize),
    #[error("object dimension must be 1..={0} characters")]
    InvalidDimension(usize),
    #[error("generator `{generator}` version {version} is not supported")]
    UnsupportedGenerator { generator: String, version: u32 },
    #[error("invalid parametric parameters: {0}")]
    InvalidParams(String),
    #[error("invalid parametric material: {0}")]
    InvalidMaterial(String),
    #[error("object generates {requested} blocks, limit is {limit}")]
    TooManyBlocks { requested: u64, limit: usize },
    #[error("parametric baseline uses {total} blocks in total, limit is {limit}")]
    TooManyBaselineBlocks { total: usize, limit: usize },
    #[error("parametric baseline is inconsistent: {0}")]
    BaselineMismatch(String),
    #[error("parametric object `{0}` was not found")]
    NotFound(String),
}

impl ParametricError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema { .. } | Self::UnsupportedGenerator { .. } => {
                "unsupported_version"
            }
            Self::TooManyObjects { .. }
            | Self::TooManyBlocks { .. }
            | Self::TooManyBaselineBlocks { .. } => "limit_exceeded",
            Self::NotFound(_) => "not_found",
            Self::DuplicateId(_) | Self::BaselineMismatch(_) => "conflict",
            Self::InvalidId(_)
            | Self::InvalidName(_)
            | Self::InvalidDimension(_)
            | Self::InvalidParams(_)
            | Self::InvalidMaterial(_) => "invalid_argument",
        }
    }
}

impl ParametricScene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn objects(&self) -> &[ParametricObject] {
        &self.objects
    }

    pub fn get(&self, id: &str) -> Option<&ParametricObject> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn summaries(&self) -> Result<Vec<ParametricObjectSummary>, ParametricError> {
        self.objects.iter().map(ParametricObject::summary).collect()
    }

    pub fn next_id(&self, name: &str) -> Result<String, ParametricError> {
        let name = normalise_parametric_name(name)?;
        if self.objects.len() >= MAX_PARAMETRIC_OBJECTS {
            return Err(ParametricError::TooManyObjects {
                found: self.objects.len() + 1,
                limit: MAX_PARAMETRIC_OBJECTS,
            });
        }
        let base = slugify(&name);
        let base = if base == FALLBACK_ID {
            "house".to_owned()
        } else {
            base
        };
        if self.get(&base).is_none() {
            return Ok(base);
        }
        for suffix in 2..=MAX_PARAMETRIC_OBJECTS + 1 {
            let suffix = format!("-{suffix}");
            let keep = MAX_ID_CHARS.saturating_sub(suffix.len());
            let prefix = base
                .get(..keep.min(base.len()))
                .unwrap_or(&base)
                .trim_end_matches('-');
            let candidate = format!("{prefix}{suffix}");
            if self.get(&candidate).is_none() {
                return Ok(candidate);
            }
        }
        Err(ParametricError::DuplicateId(base))
    }

    /// Inserts a fully validated object. Ids are assigned by `next_id`, never supplied by
    /// an untrusted client.
    pub fn insert(&mut self, object: ParametricObject) -> Result<(), ParametricError> {
        if self.objects.len() >= MAX_PARAMETRIC_OBJECTS {
            return Err(ParametricError::TooManyObjects {
                found: self.objects.len() + 1,
                limit: MAX_PARAMETRIC_OBJECTS,
            });
        }
        if self.get(&object.id).is_some() {
            return Err(ParametricError::DuplicateId(object.id));
        }
        object.validate()?;
        let total = self.baseline_total() + object.baseline.len();
        if total > MAX_TOTAL_PARAMETRIC_BLOCKS {
            return Err(ParametricError::TooManyBaselineBlocks {
                total,
                limit: MAX_TOTAL_PARAMETRIC_BLOCKS,
            });
        }
        self.objects.push(object);
        Ok(())
    }

    pub fn replace(&mut self, object: ParametricObject) -> Result<(), ParametricError> {
        let index = self
            .objects
            .iter()
            .position(|existing| existing.id == object.id)
            .ok_or_else(|| ParametricError::NotFound(object.id.clone()))?;
        object.validate()?;
        let total =
            self.baseline_total() - self.objects[index].baseline.len() + object.baseline.len();
        if total > MAX_TOTAL_PARAMETRIC_BLOCKS {
            return Err(ParametricError::TooManyBaselineBlocks {
                total,
                limit: MAX_TOTAL_PARAMETRIC_BLOCKS,
            });
        }
        self.objects[index] = object;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<ParametricObject, ParametricError> {
        let index = self
            .objects
            .iter()
            .position(|object| object.id == id)
            .ok_or_else(|| ParametricError::NotFound(id.to_owned()))?;
        Ok(self.objects.remove(index))
    }

    /// Full fail-closed validation used when opening or saving a project.
    pub fn validate(&self) -> Result<(), ParametricError> {
        if self.schema_version != PARAMETRIC_SCHEMA_VERSION {
            return Err(ParametricError::UnsupportedSchema {
                found: self.schema_version,
                expected: PARAMETRIC_SCHEMA_VERSION,
            });
        }
        if self.objects.len() > MAX_PARAMETRIC_OBJECTS {
            return Err(ParametricError::TooManyObjects {
                found: self.objects.len(),
                limit: MAX_PARAMETRIC_OBJECTS,
            });
        }
        let mut ids = BTreeSet::new();
        for object in &self.objects {
            if !ids.insert(object.id.as_str()) {
                return Err(ParametricError::DuplicateId(object.id.clone()));
            }
            object.validate()?;
        }
        let total = self.baseline_total();
        if total > MAX_TOTAL_PARAMETRIC_BLOCKS {
            return Err(ParametricError::TooManyBaselineBlocks {
                total,
                limit: MAX_TOTAL_PARAMETRIC_BLOCKS,
            });
        }
        Ok(())
    }

    fn baseline_total(&self) -> usize {
        self.objects
            .iter()
            .map(|object| object.baseline.len())
            .sum()
    }
}

fn normalise_parametric_name(name: &str) -> Result<String, ParametricError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(ParametricError::InvalidName(MAX_NAME_CHARS));
    }
    Ok(trimmed.to_owned())
}

fn normalise_parametric_dimension(dimension: &str) -> Result<String, ParametricError> {
    let trimmed = dimension.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_DIMENSION_CHARS {
        return Err(ParametricError::InvalidDimension(MAX_DIMENSION_CHARS));
    }
    Ok(trimmed.to_owned())
}

fn normalise_name(name: &str) -> Result<String, SceneError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(SceneError::InvalidName(MAX_NAME_CHARS));
    }
    Ok(trimmed.to_owned())
}

fn normalise_dimension(dimension: &str) -> Result<String, SceneError> {
    let trimmed = dimension.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_DIMENSION_CHARS {
        return Err(SceneError::InvalidDimension(MAX_DIMENSION_CHARS));
    }
    Ok(trimmed.to_owned())
}

fn normalise_note(note: Option<String>) -> Result<Option<String>, SceneError> {
    let Some(note) = note else {
        return Ok(None);
    };
    let trimmed = note.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > MAX_NOTE_CHARS {
        return Err(SceneError::NoteTooLong {
            object: "new object".to_owned(),
            limit: MAX_NOTE_CHARS,
        });
    }
    Ok(Some(trimmed.to_owned()))
}

fn validate_tags(object: &str, tags: &BTreeMap<String, String>) -> Result<(), SceneError> {
    if tags.len() > MAX_TAGS {
        return Err(SceneError::TooManyTags {
            object: object.to_owned(),
            found: tags.len(),
            limit: MAX_TAGS,
        });
    }
    for (key, value) in tags {
        if key.trim() != key
            || key.is_empty()
            || key.chars().count() > MAX_TAG_KEY_CHARS
            || value.chars().count() > MAX_TAG_VALUE_CHARS
        {
            return Err(SceneError::InvalidTag {
                object: object.to_owned(),
            });
        }
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let mut slug = slug.trim_matches('-').to_owned();
    if slug.len() > MAX_ID_CHARS {
        slug.truncate(MAX_ID_CHARS);
        slug = slug.trim_end_matches('-').to_owned();
    }
    if slug.is_empty() {
        return FALLBACK_ID.to_owned();
    }
    if !slug
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
    {
        slug.insert(0, 'o');
        if slug.len() > MAX_ID_CHARS {
            slug.truncate(MAX_ID_CHARS);
        }
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;
    use world_domain::BlockPos;

    fn bounds(min: (i32, i32, i32), max: (i32, i32, i32)) -> BlockBox {
        BlockBox::new(
            BlockPos::new(min.0, min.1, min.2),
            BlockPos::new(max.0, max.1, max.2),
        )
        .expect("bounds")
    }

    fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn default_scene_is_empty_and_serializes_with_camel_case() {
        let scene = Scene::default();
        assert!(scene.is_empty());
        assert_eq!(scene.schema_version, SCENE_SCHEMA_VERSION);
        let value = serde_json::to_value(&scene).expect("json");
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["objects"], serde_json::json!([]));
    }

    #[test]
    fn push_generates_unique_slug_ids_and_links_children() {
        let mut scene = Scene::new();
        let district = scene
            .push(
                " Old Town ",
                ObjectKind::District,
                "minecraft:overworld",
                bounds((0, 0, 0), (64, 64, 64)),
                None,
                tags(&[("style", "medieval")]),
                Some("  Historic center  ".to_owned()),
            )
            .expect("district")
            .clone();
        assert_eq!(district.id, "old-town");
        assert_eq!(district.name, "Old Town");
        assert_eq!(district.note.as_deref(), Some("Historic center"));
        let building = scene
            .push(
                "Old Town",
                ObjectKind::Building,
                "minecraft:overworld",
                bounds((4, 64, 4), (12, 72, 12)),
                Some(district.id.clone()),
                BTreeMap::new(),
                None,
            )
            .expect("building")
            .clone();
        assert_eq!(building.id, "old-town-2");
        assert_eq!(building.parent_id.as_deref(), Some("old-town"));
        assert_eq!(scene.children("old-town"), vec![&building]);
        assert_eq!(scene.depth("old-town-2"), Ok(2));
    }

    #[test]
    fn push_rejects_missing_or_cross_dimension_parents() {
        let mut scene = Scene::new();
        assert_eq!(
            scene.push(
                "Alone",
                ObjectKind::Building,
                "minecraft:overworld",
                bounds((0, 0, 0), (1, 1, 1)),
                Some("missing".to_owned()),
                BTreeMap::new(),
                None,
            ),
            Err(SceneError::UnknownParent("missing".to_owned()))
        );
        let district = scene
            .push(
                "District",
                ObjectKind::District,
                "minecraft:overworld",
                bounds((0, 0, 0), (8, 8, 8)),
                None,
                BTreeMap::new(),
                None,
            )
            .expect("district")
            .id
            .clone();
        assert!(matches!(
            scene.push(
                "Nether annex",
                ObjectKind::Building,
                "minecraft:the_nether",
                bounds((0, 0, 0), (1, 1, 1)),
                Some(district),
                BTreeMap::new(),
                None,
            ),
            Err(SceneError::ParentDimension { .. })
        ));
    }

    #[test]
    fn validate_detects_cycles_duplicates_and_depth_overflow() {
        let mut scene = Scene::new();
        let root = scene
            .push(
                "Root",
                ObjectKind::District,
                "minecraft:overworld",
                bounds((0, 0, 0), (8, 8, 8)),
                None,
                BTreeMap::new(),
                None,
            )
            .expect("root")
            .id
            .clone();
        let mut parent = root;
        for index in 2..=MAX_DEPTH {
            parent = scene
                .push(
                    &format!("Level {index}"),
                    ObjectKind::Other,
                    "minecraft:overworld",
                    bounds((0, 0, 0), (1, 1, 1)),
                    Some(parent),
                    BTreeMap::new(),
                    None,
                )
                .expect("level")
                .id
                .clone();
        }
        assert!(matches!(
            scene.push(
                "Too deep",
                ObjectKind::Other,
                "minecraft:overworld",
                bounds((0, 0, 0), (1, 1, 1)),
                Some(parent),
                BTreeMap::new(),
                None,
            ),
            Err(SceneError::DepthExceeded { .. })
        ));
        scene.validate().expect("valid scene");

        let mut duplicate = scene.clone();
        duplicate.objects.push(duplicate.objects[0].clone());
        assert!(matches!(
            duplicate.validate(),
            Err(SceneError::DuplicateId(_))
        ));

        let mut cycle = scene.clone();
        cycle.objects[0].parent_id = Some(cycle.objects[MAX_DEPTH - 1].id.clone());
        assert!(matches!(cycle.validate(), Err(SceneError::ParentCycle(_))));
    }

    #[test]
    fn remove_refuses_ancestors_and_removes_leaves() {
        let mut scene = Scene::new();
        let district = scene
            .push(
                "District",
                ObjectKind::District,
                "minecraft:overworld",
                bounds((0, 0, 0), (8, 8, 8)),
                None,
                BTreeMap::new(),
                None,
            )
            .expect("district")
            .id
            .clone();
        let building = scene
            .push(
                "House",
                ObjectKind::Building,
                "minecraft:overworld",
                bounds((0, 0, 0), (2, 2, 2)),
                Some(district.clone()),
                BTreeMap::new(),
                None,
            )
            .expect("building")
            .id
            .clone();
        assert_eq!(
            scene.remove(&district),
            Err(SceneError::HasChildren(district.clone()))
        );
        scene.remove(&building).expect("remove leaf");
        scene.remove(&district).expect("remove district");
        assert!(scene.is_empty());
    }

    #[test]
    fn rename_validates_names_and_keeps_the_id() {
        let mut scene = Scene::new();
        let id = scene
            .push(
                "House",
                ObjectKind::Building,
                "minecraft:overworld",
                bounds((0, 0, 0), (2, 2, 2)),
                None,
                BTreeMap::new(),
                None,
            )
            .expect("house")
            .id
            .clone();
        scene.rename(&id, "Town Hall").expect("rename");
        assert_eq!(scene.get(&id).expect("object").name, "Town Hall");
        assert_eq!(scene.get(&id).expect("object").id, id);
        assert_eq!(
            scene.rename(&id, "   "),
            Err(SceneError::InvalidName(MAX_NAME_CHARS))
        );
        assert_eq!(
            scene.rename("missing", "Anything"),
            Err(SceneError::NotFound("missing".to_owned()))
        );
    }

    #[test]
    fn limits_and_contract_errors_are_stable() {
        let mut scene = Scene::new();
        assert_eq!(
            scene.push(
                "Object",
                ObjectKind::Other,
                "minecraft:overworld",
                bounds((0, 0, 0), (1, 1, 1)),
                None,
                (0..=MAX_TAGS)
                    .map(|index| (format!("tag{index}"), "value".to_owned()))
                    .collect(),
                None,
            ),
            Err(SceneError::TooManyTags {
                object: "Object".to_owned(),
                found: MAX_TAGS + 1,
                limit: MAX_TAGS,
            })
        );
        assert_eq!(
            scene.push(
                "Note",
                ObjectKind::Other,
                "minecraft:overworld",
                bounds((0, 0, 0), (1, 1, 1)),
                None,
                BTreeMap::new(),
                Some("x".repeat(MAX_NOTE_CHARS + 1)),
            ),
            Err(SceneError::NoteTooLong {
                object: "new object".to_owned(),
                limit: MAX_NOTE_CHARS,
            })
        );
        assert_eq!(
            scene.push(
                "Bad",
                ObjectKind::Other,
                "",
                bounds((0, 0, 0), (1, 1, 1)),
                None,
                BTreeMap::new(),
                None,
            ),
            Err(SceneError::InvalidDimension(MAX_DIMENSION_CHARS))
        );

        let mut value = serde_json::to_value(SemanticObject {
            id: "house".to_owned(),
            name: "House".to_owned(),
            kind: ObjectKind::Building,
            dimension: "minecraft:overworld".to_owned(),
            bounds: bounds((0, 0, 0), (1, 1, 1)),
            parent_id: Some("district".to_owned()),
            tags: tags(&[("style", "nordic")]),
            note: None,
        })
        .expect("json");
        assert_eq!(value["kind"], "building");
        assert_eq!(value["parentId"], "district");
        value["kind"] = serde_json::json!("unknown");
        assert!(serde_json::from_value::<SemanticObject>(value).is_err());
    }

    #[test]
    fn object_limit_is_enforced() {
        let mut scene = Scene::new();
        for index in 0..MAX_OBJECTS {
            scene
                .push(
                    &format!("Object {index}"),
                    ObjectKind::Other,
                    "minecraft:overworld",
                    bounds((0, 0, 0), (1, 1, 1)),
                    None,
                    BTreeMap::new(),
                    None,
                )
                .expect("object");
        }
        assert!(matches!(
            scene.push(
                "Overflow",
                ObjectKind::Other,
                "minecraft:overworld",
                bounds((0, 0, 0), (1, 1, 1)),
                None,
                BTreeMap::new(),
                None,
            ),
            Err(SceneError::TooManyObjects { .. })
        ));
        scene.validate().expect("valid");
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(
            SceneError::UnsupportedSchema {
                found: 2,
                expected: 1
            }
            .code(),
            "unsupported_version"
        );
        assert_eq!(SceneError::NotFound("x".to_owned()).code(), "not_found");
        assert_eq!(SceneError::HasChildren("x".to_owned()).code(), "conflict");
        assert_eq!(
            SceneError::InvalidName(MAX_NAME_CHARS).code(),
            "invalid_argument"
        );
    }

    fn house_params() -> HouseParams {
        HouseParams {
            width: 5,
            depth: 4,
            height: 3,
            wall: "minecraft:stone_bricks".to_owned(),
            floor: "minecraft:oak_planks".to_owned(),
            roof: "minecraft:dark_oak_planks".to_owned(),
            window: Some("minecraft:glass".to_owned()),
        }
    }

    fn parametric_spec(name: &str) -> ParametricSpec {
        ParametricSpec {
            name: name.to_owned(),
            generator: GeneratorKind::House,
            generator_version: HOUSE_GENERATOR_VERSION,
            dimension: "minecraft:overworld".to_owned(),
            origin: BlockPos::new(10, 64, -20),
            rotation: Rotation::None,
            params: house_params(),
            layer_id: None,
        }
    }

    fn parametric_object(scene: &ParametricScene, name: &str) -> ParametricObject {
        let spec = parametric_spec(name);
        let mut object = ParametricObject {
            id: scene.next_id(name).expect("id"),
            name: spec.name,
            generator: spec.generator,
            generator_version: spec.generator_version,
            dimension: spec.dimension,
            origin: spec.origin,
            rotation: spec.rotation,
            params: spec.params,
            layer_id: spec.layer_id,
            applied_revision: 3,
            last_operation_id: Some("op-1".to_owned()),
            baseline: ParametricBaseline::default(),
        };
        let generated = object.generate().expect("generate");
        let map: BTreeMap<BlockPos, BlockState> = generated
            .into_iter()
            .map(|block| (block.pos, block.state))
            .collect();
        object.baseline = ParametricBaseline::encode(&map).expect("baseline");
        object
    }

    #[test]
    fn house_generator_is_deterministic_and_inside_bounds() {
        let scene = ParametricScene::new();
        let object = parametric_object(&scene, "Old House");
        assert_eq!(object.id, "old-house");
        let first = object.generate().expect("first");
        let second = object.generate().expect("second");
        assert_eq!(first, second);
        assert_eq!(
            first.len() as u64,
            object.baseline.len() as u64,
            "baseline positions mirror the generated overlay"
        );
        let bounds = object.bounds().expect("bounds");
        assert_eq!(bounds.min, BlockPos::new(10, 64, -20));
        assert_eq!(bounds.max, BlockPos::new(14, 68, -17));
        for block in &first {
            assert!(bounds.contains(block.pos));
        }
        let state_at = |pos: BlockPos| {
            first
                .iter()
                .find(|block| block.pos == pos)
                .map(|block| block.state.to_string())
        };
        assert_eq!(
            state_at(BlockPos::new(10, 64, -20)).as_deref(),
            Some("minecraft:oak_planks")
        );
        assert_eq!(
            state_at(BlockPos::new(10, 65, -20)).as_deref(),
            Some("minecraft:stone_bricks")
        );
        assert_eq!(
            state_at(BlockPos::new(10, 68, -20)).as_deref(),
            Some("minecraft:dark_oak_planks")
        );
        assert_eq!(
            state_at(BlockPos::new(12, 65, -20)).as_deref(),
            Some("minecraft:air"),
            "the door is a two-block opening in the front wall"
        );
        assert_eq!(
            state_at(BlockPos::new(10, 66, -18)).as_deref(),
            Some("minecraft:glass"),
            "the mid-height window is rotated with the house"
        );
        object.validate().expect("valid object");
    }

    #[test]
    fn rotation_swaps_footprints_and_common_block_properties() {
        let scene = ParametricScene::new();
        let mut object = parametric_object(&scene, "Rotated");
        object.params.wall = "minecraft:oak_stairs[facing=north]".to_owned();
        object.params.floor = "minecraft:oak_log[axis=x]".to_owned();
        object.params.window = None;
        object.rotation = Rotation::Cw90;
        let generated = object.generate().expect("rotated");
        let bounds = object.bounds().expect("bounds");
        assert_eq!(bounds.max, BlockPos::new(13, 68, -16));
        let at = |pos: BlockPos| {
            generated
                .iter()
                .find(|block| block.pos == pos)
                .map(|block| block.state.to_string())
        };
        // Local (0, 1, 0) becomes (depth - 1, 0) = (3, 0) and north becomes east.
        assert_eq!(
            at(BlockPos::new(13, 65, -20)).as_deref(),
            Some("minecraft:oak_stairs[facing=east]")
        );
        // X-axis logs become Z-axis logs for quarter turns.
        assert_eq!(
            at(BlockPos::new(13, 64, -20)).as_deref(),
            Some("minecraft:oak_log[axis=z]")
        );
        let mut baseline: BTreeMap<BlockPos, BlockState> = generated
            .into_iter()
            .map(|block| (block.pos, block.state))
            .collect();
        object.baseline = ParametricBaseline::encode(&baseline).expect("baseline");
        object.validate().expect("valid rotated object");

        object.rotation = Rotation::Cw270;
        let generated = object.generate().expect("rotated back");
        baseline = generated
            .into_iter()
            .map(|block| (block.pos, block.state))
            .collect();
        object.baseline = ParametricBaseline::encode(&baseline).expect("baseline");
        assert_eq!(
            object
                .generate()
                .expect("generate")
                .iter()
                .find(|block| block.pos == BlockPos::new(10, 65, -16))
                .map(|block| block.state.to_string())
                .as_deref(),
            Some("minecraft:oak_stairs[facing=west]")
        );
        object.validate().expect("valid rotated object");
    }

    #[test]
    fn baseline_compresses_states_and_rejects_corruption() {
        let scene = ParametricScene::new();
        let object = parametric_object(&scene, "Baseline");
        let decoded = object.baseline.decode().expect("decode");
        assert_eq!(decoded.len(), object.baseline.len());
        let generated: BTreeMap<BlockPos, BlockState> = object
            .generate()
            .expect("generate")
            .into_iter()
            .map(|block| (block.pos, block.state))
            .collect();
        assert_eq!(decoded, generated);
        assert!(object.baseline.palette.len() < object.baseline.positions.len());

        let mut broken = object.clone();
        broken.baseline.indices[0] = u32::MAX;
        assert!(matches!(
            broken.validate(),
            Err(ParametricError::BaselineMismatch(_))
        ));
        let mut missing = object.clone();
        missing.baseline.positions.pop();
        missing.baseline.indices.pop();
        assert!(matches!(
            missing.validate(),
            Err(ParametricError::BaselineMismatch(_))
        ));
        let mut outside = object.clone();
        outside.baseline.positions[0] = BlockPos::new(-100, 0, -100);
        assert!(matches!(
            outside.validate(),
            Err(ParametricError::BaselineMismatch(_))
        ));
    }

    #[test]
    fn catalog_ids_limits_and_schema_are_fail_closed() {
        let mut scene = ParametricScene::new();
        assert_eq!(scene.next_id("Old House").expect("id"), "old-house");
        let first = parametric_object(&scene, "Old House");
        scene.insert(first).expect("insert");
        assert_eq!(scene.next_id("Old House").expect("id"), "old-house-2");
        let second = parametric_object(&scene, "Old House");
        scene.insert(second).expect("insert second");
        let duplicate = scene.objects()[0].clone();
        assert_eq!(
            scene.insert(duplicate),
            Err(ParametricError::DuplicateId("old-house".to_owned()))
        );
        scene.validate().expect("valid");

        let mut invalid_schema = scene.clone();
        invalid_schema.schema_version = 99;
        assert!(matches!(
            invalid_schema.validate(),
            Err(ParametricError::UnsupportedSchema { .. })
        ));

        let mut bad_size = parametric_spec("Bad");
        bad_size.params.width = 2;
        assert!(matches!(
            bad_size.validate(),
            Err(ParametricError::InvalidParams(_))
        ));
        let mut bad_generator = parametric_spec("Future");
        bad_generator.generator_version = 2;
        assert!(matches!(
            bad_generator.validate(),
            Err(ParametricError::UnsupportedGenerator { .. })
        ));
        let mut missing_object = scene.clone();
        assert_eq!(
            missing_object.remove("missing"),
            Err(ParametricError::NotFound("missing".to_owned()))
        );
    }

    #[test]
    fn summaries_omit_the_baseline_and_report_bounds() {
        let scene = ParametricScene::new();
        let object = parametric_object(&scene, "Summary");
        let summary = object.summary().expect("summary");
        assert_eq!(summary.id, "summary");
        assert_eq!(summary.bounds.max, BlockPos::new(14, 68, -17));
        assert_eq!(summary.generated_blocks, object.baseline.len() as u64);
        let json = serde_json::to_value(&summary).expect("json");
        assert!(json.get("baseline").is_none());
        assert_eq!(json["generator"], "house");
        assert_eq!(json["generatorVersion"], 1);
        assert_eq!(json["rotation"], "none");
    }
}
