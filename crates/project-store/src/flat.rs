//! Version-aware parameters for the deterministic flat starting shape.
//!
//! The flat base is deliberate editor geometry, not vanilla world generation. Its layers,
//! height and biome are user input, but every value is validated against the selected version
//! profile before a single block is written.

use minecraft_version::{CatalogRegistryFormat, DimensionKind, VersionProfile};
use serde::{Deserialize, Serialize};
use world_domain::{BiomeSection, BiomeState, BlockPos, BlockRegistry, ChunkPos, World};

use crate::error::ProjectError;

/// The platform always covers a fixed odd-sized footprint around the origin.
pub const FLAT_FOOTPRINT: i32 = 33;
pub const MAX_FLAT_LAYERS: usize = 8;
pub const MAX_FLAT_LAYER_THICKNESS: u32 = 16;
pub const MAX_FLAT_TOTAL_THICKNESS: u32 = 64;

/// Biome ids offered as starting values for the flat source. The full version catalog will
/// replace this quick list once components are resolved.
pub const FLAT_BIOME_CHOICES: &[&str] = &[
    "minecraft:plains",
    "minecraft:sunflower_plains",
    "minecraft:forest",
    "minecraft:birch_forest",
    "minecraft:taiga",
    "minecraft:snowy_plains",
    "minecraft:desert",
    "minecraft:savanna",
    "minecraft:swamp",
    "minecraft:jungle",
    "minecraft:badlands",
    "minecraft:cherry_grove",
    "minecraft:ocean",
];

/// One horizontal layer in the flat starting shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlatLayer {
    pub block: String,
    pub thickness: u32,
}

/// Bottom-up flat source definition. Layers are listed from the lowest to the highest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FlatPreset {
    pub base_y: i32,
    pub layers: Vec<FlatLayer>,
    pub biome: String,
}

impl Default for FlatPreset {
    fn default() -> Self {
        Self {
            base_y: 62,
            layers: vec![
                FlatLayer {
                    block: "minecraft:stone".to_owned(),
                    thickness: 1,
                },
                FlatLayer {
                    block: "minecraft:dirt".to_owned(),
                    thickness: 1,
                },
                FlatLayer {
                    block: "minecraft:grass_block".to_owned(),
                    thickness: 1,
                },
            ],
            biome: "minecraft:plains".to_owned(),
        }
    }
}

impl FlatPreset {
    /// Total thickness in blocks. Widened so malformed IPC values cannot overflow the sum
    /// before the per-layer limits reject them.
    pub fn total_thickness(&self) -> u64 {
        self.layers
            .iter()
            .map(|layer| u64::from(layer.thickness))
            .sum()
    }

    pub fn top_y(&self) -> i64 {
        let total = i64::try_from(self.total_thickness()).unwrap_or(i64::MAX);
        i64::from(self.base_y)
            .saturating_add(total)
            .saturating_sub(1)
    }

    /// Rejects every value that cannot be represented safely by the selected version.
    pub fn validate(&self, profile: &VersionProfile) -> Result<(), ProjectError> {
        if !profile.capabilities.write {
            return Err(ProjectError::UnsupportedWorldVersion(format!(
                "the selected version profile `{}` is not writable",
                profile.id
            )));
        }
        if profile.content_catalog.registry_format != CatalogRegistryFormat::Named {
            return Err(ProjectError::InvalidFlatPreset(
                "this version does not use named block ids, so the flat source editor cannot describe its layers"
                    .to_owned(),
            ));
        }
        if self.layers.is_empty() {
            return Err(ProjectError::InvalidFlatPreset(
                "add at least one flat layer".to_owned(),
            ));
        }
        if self.layers.len() > MAX_FLAT_LAYERS {
            return Err(ProjectError::InvalidFlatPreset(format!(
                "a flat source supports at most {MAX_FLAT_LAYERS} layers"
            )));
        }
        let total = self.total_thickness();
        if total == 0 || total > u64::from(MAX_FLAT_TOTAL_THICKNESS) {
            return Err(ProjectError::InvalidFlatPreset(format!(
                "the flat source must be between 1 and {MAX_FLAT_TOTAL_THICKNESS} blocks thick"
            )));
        }

        let registry = BlockRegistry::target_1_21_4();
        for (index, layer) in self.layers.iter().enumerate() {
            if layer.thickness == 0 || layer.thickness > MAX_FLAT_LAYER_THICKNESS {
                return Err(ProjectError::InvalidFlatPreset(format!(
                    "layer {} must be between 1 and {MAX_FLAT_LAYER_THICKNESS} blocks thick",
                    index + 1
                )));
            }
            let state = registry
                .canonicalize(&layer.block, std::collections::BTreeMap::new())
                .map_err(|error| {
                    ProjectError::InvalidFlatPreset(format!(
                        "layer {} block is not in this build's verified starting set: {error}",
                        index + 1
                    ))
                })?;
            if state.name() == "minecraft:air" {
                return Err(ProjectError::InvalidFlatPreset(format!(
                    "layer {} cannot be air",
                    index + 1
                )));
            }
        }

        self.biome.parse::<BiomeState>().map_err(|error| {
            ProjectError::InvalidFlatPreset(format!("the flat biome is invalid: {error}"))
        })?;

        let overworld = profile
            .dimension_profiles
            .iter()
            .find(|dimension| dimension.kind == DimensionKind::Overworld)
            .ok_or_else(|| {
                ProjectError::InvalidFlatPreset(
                    "the selected version profile has no overworld dimension".to_owned(),
                )
            })?;
        let min_y = i64::from(overworld.min_y);
        let max_y = min_y + i64::from(overworld.height) - 1;
        let top = self.top_y();
        if i64::from(self.base_y) < min_y || top > max_y {
            return Err(ProjectError::InvalidFlatPreset(format!(
                "the flat source must stay inside overworld Y {min_y}..={max_y}; this one spans Y {}..={top}",
                self.base_y
            )));
        }
        Ok(())
    }
}

/// Limits and quick choices owned by the backend so the wizard never invents product limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlatLimits {
    pub max_layers: usize,
    pub max_layer_thickness: u32,
    pub max_total_thickness: u32,
    pub footprint: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlatDefaults {
    pub preset: FlatPreset,
    pub limits: FlatLimits,
    pub block_choices: Vec<String>,
    pub biome_choices: Vec<String>,
}

impl Default for FlatDefaults {
    fn default() -> Self {
        let registry = BlockRegistry::target_1_21_4();
        Self {
            preset: FlatPreset::default(),
            limits: FlatLimits {
                max_layers: MAX_FLAT_LAYERS,
                max_layer_thickness: MAX_FLAT_LAYER_THICKNESS,
                max_total_thickness: MAX_FLAT_TOTAL_THICKNESS,
                footprint: FLAT_FOOTPRINT,
            },
            block_choices: registry
                .names()
                .filter(|name| *name != "minecraft:air")
                .map(str::to_owned)
                .collect(),
            biome_choices: FLAT_BIOME_CHOICES
                .iter()
                .map(|id| (*id).to_owned())
                .collect(),
        }
    }
}

/// Writes the validated flat source into an already versioned world.
pub fn build_flat_base(
    world: &mut World,
    preset: &FlatPreset,
    profile: &VersionProfile,
) -> Result<(), ProjectError> {
    preset.validate(profile)?;
    let biome: BiomeState = preset
        .biome
        .parse()
        .map_err(|error| ProjectError::InvalidFlatPreset(format!("invalid biome: {error}")))?;
    let half = FLAT_FOOTPRINT / 2;
    let registry = BlockRegistry::target_1_21_4();

    let mut y = i64::from(preset.base_y);
    for layer in &preset.layers {
        let state = registry
            .canonicalize(&layer.block, std::collections::BTreeMap::new())
            .map_err(|error| {
                ProjectError::InvalidFlatPreset(format!(
                    "invalid flat block `{}`: {error}",
                    layer.block
                ))
            })?;
        for _ in 0..layer.thickness {
            if y > i64::from(i32::MAX) {
                return Err(ProjectError::InvalidFlatPreset(
                    "the flat source exceeds the supported height".to_owned(),
                ));
            }
            let block_y = y as i32;
            for x in -half..=half {
                for z in -half..=half {
                    world
                        .set_block("minecraft:overworld", BlockPos::new(x, block_y, z), &state)
                        .map_err(|error| ProjectError::corrupt(error.to_string()))?;
                }
            }
            y += 1;
        }
    }

    let overworld = profile
        .dimension_profiles
        .iter()
        .find(|dimension| dimension.kind == DimensionKind::Overworld)
        .ok_or_else(|| {
            ProjectError::InvalidFlatPreset(
                "the selected version profile has no overworld dimension".to_owned(),
            )
        })?;
    let min_section = overworld.min_y.div_euclid(16);
    let max_section = (overworld.min_y + overworld.height - 1).div_euclid(16);
    for chunk_x in -1..=1 {
        for chunk_z in -1..=1 {
            for section_y in min_section..=max_section {
                world
                    .install_biomes(
                        "minecraft:overworld",
                        ChunkPos::new(chunk_x, chunk_z),
                        section_y,
                        Some(BiomeSection::uniform(biome.clone())),
                    )
                    .map_err(|error| ProjectError::corrupt(error.to_string()))?;
            }
        }
    }
    Ok(())
}
