//! Editor-only semantic objects layered over the authoritative voxel world.
//!
//! Districts, buildings, roads and landmarks are project metadata: they never change
//! Minecraft blocks by themselves. The scene is independent of Tauri and the renderer so
//! the UI and MCP can inspect exactly the same catalog.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::BlockBox;

pub const SCENE_SCHEMA_VERSION: u32 = 1;
pub const MAX_OBJECTS: usize = 1024;
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
}
