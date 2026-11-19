//! Project-level editing policy: named layers, an optional allowed-edit mask and
//! protected regions.
//!
//! These are editor concepts, not Minecraft world data. The model is deliberately
//! small and deterministic so the same policy can be persisted in `project.json`,
//! enforced by the application service and shown by the UI. Region edits are
//! fail-closed: an operation is refused when its bounding box is not fully inside
//! the active mask or when it intersects a protected region.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BlockBox, BlockPos};

pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;
pub const MAX_LAYERS: usize = 64;
pub const MAX_MASKS: usize = 64;
pub const MAX_PROTECTED_REGIONS: usize = 64;
pub const MAX_NAME_CHARS: usize = 64;
pub const MAX_ID_CHARS: usize = 64;
pub const MAX_DIMENSION_CHARS: usize = 128;
/// A single mask or protected region cannot cover more than 16 million blocks.
/// The editor selection itself is capped at one million, so this only guards
/// hand-written or imported project metadata.
pub const MAX_REGION_VOLUME: u64 = 16 * 1024 * 1024;
pub const BASE_LAYER_ID: &str = "base";

fn default_schema_version() -> u32 {
    WORKSPACE_SCHEMA_VERSION
}

fn default_true() -> bool {
    true
}

/// A named edit layer. Layers group operations and can be locked to stop further
/// writes. FuryBuilder does not track per-block ownership, so `visible` applies to
/// workspace overlays rather than hiding already written blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
}

impl Layer {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            visible: true,
            locked: false,
        }
    }
}

/// A dimension-scoped box used either as an allowed-edit mask or as a protected
/// region. The same shape keeps the UI and validation uniform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRegion {
    pub id: String,
    pub name: String,
    pub dimension: String,
    pub bounds: BlockBox,
}

impl WorkspaceRegion {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        dimension: impl Into<String>,
        bounds: BlockBox,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            dimension: dimension.into(),
            bounds,
        }
    }
}

/// Persisted workspace configuration. Missing fields are filled from the default so
/// projects written before M9.13 keep loading and simply start with one base layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePolicy {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_layers")]
    pub layers: Vec<Layer>,
    #[serde(default = "default_active_layer")]
    pub active_layer_id: Option<String>,
    #[serde(default)]
    pub masks: Vec<WorkspaceRegion>,
    #[serde(default)]
    pub active_mask_id: Option<String>,
    #[serde(default)]
    pub protected_regions: Vec<WorkspaceRegion>,
}

fn default_layers() -> Vec<Layer> {
    vec![Layer::new(BASE_LAYER_ID, "Base")]
}

fn default_active_layer() -> Option<String> {
    Some(BASE_LAYER_ID.to_owned())
}

impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            schema_version: WORKSPACE_SCHEMA_VERSION,
            layers: default_layers(),
            active_layer_id: default_active_layer(),
            masks: Vec::new(),
            active_mask_id: None,
            protected_regions: Vec::new(),
        }
    }
}

/// Stable machine-readable policy failures. The application facade maps these to
/// API error codes; the original variant stays available for tests and auditing.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkspaceError {
    #[error("workspace schema version {found} is not supported; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("too many {kind}: {found}, limit is {limit}")]
    TooMany {
        kind: &'static str,
        found: usize,
        limit: usize,
    },
    #[error("{kind} id `{id}` is invalid")]
    InvalidId { kind: &'static str, id: String },
    #[error("{kind} id `{id}` is duplicated")]
    DuplicateId { kind: &'static str, id: String },
    #[error("{kind} name must be 1..={max} characters")]
    InvalidName { kind: &'static str, max: usize },
    #[error("{kind} dimension must be 1..={max} characters")]
    InvalidDimension { kind: &'static str, max: usize },
    #[error("{kind} `{id}` has inverted bounds")]
    InvertedBounds { kind: &'static str, id: String },
    #[error("{kind} `{id}` covers {volume} blocks, limit is {limit}")]
    RegionTooLarge {
        kind: &'static str,
        id: String,
        volume: u64,
        limit: u64,
    },
    #[error("workspace needs at least one layer")]
    NoLayers,
    #[error("active layer `{0}` does not exist")]
    UnknownActiveLayer(String),
    #[error("active mask `{0}` does not exist")]
    UnknownActiveMask(String),
    #[error("layer `{0}` does not exist")]
    UnknownLayer(String),
    #[error("layer `{name}` is locked; unlock it before editing")]
    LockedLayer { name: String },
    #[error("operation is outside the active mask `{name}`")]
    MaskViolation { name: String },
    #[error("operation intersects the protected region `{name}`")]
    ProtectedRegion { name: String },
}

impl WorkspaceError {
    /// Stable code used by the application API. Policy problems are argument errors;
    /// lock/mask/protection failures are permission errors.
    pub fn code(&self) -> &'static str {
        match self {
            WorkspaceError::LockedLayer { .. }
            | WorkspaceError::MaskViolation { .. }
            | WorkspaceError::ProtectedRegion { .. } => "permission_denied",
            WorkspaceError::TooMany { .. } | WorkspaceError::RegionTooLarge { .. } => {
                "limit_exceeded"
            }
            WorkspaceError::UnsupportedSchema { .. } => "unsupported_version",
            _ => "invalid_argument",
        }
    }
}

fn is_valid_id(value: &str) -> bool {
    if value.is_empty() || value.len() > MAX_ID_CHARS {
        return false;
    }
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

fn valid_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed.chars().count() <= MAX_NAME_CHARS
}

fn box_contains(outer: &BlockBox, inner: &BlockBox) -> bool {
    inner.min.x >= outer.min.x
        && inner.min.y >= outer.min.y
        && inner.min.z >= outer.min.z
        && inner.max.x <= outer.max.x
        && inner.max.y <= outer.max.y
        && inner.max.z <= outer.max.z
}

fn bounds_are_ordered(bounds: &BlockBox) -> bool {
    bounds.min.x <= bounds.max.x && bounds.min.y <= bounds.max.y && bounds.min.z <= bounds.max.z
}

impl WorkspacePolicy {
    pub fn layer(&self, id: &str) -> Option<&Layer> {
        self.layers.iter().find(|layer| layer.id == id)
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.active_layer_id
            .as_deref()
            .and_then(|id| self.layer(id))
    }

    pub fn active_mask(&self) -> Option<&WorkspaceRegion> {
        self.active_mask_id
            .as_deref()
            .and_then(|id| self.masks.iter().find(|mask| mask.id == id))
    }

    /// Resolves the layer an operation should be attributed to. An omitted layer
    /// means "the active layer", including for MCP clients, so a locked layer also
    /// protects against AI edits that do not name a layer explicitly.
    pub fn effective_layer(
        &self,
        requested: Option<&str>,
    ) -> Result<Option<&Layer>, WorkspaceError> {
        match requested {
            Some(id) => self
                .layer(id)
                .map(Some)
                .ok_or_else(|| WorkspaceError::UnknownLayer(id.to_owned())),
            None => Ok(self.active_layer()),
        }
    }

    /// Checks a proposed operation against the active layer, active mask and all
    /// protected regions. Undo/redo intentionally bypass this check: restoring a
    /// previous snapshot is not a new construction decision.
    pub fn check_operation(
        &self,
        requested_layer: Option<&str>,
        dimension: &str,
        bounds: &BlockBox,
    ) -> Result<(), WorkspaceError> {
        if let Some(layer) = self.effective_layer(requested_layer)? {
            if layer.locked {
                return Err(WorkspaceError::LockedLayer {
                    name: layer.name.clone(),
                });
            }
        }
        if let Some(mask) = self.active_mask() {
            if mask.dimension == dimension && !box_contains(&mask.bounds, bounds) {
                return Err(WorkspaceError::MaskViolation {
                    name: mask.name.clone(),
                });
            }
        }
        if let Some(region) = self
            .protected_regions
            .iter()
            .find(|region| region.dimension == dimension && region.bounds.intersects(bounds))
        {
            return Err(WorkspaceError::ProtectedRegion {
                name: region.name.clone(),
            });
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), WorkspaceError> {
        if self.schema_version != WORKSPACE_SCHEMA_VERSION {
            return Err(WorkspaceError::UnsupportedSchema {
                found: self.schema_version,
                expected: WORKSPACE_SCHEMA_VERSION,
            });
        }
        if self.layers.is_empty() {
            return Err(WorkspaceError::NoLayers);
        }
        check_count("layer", self.layers.len(), MAX_LAYERS)?;
        check_count("mask", self.masks.len(), MAX_MASKS)?;
        check_count(
            "protected region",
            self.protected_regions.len(),
            MAX_PROTECTED_REGIONS,
        )?;
        let mut layer_ids = std::collections::BTreeSet::new();
        for layer in &self.layers {
            validate_id("layer", &layer.id)?;
            if !layer_ids.insert(layer.id.as_str()) {
                return Err(WorkspaceError::DuplicateId {
                    kind: "layer",
                    id: layer.id.clone(),
                });
            }
            if !valid_name(&layer.name) {
                return Err(WorkspaceError::InvalidName {
                    kind: "layer",
                    max: MAX_NAME_CHARS,
                });
            }
        }
        for (kind, regions) in [
            ("mask", &self.masks),
            ("protected region", &self.protected_regions),
        ] {
            let mut ids = std::collections::BTreeSet::new();
            for region in regions {
                validate_id(kind, &region.id)?;
                if !ids.insert(region.id.as_str()) {
                    return Err(WorkspaceError::DuplicateId {
                        kind,
                        id: region.id.clone(),
                    });
                }
                if !valid_name(&region.name) {
                    return Err(WorkspaceError::InvalidName {
                        kind,
                        max: MAX_NAME_CHARS,
                    });
                }
                if region.dimension.trim().is_empty()
                    || region.dimension.chars().count() > MAX_DIMENSION_CHARS
                {
                    return Err(WorkspaceError::InvalidDimension {
                        kind,
                        max: MAX_DIMENSION_CHARS,
                    });
                }
                if !bounds_are_ordered(&region.bounds) {
                    return Err(WorkspaceError::InvertedBounds {
                        kind,
                        id: region.id.clone(),
                    });
                }
                let volume = region.bounds.volume();
                if volume > MAX_REGION_VOLUME {
                    return Err(WorkspaceError::RegionTooLarge {
                        kind,
                        id: region.id.clone(),
                        volume,
                        limit: MAX_REGION_VOLUME,
                    });
                }
            }
        }
        match &self.active_layer_id {
            Some(id) if self.layer(id).is_some() => {}
            Some(id) => return Err(WorkspaceError::UnknownActiveLayer(id.clone())),
            None => return Err(WorkspaceError::NoLayers),
        }
        if let Some(id) = &self.active_mask_id {
            if !self.masks.iter().any(|mask| &mask.id == id) {
                return Err(WorkspaceError::UnknownActiveMask(id.clone()));
            }
        }
        Ok(())
    }

    /// Appends a layer with a slug derived from `name`, uniquified against existing
    /// ids. Names are not required to be unique, ids are.
    pub fn push_layer(&mut self, name: &str) -> Result<&Layer, WorkspaceError> {
        if !valid_name(name) {
            return Err(WorkspaceError::InvalidName {
                kind: "layer",
                max: MAX_NAME_CHARS,
            });
        }
        if self.layers.len() >= MAX_LAYERS {
            return Err(WorkspaceError::TooMany {
                kind: "layer",
                found: self.layers.len() + 1,
                limit: MAX_LAYERS,
            });
        }
        let id = unique_id(name, self.layers.iter().map(|layer| layer.id.as_str()));
        self.layers.push(Layer::new(id, name.trim()));
        Ok(self.layers.last().expect("layer just pushed"))
    }

    /// Appends a mask with a slug derived from `name`.
    pub fn push_mask(
        &mut self,
        name: &str,
        dimension: &str,
        bounds: BlockBox,
    ) -> Result<&WorkspaceRegion, WorkspaceError> {
        if self.masks.len() >= MAX_MASKS {
            return Err(WorkspaceError::TooMany {
                kind: "mask",
                found: self.masks.len() + 1,
                limit: MAX_MASKS,
            });
        }
        let region = new_region(
            "mask",
            name,
            dimension,
            bounds,
            self.masks.iter().map(|mask| mask.id.as_str()),
        )?;
        self.masks.push(region);
        Ok(self.masks.last().expect("mask just pushed"))
    }

    /// Appends a protected region with a slug derived from `name`.
    pub fn push_protected_region(
        &mut self,
        name: &str,
        dimension: &str,
        bounds: BlockBox,
    ) -> Result<&WorkspaceRegion, WorkspaceError> {
        if self.protected_regions.len() >= MAX_PROTECTED_REGIONS {
            return Err(WorkspaceError::TooMany {
                kind: "protected region",
                found: self.protected_regions.len() + 1,
                limit: MAX_PROTECTED_REGIONS,
            });
        }
        let region = new_region(
            "protected region",
            name,
            dimension,
            bounds,
            self.protected_regions
                .iter()
                .map(|region| region.id.as_str()),
        )?;
        self.protected_regions.push(region);
        Ok(self.protected_regions.last().expect("region just pushed"))
    }

    /// Removes a layer. The last layer and the active layer cannot be removed.
    pub fn remove_layer(&mut self, id: &str) -> Result<Layer, WorkspaceError> {
        if self.layers.len() == 1 {
            return Err(WorkspaceError::NoLayers);
        }
        if self.active_layer_id.as_deref() == Some(id) {
            return Err(WorkspaceError::UnknownActiveLayer(id.to_owned()));
        }
        let index = self
            .layers
            .iter()
            .position(|layer| layer.id == id)
            .ok_or_else(|| WorkspaceError::UnknownLayer(id.to_owned()))?;
        Ok(self.layers.remove(index))
    }

    /// Removes a mask and clears the active reference when it pointed at this mask.
    pub fn remove_mask(&mut self, id: &str) -> Option<WorkspaceRegion> {
        let index = self.masks.iter().position(|mask| mask.id == id)?;
        let removed = self.masks.remove(index);
        if self.active_mask_id.as_deref() == Some(id) {
            self.active_mask_id = None;
        }
        Some(removed)
    }

    /// Removes a protected region.
    pub fn remove_protected_region(&mut self, id: &str) -> Option<WorkspaceRegion> {
        let index = self
            .protected_regions
            .iter()
            .position(|region| region.id == id)?;
        Some(self.protected_regions.remove(index))
    }
}

fn check_count(kind: &'static str, found: usize, limit: usize) -> Result<(), WorkspaceError> {
    if found > limit {
        return Err(WorkspaceError::TooMany { kind, found, limit });
    }
    Ok(())
}

fn validate_id(kind: &'static str, id: &str) -> Result<(), WorkspaceError> {
    if !is_valid_id(id) {
        return Err(WorkspaceError::InvalidId {
            kind,
            id: id.to_owned(),
        });
    }
    Ok(())
}

fn new_region<'a>(
    kind: &'static str,
    name: &str,
    dimension: &str,
    bounds: BlockBox,
    existing: impl Iterator<Item = &'a str>,
) -> Result<WorkspaceRegion, WorkspaceError> {
    if !valid_name(name) {
        return Err(WorkspaceError::InvalidName {
            kind,
            max: MAX_NAME_CHARS,
        });
    }
    if dimension.trim().is_empty() || dimension.chars().count() > MAX_DIMENSION_CHARS {
        return Err(WorkspaceError::InvalidDimension {
            kind,
            max: MAX_DIMENSION_CHARS,
        });
    }
    if !bounds_are_ordered(&bounds) {
        return Err(WorkspaceError::InvertedBounds {
            kind,
            id: name.to_owned(),
        });
    }
    let volume = bounds.volume();
    if volume > MAX_REGION_VOLUME {
        return Err(WorkspaceError::RegionTooLarge {
            kind,
            id: name.to_owned(),
            volume,
            limit: MAX_REGION_VOLUME,
        });
    }
    let id = unique_id(name, existing);
    Ok(WorkspaceRegion::new(id, name.trim(), dimension, bounds))
}

/// Turns a display name into a stable lowercase id, appending `-2`, `-3`, ... when
/// the slug is already used.
fn unique_id<'a>(name: &str, existing: impl Iterator<Item = &'a str>) -> String {
    let mut base: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while base.contains("--") {
        base = base.replace("--", "-");
    }
    let base = base.trim_matches('-');
    let mut base = if base.is_empty() {
        "region".to_owned()
    } else {
        base.to_owned()
    };
    if base.len() > MAX_ID_CHARS {
        base.truncate(MAX_ID_CHARS);
        base = base.trim_end_matches('-').to_owned();
    }
    if !base
        .chars()
        .next()
        .map(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .unwrap_or(false)
    {
        base.insert(0, 'r');
    }
    let used: std::collections::BTreeSet<&str> = existing.collect();
    if !used.contains(base.as_str()) {
        return base;
    }
    for suffix in 2..=u32::MAX {
        let candidate = format!("{base}-{suffix}");
        if !used.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("u32 suffix space cannot be exhausted by a bounded workspace")
}

/// Small helper for tests and callers that build boxes from unsigned sizes.
pub fn box_from_min_size(min: BlockPos, size: [u32; 3]) -> Result<BlockBox, WorkspaceError> {
    BlockBox::from_min_size(min, size).map_err(|_| WorkspaceError::InvertedBounds {
        kind: "region",
        id: "unknown".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(min: (i32, i32, i32), max: (i32, i32, i32)) -> BlockBox {
        BlockBox::new(
            BlockPos::new(min.0, min.1, min.2),
            BlockPos::new(max.0, max.1, max.2),
        )
        .expect("bounds")
    }

    #[test]
    fn default_policy_has_one_base_layer_and_validates() {
        let policy = WorkspacePolicy::default();
        policy.validate().expect("default policy");
        assert_eq!(policy.layers.len(), 1);
        assert_eq!(policy.active_layer_id.as_deref(), Some(BASE_LAYER_ID));
        assert!(policy.active_layer().expect("base").visible);
        assert!(!policy.active_layer().expect("base").locked);
        assert!(policy.masks.is_empty());
        assert!(policy.protected_regions.is_empty());
    }

    #[test]
    fn missing_fields_deserialize_to_the_default_policy() {
        let policy: WorkspacePolicy = serde_json::from_str("{}").expect("defaults");
        assert_eq!(policy, WorkspacePolicy::default());
        let partial: WorkspacePolicy =
            serde_json::from_str(r#"{"layers":[{"id":"base","name":"Base"}]}"#).expect("layers");
        assert_eq!(partial.active_layer_id.as_deref(), Some(BASE_LAYER_ID));
        assert!(partial.layers[0].visible);
    }

    #[test]
    fn layer_ids_and_active_references_are_validated() {
        let mut policy = WorkspacePolicy::default();
        policy.layers.push(Layer::new("Bad Id", "Bad"));
        assert!(matches!(
            policy.validate(),
            Err(WorkspaceError::InvalidId { .. })
        ));

        let policy = WorkspacePolicy {
            active_layer_id: Some("missing".to_owned()),
            ..WorkspacePolicy::default()
        };
        assert_eq!(
            policy.validate(),
            Err(WorkspaceError::UnknownActiveLayer("missing".to_owned()))
        );

        let mut policy = WorkspacePolicy::default();
        policy.push_layer("Terrain").expect("layer");
        assert_eq!(policy.layers[1].id, "terrain");
        assert_eq!(policy.push_layer("Terrain").expect("layer").id, "terrain-2");
        assert_eq!(policy.layers.len(), 3);
        policy.validate().expect("valid");
    }

    #[test]
    fn duplicate_ids_names_and_limits_are_rejected() {
        let mut policy = WorkspacePolicy::default();
        policy.layers.push(Layer::new("base", "Duplicate"));
        assert_eq!(
            policy.validate(),
            Err(WorkspaceError::DuplicateId {
                kind: "layer",
                id: "base".to_owned()
            })
        );

        let mut policy = WorkspacePolicy::default();
        assert!(matches!(
            policy.push_layer("   "),
            Err(WorkspaceError::InvalidName { .. })
        ));
        for index in 0..(MAX_LAYERS - 1) {
            policy
                .push_layer(&format!("Layer {index}"))
                .expect("bounded layer");
        }
        assert!(matches!(
            policy.push_layer("One too many"),
            Err(WorkspaceError::TooMany { .. })
        ));
    }

    #[test]
    fn masks_are_dimension_scoped_and_require_full_containment() {
        let mut policy = WorkspacePolicy::default();
        let mask = policy
            .push_mask(
                "Build area",
                "minecraft:overworld",
                bounds((0, 0, 0), (15, 15, 15)),
            )
            .expect("mask")
            .id
            .clone();
        policy.active_mask_id = Some(mask);
        policy.validate().expect("valid mask");
        policy
            .check_operation(
                None,
                "minecraft:overworld",
                &bounds((1, 1, 1), (14, 14, 14)),
            )
            .expect("inside mask");
        assert_eq!(
            policy.check_operation(
                None,
                "minecraft:overworld",
                &bounds((1, 1, 1), (16, 14, 14))
            ),
            Err(WorkspaceError::MaskViolation {
                name: "Build area".to_owned()
            })
        );
        // The mask does not restrict another dimension.
        policy
            .check_operation(
                None,
                "minecraft:the_nether",
                &bounds((100, 1, 100), (120, 20, 120)),
            )
            .expect("other dimension");
    }

    #[test]
    fn protected_regions_fail_closed_on_intersection() {
        let mut policy = WorkspacePolicy::default();
        policy
            .push_protected_region(
                "Spawn",
                "minecraft:overworld",
                bounds((-10, 0, -10), (10, 20, 10)),
            )
            .expect("protected");
        policy.validate().expect("valid");
        assert_eq!(
            policy.check_operation(
                None,
                "minecraft:overworld",
                &bounds((10, 20, 10), (30, 30, 30))
            ),
            Err(WorkspaceError::ProtectedRegion {
                name: "Spawn".to_owned()
            })
        );
        policy
            .check_operation(
                None,
                "minecraft:overworld",
                &bounds((11, 0, 11), (20, 20, 20)),
            )
            .expect("outside protection");
        policy
            .check_operation(None, "minecraft:the_end", &bounds((0, 0, 0), (5, 5, 5)))
            .expect("other dimension");
    }

    #[test]
    fn locked_layers_block_explicit_and_implicit_edits() {
        let mut policy = WorkspacePolicy::default();
        policy.layers[0].locked = true;
        assert_eq!(
            policy.check_operation(None, "minecraft:overworld", &bounds((0, 0, 0), (1, 1, 1))),
            Err(WorkspaceError::LockedLayer {
                name: "Base".to_owned()
            })
        );
        assert_eq!(
            policy.check_operation(
                Some("base"),
                "minecraft:overworld",
                &bounds((0, 0, 0), (1, 1, 1))
            ),
            Err(WorkspaceError::LockedLayer {
                name: "Base".to_owned()
            })
        );
        policy.layers[0].locked = false;
        policy
            .check_operation(None, "minecraft:overworld", &bounds((0, 0, 0), (1, 1, 1)))
            .expect("unlocked");
    }

    #[test]
    fn region_limits_are_enforced() {
        let mut policy = WorkspacePolicy::default();
        assert!(matches!(
            policy.push_protected_region(
                "Huge",
                "minecraft:overworld",
                bounds((0, 0, 0), (4096, 4096, 4096))
            ),
            Err(WorkspaceError::RegionTooLarge { .. })
        ));
        for index in 0..MAX_PROTECTED_REGIONS {
            policy
                .push_protected_region(
                    &format!("Region {index}"),
                    "minecraft:overworld",
                    bounds((index as i32, 0, 0), (index as i32, 1, 1)),
                )
                .expect("bounded region");
        }
        assert!(matches!(
            policy.push_protected_region(
                "Overflow",
                "minecraft:overworld",
                bounds((0, 0, 0), (1, 1, 1))
            ),
            Err(WorkspaceError::TooMany { .. })
        ));
    }

    #[test]
    fn removing_the_last_layer_and_active_mask_is_safe() {
        let mut policy = WorkspacePolicy::default();
        assert_eq!(
            policy.remove_layer(BASE_LAYER_ID),
            Err(WorkspaceError::NoLayers)
        );
        let mask = policy
            .push_mask(
                "Allowed",
                "minecraft:overworld",
                bounds((0, 0, 0), (4, 4, 4)),
            )
            .expect("mask")
            .id
            .clone();
        policy.active_mask_id = Some(mask.clone());
        policy.remove_mask(&mask).expect("removed");
        assert!(policy.active_mask_id.is_none());
        assert!(policy
            .check_operation(
                None,
                "minecraft:overworld",
                &bounds((9, 9, 9), (10, 10, 10))
            )
            .is_ok());
        // The last layer cannot be removed, but a second layer can.
        let second = policy.push_layer("Details").expect("layer").id.clone();
        policy.remove_layer(&second).expect("removed");
        assert_eq!(policy.layers.len(), 1);
    }

    #[test]
    fn policy_problems_have_stable_codes() {
        assert_eq!(
            WorkspaceError::LockedLayer {
                name: "Base".into()
            }
            .code(),
            "permission_denied"
        );
        assert_eq!(
            WorkspaceError::TooMany {
                kind: "layer",
                found: 65,
                limit: 64
            }
            .code(),
            "limit_exceeded"
        );
        assert_eq!(
            WorkspaceError::InvalidName {
                kind: "layer",
                max: 64
            }
            .code(),
            "invalid_argument"
        );
    }
}
