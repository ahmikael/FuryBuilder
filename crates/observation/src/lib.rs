//! Deterministic spatial observations of the authoritative voxel world.
//!
//! These maps are derived views, never a second source of truth. They are rendered in Rust
//! from the same block/biome data used by every other query, so the same revision, bounds
//! and mode always produce the same PNG. Exact block reads remain the authority for
//! positions; a colour map deliberately summarises a region and says so in its metadata.

use std::collections::BTreeMap;

use png::{BitDepth, ColorType, Compression, Encoder, FilterType};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BiomeState, BlockBox, BlockPos, BlockState, Dimension};

/// Version of the observation metadata and colour contract.
pub const OBSERVATION_SCHEMA_VERSION: u32 = 1;
/// Largest image side supported by one observation.
pub const MAX_OBSERVATION_SIDE: u32 = 512;
/// Largest number of pixels supported by one observation.
pub const MAX_OBSERVATION_PIXELS: u64 = 262_144;
/// Largest number of block reads one surface or diff observation may perform.
pub const MAX_OBSERVATION_SCAN_BLOCKS: u64 = 8_000_000;
/// Maximum number of legend entries returned with an observation.
pub const MAX_LEGEND_ENTRIES: usize = 16;

const NO_SURFACE_COLOR: [u8; 3] = [17, 24, 39];
const NO_BIOME_COLOR: [u8; 3] = [107, 114, 128];
const AIR_COLOR: [u8; 3] = [55, 65, 81];
const UNCHANGED_COLOR: [u8; 3] = [24, 32, 44];
const ADDED_COLOR: [u8; 3] = [74, 222, 128];
const REMOVED_COLOR: [u8; 3] = [248, 113, 113];
const CHANGED_COLOR: [u8; 3] = [250, 204, 21];
const HEIGHT_COLORS: [[u8; 3]; 5] = [
    [29, 78, 216],
    [34, 197, 94],
    [234, 179, 8],
    [249, 115, 22],
    [248, 250, 252],
];
const HEIGHT_LABELS: [&str; 5] = [
    "Lowest elevation",
    "Low elevation",
    "Middle elevation",
    "High elevation",
    "Highest elevation",
];

/// The supported reproducible observation modes. `viewport` remains a real WebGL capture
/// and is not part of this deterministic map family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMode {
    TopDown,
    Heightmap,
    MaterialMap,
    BiomeMap,
    CrossSection,
    RevisionDiff,
}

impl ObservationMode {
    pub fn is_surface(self) -> bool {
        matches!(
            self,
            Self::TopDown | Self::Heightmap | Self::MaterialMap | Self::BiomeMap
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceAxis {
    X,
    Y,
    Z,
}

/// One orthogonal cross-section plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slice {
    pub axis: SliceAxis,
    pub coordinate: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationLegendEntry {
    pub key: String,
    pub label: String,
    pub color: String,
    pub count: u64,
}

/// Block-level revision diff totals. Columns in the image are a projection; these counts
/// are exact for the bounded region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaCounts {
    pub added: u64,
    pub removed: u64,
    pub changed: u64,
}

/// One block-level change between two revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockDelta {
    Added,
    Removed,
    Changed,
}

/// Renderer output before the application wraps it with dimension, revision and bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedObservation {
    pub width: u32,
    pub height: u32,
    pub png: Vec<u8>,
    pub legend: Vec<ObservationLegendEntry>,
    pub legend_truncated: bool,
    pub warnings: Vec<String>,
    pub scanned_blocks: u64,
    /// Surface columns for map modes, blocks for a cross-section, positions for a diff.
    pub sampled_units: u64,
    pub non_air_units: u64,
    pub counts: DeltaCounts,
}

#[derive(Debug, Clone, Copy, Default)]
struct RenderStats {
    scanned_blocks: u64,
    sampled_units: u64,
    non_air_units: u64,
    counts: DeltaCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ObservationError {
    #[error("mode {0:?} does not use this observation renderer")]
    InvalidMode(ObservationMode),
    #[error("observation image is {width}x{height}, limit is {limit}x{limit}")]
    TooLarge { width: u64, height: u64, limit: u32 },
    #[error("observation would scan {requested} blocks, limit is {limit}")]
    TooManyBlocks { requested: u64, limit: u64 },
    #[error(
        "observation Y range {min}..={max} is outside dimension `{dimension}` ({dimension_min}..={dimension_max})"
    )]
    OutOfBounds {
        dimension: String,
        min: i32,
        max: i32,
        dimension_min: i32,
        dimension_max: i32,
    },
    #[error("cross-section plane {axis:?}={coordinate} is outside the requested bounds")]
    SliceOutOfBounds { axis: SliceAxis, coordinate: i32 },
    #[error("observation block read failed: {0}")]
    World(String),
    #[error("observation PNG could not be encoded: {0}")]
    Png(String),
}

impl ObservationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge { .. } | Self::TooManyBlocks { .. } => "limit_exceeded",
            Self::OutOfBounds { .. } | Self::SliceOutOfBounds { .. } => "out_of_bounds",
            Self::InvalidMode(_) => "invalid_argument",
            Self::World(_) | Self::Png(_) => "internal",
        }
    }
}

/// Renders a top-down family map. The vertical range of `bounds` is scanned for the first
/// non-air block in each column; the X/Z extents become the image dimensions.
pub fn render_surface(
    dimension: &Dimension,
    bounds: BlockBox,
    mode: ObservationMode,
) -> Result<RenderedObservation, ObservationError> {
    if !mode.is_surface() {
        return Err(ObservationError::InvalidMode(mode));
    }
    check_dimension_y(dimension, bounds)?;
    let width = extent(bounds.min.x, bounds.max.x);
    let height = extent(bounds.min.z, bounds.max.z);
    let (width, height) = checked_image_size(width, height)?;
    let y_span = extent_i32(bounds.min.y, bounds.max.y);
    let scan_budget = u64::from(width) * u64::from(height) * y_span;
    if scan_budget > MAX_OBSERVATION_SCAN_BLOCKS {
        return Err(ObservationError::TooManyBlocks {
            requested: scan_budget,
            limit: MAX_OBSERVATION_SCAN_BLOCKS,
        });
    }

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let mut legend = LegendBuilder::default();
    let mut scanned_blocks = 0u64;
    let mut non_air_units = 0u64;

    for row in 0..height {
        let z = bounds.min.z + row as i32;
        for column in 0..width {
            let x = bounds.min.x + column as i32;
            let mut top: Option<(i32, BlockState)> = None;
            for y in (bounds.min.y..=bounds.max.y).rev() {
                let state = dimension
                    .get_block(BlockPos::new(x, y, z))
                    .map_err(|error| ObservationError::World(error.to_string()))?;
                scanned_blocks += 1;
                if !state.is_air() {
                    top = Some((y, state));
                    break;
                }
            }
            let index = (row as usize * width as usize) + column as usize;
            match mode {
                ObservationMode::TopDown => match &top {
                    Some((y, state)) => {
                        non_air_units += 1;
                        let base = block_color(state.name());
                        legend.add(state.to_string(), state.to_string(), rgb_hex(base));
                        put_pixel(&mut pixels, index, shade(base, top_down_shade(*y, bounds)));
                    }
                    None => {
                        legend.add("empty", "No surface block", rgb_hex(NO_SURFACE_COLOR));
                        put_pixel(&mut pixels, index, NO_SURFACE_COLOR);
                    }
                },
                ObservationMode::Heightmap => match &top {
                    Some((y, _)) => {
                        non_air_units += 1;
                        let bucket = height_bucket(*y, bounds.min.y, y_span);
                        legend.add(
                            format!("height:{bucket}"),
                            HEIGHT_LABELS[bucket as usize],
                            rgb_hex(HEIGHT_COLORS[bucket as usize]),
                        );
                        put_pixel(&mut pixels, index, HEIGHT_COLORS[bucket as usize]);
                    }
                    None => {
                        legend.add("height:none", "No surface block", rgb_hex(NO_SURFACE_COLOR));
                        put_pixel(&mut pixels, index, NO_SURFACE_COLOR);
                    }
                },
                ObservationMode::MaterialMap => match &top {
                    Some((_, state)) => {
                        non_air_units += 1;
                        let color = block_color(state.name());
                        legend.add(state.to_string(), state.to_string(), rgb_hex(color));
                        put_pixel(&mut pixels, index, color);
                    }
                    None => {
                        legend.add("empty", "No surface block", rgb_hex(NO_SURFACE_COLOR));
                        put_pixel(&mut pixels, index, NO_SURFACE_COLOR);
                    }
                },
                ObservationMode::BiomeMap => {
                    let y = top.as_ref().map_or(bounds.max.y, |(y, _)| *y);
                    let biome: Option<BiomeState> = dimension
                        .get_biome(BlockPos::new(x, y, z))
                        .map_err(|error| ObservationError::World(error.to_string()))?;
                    if top.is_some() {
                        non_air_units += 1;
                    }
                    match biome {
                        Some(biome) => {
                            let color = biome_color(biome.as_str());
                            legend.add(
                                format!("biome:{}", biome.as_str()),
                                biome.as_str(),
                                rgb_hex(color),
                            );
                            put_pixel(&mut pixels, index, color);
                        }
                        None => {
                            legend.add("biome:unknown", "No biome data", rgb_hex(NO_BIOME_COLOR));
                            put_pixel(&mut pixels, index, NO_BIOME_COLOR);
                        }
                    }
                }
                ObservationMode::CrossSection | ObservationMode::RevisionDiff => unreachable!(),
            }
        }
    }

    finish(
        width,
        height,
        pixels,
        legend,
        RenderStats {
            scanned_blocks,
            sampled_units: u64::from(width) * u64::from(height),
            non_air_units,
            counts: DeltaCounts::default(),
        },
    )
}

/// Renders one orthogonal plane. For X and Z planes the image is upright: row 0 is the
/// highest Y in the bounds, not the lowest.
pub fn render_cross_section(
    dimension: &Dimension,
    bounds: BlockBox,
    slice: Slice,
) -> Result<RenderedObservation, ObservationError> {
    check_dimension_y(dimension, bounds)?;
    match slice.axis {
        SliceAxis::X => {
            if slice.coordinate < bounds.min.x || slice.coordinate > bounds.max.x {
                return Err(ObservationError::SliceOutOfBounds {
                    axis: slice.axis,
                    coordinate: slice.coordinate,
                });
            }
        }
        SliceAxis::Y => {
            if !dimension.contains_y(slice.coordinate)
                || slice.coordinate < bounds.min.y
                || slice.coordinate > bounds.max.y
            {
                return Err(ObservationError::SliceOutOfBounds {
                    axis: slice.axis,
                    coordinate: slice.coordinate,
                });
            }
        }
        SliceAxis::Z => {
            if slice.coordinate < bounds.min.z || slice.coordinate > bounds.max.z {
                return Err(ObservationError::SliceOutOfBounds {
                    axis: slice.axis,
                    coordinate: slice.coordinate,
                });
            }
        }
    }

    let (width, height) = match slice.axis {
        SliceAxis::Y => (
            extent(bounds.min.x, bounds.max.x),
            extent(bounds.min.z, bounds.max.z),
        ),
        SliceAxis::X | SliceAxis::Z => (
            extent(bounds.min.z, bounds.max.z),
            extent(bounds.min.y, bounds.max.y),
        ),
    };
    let (width, height) = checked_image_size(width, height)?;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let mut legend = LegendBuilder::default();
    let mut non_air_units = 0u64;

    for row in 0..height {
        for column in 0..width {
            let pos = match slice.axis {
                SliceAxis::Y => BlockPos::new(
                    bounds.min.x + column as i32,
                    slice.coordinate,
                    bounds.min.z + row as i32,
                ),
                SliceAxis::X => BlockPos::new(
                    slice.coordinate,
                    bounds.max.y - row as i32,
                    bounds.min.z + column as i32,
                ),
                SliceAxis::Z => BlockPos::new(
                    bounds.min.x + column as i32,
                    bounds.max.y - row as i32,
                    slice.coordinate,
                ),
            };
            let state = dimension
                .get_block(pos)
                .map_err(|error| ObservationError::World(error.to_string()))?;
            let color = if state.is_air() {
                AIR_COLOR
            } else {
                non_air_units += 1;
                block_color(state.name())
            };
            let label = if state.is_air() {
                "Air".to_owned()
            } else {
                state.to_string()
            };
            legend.add(format!("state:{state}"), label, rgb_hex(color));
            let index = row as usize * width as usize + column as usize;
            put_pixel(&mut pixels, index, color);
        }
    }

    finish(
        width,
        height,
        pixels,
        legend,
        RenderStats {
            scanned_blocks: u64::from(width) * u64::from(height),
            sampled_units: u64::from(width) * u64::from(height),
            non_air_units,
            counts: DeltaCounts::default(),
        },
    )
}

/// Renders a top-down projection of block-level changes. Multiple changed blocks in one
/// column are collapsed by priority (added > removed > changed); `counts` remains exact.
pub fn render_revision_diff(
    bounds: BlockBox,
    deltas: &BTreeMap<BlockPos, BlockDelta>,
) -> Result<RenderedObservation, ObservationError> {
    let width = extent(bounds.min.x, bounds.max.x);
    let height = extent(bounds.min.z, bounds.max.z);
    let (width, height) = checked_image_size(width, height)?;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    for index in 0..width as usize * height as usize {
        put_pixel(&mut pixels, index, UNCHANGED_COLOR);
    }
    let mut priorities = vec![0u8; width as usize * height as usize];
    let mut counts = DeltaCounts::default();

    for (pos, delta) in deltas {
        if !bounds.contains(*pos) {
            continue;
        }
        let (priority, color) = match delta {
            BlockDelta::Added => {
                counts.added += 1;
                (3u8, ADDED_COLOR)
            }
            BlockDelta::Removed => {
                counts.removed += 1;
                (2u8, REMOVED_COLOR)
            }
            BlockDelta::Changed => {
                counts.changed += 1;
                (1u8, CHANGED_COLOR)
            }
        };
        let column = (pos.x - bounds.min.x) as usize;
        let row = (pos.z - bounds.min.z) as usize;
        let index = row * width as usize + column;
        if priority > priorities[index] {
            priorities[index] = priority;
            put_pixel(&mut pixels, index, color);
        }
    }

    let mut legend = LegendBuilder::default();
    if counts.added > 0 {
        legend.add("diff:added", "Added blocks", rgb_hex(ADDED_COLOR));
        legend.set_count("diff:added", counts.added);
    }
    if counts.removed > 0 {
        legend.add("diff:removed", "Removed blocks", rgb_hex(REMOVED_COLOR));
        legend.set_count("diff:removed", counts.removed);
    }
    if counts.changed > 0 {
        legend.add("diff:changed", "Changed blocks", rgb_hex(CHANGED_COLOR));
        legend.set_count("diff:changed", counts.changed);
    }
    let mut warnings = Vec::new();
    if counts.added + counts.removed + counts.changed == 0 {
        warnings.push("No block changes were found in the requested region.".to_owned());
    }
    warnings.push(
        "Revision diff is a top-down projection; use region_blocks for exact positions.".to_owned(),
    );

    let mut rendered = finish(
        width,
        height,
        pixels,
        legend,
        RenderStats {
            scanned_blocks: bounds.volume(),
            sampled_units: bounds.volume(),
            non_air_units: 0,
            counts,
        },
    )?;
    rendered.warnings.extend(warnings);
    Ok(rendered)
}

fn finish(
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    legend: LegendBuilder,
    stats: RenderStats,
) -> Result<RenderedObservation, ObservationError> {
    let (legend, legend_truncated) = legend.finish();
    let mut warnings = Vec::new();
    if legend_truncated {
        warnings.push(format!(
            "Legend shows the {MAX_LEGEND_ENTRIES} most common entries; more distinct entries exist."
        ));
    }
    let png = encode_png(width, height, &pixels)?;
    Ok(RenderedObservation {
        width,
        height,
        png,
        legend,
        legend_truncated,
        warnings,
        scanned_blocks: stats.scanned_blocks,
        sampled_units: stats.sampled_units,
        non_air_units: stats.non_air_units,
        counts: stats.counts,
    })
}

fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, ObservationError> {
    let mut output = Vec::new();
    {
        let mut encoder = Encoder::new(&mut output, width, height);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        encoder.set_compression(Compression::Fast);
        encoder.set_filter(FilterType::NoFilter);
        let mut writer = encoder
            .write_header()
            .map_err(|error| ObservationError::Png(error.to_string()))?;
        writer
            .write_image_data(pixels)
            .map_err(|error| ObservationError::Png(error.to_string()))?;
    }
    Ok(output)
}

fn check_dimension_y(dimension: &Dimension, bounds: BlockBox) -> Result<(), ObservationError> {
    if dimension.contains_y(bounds.min.y) && dimension.contains_y(bounds.max.y) {
        return Ok(());
    }
    Err(ObservationError::OutOfBounds {
        dimension: dimension.id().to_owned(),
        min: bounds.min.y,
        max: bounds.max.y,
        dimension_min: dimension.min_y(),
        dimension_max: dimension.max_y(),
    })
}

fn extent(start: i32, end: i32) -> u64 {
    (i64::from(end) - i64::from(start) + 1) as u64
}

fn extent_i32(start: i32, end: i32) -> u64 {
    extent(start, end)
}

fn checked_image_size(width: u64, height: u64) -> Result<(u32, u32), ObservationError> {
    if width == 0 || height == 0 || width > u64::from(MAX_OBSERVATION_SIDE) {
        return Err(ObservationError::TooLarge {
            width,
            height,
            limit: MAX_OBSERVATION_SIDE,
        });
    }
    if height > u64::from(MAX_OBSERVATION_SIDE) || width * height > MAX_OBSERVATION_PIXELS {
        return Err(ObservationError::TooLarge {
            width,
            height,
            limit: MAX_OBSERVATION_SIDE,
        });
    }
    Ok((width as u32, height as u32))
}

fn top_down_shade(y: i32, bounds: BlockBox) -> u32 {
    let span = extent_i32(bounds.min.y, bounds.max.y);
    if span <= 1 {
        return 100;
    }
    let relative = u64::try_from(i64::from(y) - i64::from(bounds.min.y)).unwrap_or(0);
    70 + (50 * relative / (span - 1)) as u32
}

fn height_bucket(y: i32, min_y: i32, span: u64) -> u8 {
    if span <= 1 {
        return 0;
    }
    let relative = u64::try_from(i64::from(y) - i64::from(min_y)).unwrap_or(0);
    u8::try_from((relative * 4 / (span - 1)).min(4)).unwrap_or(0)
}

fn put_pixel(pixels: &mut [u8], index: usize, color: [u8; 3]) {
    let offset = index * 4;
    pixels[offset] = color[0];
    pixels[offset + 1] = color[1];
    pixels[offset + 2] = color[2];
    pixels[offset + 3] = 255;
}

fn shade(color: [u8; 3], percent: u32) -> [u8; 3] {
    let scale = |value: u8| -> u8 {
        u8::try_from((u32::from(value) * percent / 100).min(255)).unwrap_or(255)
    };
    [scale(color[0]), scale(color[1]), scale(color[2])]
}

fn rgb_hex(color: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

#[derive(Default)]
struct LegendBuilder {
    entries: BTreeMap<String, LegendAccumulator>,
}

struct LegendAccumulator {
    label: String,
    color: String,
    count: u64,
}

impl LegendBuilder {
    fn add(&mut self, key: impl Into<String>, label: impl Into<String>, color: String) {
        let entry = self
            .entries
            .entry(key.into())
            .or_insert_with(|| LegendAccumulator {
                label: label.into(),
                color,
                count: 0,
            });
        entry.count += 1;
    }

    fn set_count(&mut self, key: &str, count: u64) {
        if let Some(entry) = self.entries.get_mut(key) {
            entry.count = count;
        }
    }

    fn finish(self) -> (Vec<ObservationLegendEntry>, bool) {
        let mut entries: Vec<ObservationLegendEntry> = self
            .entries
            .into_iter()
            .map(|(key, entry)| ObservationLegendEntry {
                key,
                label: entry.label,
                color: entry.color,
                count: entry.count,
            })
            .collect();
        entries.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.key.cmp(&right.key))
        });
        let truncated = entries.len() > MAX_LEGEND_ENTRIES;
        entries.truncate(MAX_LEGEND_ENTRIES);
        (entries, truncated)
    }
}

fn base_name(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, path)| path)
}

fn block_color(name: &str) -> [u8; 3] {
    match base_name(name) {
        "grass_block" | "moss_block" => [95, 158, 74],
        "dirt" | "coarse_dirt" | "rooted_dirt" => [122, 82, 48],
        "podzol" | "mycelium" => [107, 75, 42],
        "stone" | "smooth_stone" => [138, 143, 152],
        "cobblestone" | "mossy_cobblestone" => [115, 119, 127],
        "stone_bricks" | "mossy_stone_bricks" | "cracked_stone_bricks" => [154, 160, 166],
        "deepslate" | "cobbled_deepslate" | "polished_deepslate" => [75, 79, 85],
        "sand" | "suspicious_sand" => [217, 207, 154],
        "sandstone" | "smooth_sandstone" => [203, 191, 134],
        "gravel" => [143, 139, 134],
        "water" => [60, 111, 216],
        "lava" => [226, 103, 43],
        "oak_planks" | "stripped_oak_wood" => [176, 138, 84],
        "spruce_planks" | "stripped_spruce_wood" => [116, 82, 58],
        "birch_planks" | "stripped_birch_wood" => [214, 196, 143],
        "jungle_planks" => [160, 115, 80],
        "acacia_planks" => [168, 90, 50],
        "dark_oak_planks" | "stripped_dark_oak_wood" => [79, 53, 33],
        "oak_log" | "oak_wood" => [107, 83, 52],
        "spruce_log" | "spruce_wood" => [76, 56, 38],
        "birch_log" | "birch_wood" => [216, 214, 198],
        "dark_oak_log" | "dark_oak_wood" => [64, 43, 26],
        "oak_leaves" | "jungle_leaves" | "acacia_leaves" => [63, 122, 58],
        "spruce_leaves" | "dark_oak_leaves" => [45, 89, 52],
        "glass" | "tinted_glass" => [169, 215, 232],
        "snow_block" | "snow" => [238, 244, 247],
        "ice" | "blue_ice" | "packed_ice" => [158, 205, 232],
        "clay" => [164, 168, 184],
        "bricks" | "mud_bricks" => [155, 86, 69],
        "terracotta" => [160, 90, 62],
        "bedrock" => [51, 51, 51],
        "obsidian" | "crying_obsidian" => [36, 26, 54],
        "netherrack" => [107, 47, 47],
        "end_stone" => [217, 214, 163],
        "andesite" | "polished_andesite" => [132, 134, 133],
        "diorite" | "polished_diorite" => [188, 188, 190],
        "granite" | "polished_granite" => [150, 103, 84],
        "calcite" => [223, 224, 220],
        "dripstone_block" => [134, 107, 92],
        "mud" | "packed_mud" => [59, 53, 45],
        "mangrove_roots" => [74, 58, 43],
        "glowstone" | "sea_lantern" => [235, 214, 142],
        "coal_ore" | "deepslate_coal_ore" => [72, 74, 76],
        "iron_ore" | "deepslate_iron_ore" => [148, 121, 100],
        "gold_ore" | "deepslate_gold_ore" => [207, 175, 80],
        "diamond_ore" | "deepslate_diamond_ore" => [94, 207, 202],
        "redstone_ore" | "deepslate_redstone_ore" => [156, 46, 46],
        "emerald_ore" | "deepslate_emerald_ore" => [47, 160, 91],
        _ if base_name(name).ends_with("_planks") => [176, 138, 84],
        _ if base_name(name).ends_with("_log") || base_name(name).ends_with("_wood") => {
            [107, 83, 52]
        }
        _ if base_name(name).ends_with("_leaves") => [63, 122, 58],
        _ if base_name(name).ends_with("_glass") => [169, 215, 232],
        _ if base_name(name).ends_with("_terracotta") => [160, 90, 62],
        _ if base_name(name).ends_with("_bricks") => [155, 86, 69],
        _ if base_name(name).ends_with("_wool") => [200, 200, 200],
        _ if base_name(name).ends_with("_concrete") => [160, 160, 160],
        _ => hash_color(base_name(name), 0x811c_9dc5),
    }
}

fn biome_color(name: &str) -> [u8; 3] {
    match base_name(name) {
        "plains" | "sunflower_plains" => [121, 169, 76],
        "forest" | "flower_forest" | "birch_forest" | "dark_forest" => [62, 122, 63],
        "taiga" | "old_growth_pine_taiga" | "old_growth_spruce_taiga" => [75, 127, 104],
        "desert" | "beach" => [216, 201, 138],
        "savanna" | "savanna_plateau" | "windswept_savanna" => [169, 164, 90],
        "swamp" | "mangrove_swamp" => [80, 108, 71],
        "jungle" | "sparse_jungle" | "bamboo_jungle" => [47, 122, 53],
        "ocean" | "deep_ocean" | "warm_ocean" | "cold_ocean" => [47, 98, 181],
        "river" | "frozen_river" => [59, 121, 196],
        "snowy_plains" | "snowy_taiga" | "snowy_slopes" | "frozen_ocean" => [223, 232, 236],
        "mushroom_fields" => [138, 111, 126],
        "badlands" | "eroded_badlands" | "wooded_badlands" => [181, 107, 63],
        "nether_wastes" | "crimson_forest" | "warped_forest" => [138, 47, 47],
        "the_end" | "end_highlands" | "end_midlands" => [217, 214, 163],
        "the_void" => [17, 24, 39],
        _ => hash_color(base_name(name), 0x9e37_79b9),
    }
}

const HASH_PALETTE: [[u8; 3]; 16] = [
    [139, 92, 246],
    [236, 72, 153],
    [14, 165, 233],
    [16, 185, 129],
    [245, 158, 11],
    [239, 68, 68],
    [99, 102, 241],
    [20, 184, 166],
    [217, 70, 239],
    [132, 204, 22],
    [249, 115, 22],
    [59, 130, 246],
    [168, 85, 247],
    [244, 63, 94],
    [6, 182, 212],
    [234, 179, 8],
];

fn hash_color(key: &str, salt: u32) -> [u8; 3] {
    let mut hash = 2_166_136_261u32 ^ salt;
    for byte in key.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    HASH_PALETTE[(hash as usize) % HASH_PALETTE.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use world_domain::{BiomeSection, ChunkPos, Dimension};

    fn state(value: &str) -> BlockState {
        value.parse().expect("valid block state")
    }

    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = png::Decoder::new(Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("png info");
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).expect("png frame");
        (info.width, info.height, buffer)
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    }

    #[test]
    fn surface_maps_are_deterministic_and_match_top_blocks() {
        let mut dimension = Dimension::overworld();
        dimension
            .set_block(BlockPos::new(0, 64, 0), &state("minecraft:stone"))
            .unwrap();
        dimension
            .set_block(BlockPos::new(0, 65, 0), &state("minecraft:grass_block"))
            .unwrap();
        dimension
            .set_block(BlockPos::new(1, 64, 0), &state("minecraft:dirt"))
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 65, 0)).unwrap();

        let first = render_surface(&dimension, bounds, ObservationMode::MaterialMap).unwrap();
        let second = render_surface(&dimension, bounds, ObservationMode::MaterialMap).unwrap();
        assert_eq!(first, second);
        assert_eq!((first.width, first.height), (2, 1));
        assert_eq!(first.legend.len(), 2);
        assert_eq!(first.non_air_units, 2);
        assert_eq!(first.scanned_blocks, 3);
        let (width, height, pixels) = decode_png(&first.png);
        assert_eq!((width, height), (2, 1));
        assert_eq!(pixel(&pixels, width, 0, 0), [95, 158, 74, 255]);
        assert_eq!(pixel(&pixels, width, 1, 0), [122, 82, 48, 255]);
    }

    #[test]
    fn heightmap_uses_elevation_buckets() {
        let mut dimension = Dimension::overworld();
        dimension
            .set_block(BlockPos::new(0, 64, 0), &state("minecraft:stone"))
            .unwrap();
        dimension
            .set_block(BlockPos::new(1, 65, 0), &state("minecraft:stone"))
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 65, 0)).unwrap();
        let rendered = render_surface(&dimension, bounds, ObservationMode::Heightmap).unwrap();
        let (_, _, pixels) = decode_png(&rendered.png);
        assert_eq!(
            pixel(&pixels, 2, 0, 0),
            [
                HEIGHT_COLORS[0][0],
                HEIGHT_COLORS[0][1],
                HEIGHT_COLORS[0][2],
                255
            ]
        );
        assert_eq!(
            pixel(&pixels, 2, 1, 0),
            [
                HEIGHT_COLORS[4][0],
                HEIGHT_COLORS[4][1],
                HEIGHT_COLORS[4][2],
                255
            ]
        );
        assert_eq!(rendered.legend.len(), 2);
    }

    #[test]
    fn biome_map_reports_missing_and_present_data() {
        let mut dimension = Dimension::overworld();
        dimension
            .set_block(BlockPos::new(0, 64, 0), &state("minecraft:grass_block"))
            .unwrap();
        dimension
            .set_block(BlockPos::new(1, 64, 0), &state("minecraft:grass_block"))
            .unwrap();
        let plains = "minecraft:plains".parse().expect("biome");
        let section = BiomeSection::from_palette(vec![plains], vec![0; 64]).expect("biomes");
        dimension.install_biomes(ChunkPos::new(0, 0), 4, Some(section));
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)).unwrap();
        let rendered = render_surface(&dimension, bounds, ObservationMode::BiomeMap).unwrap();
        assert!(rendered
            .legend
            .iter()
            .any(|entry| entry.key == "biome:minecraft:plains" && entry.count == 2));
    }

    #[test]
    fn cross_section_renders_an_upright_plane() {
        let mut dimension = Dimension::overworld();
        dimension
            .set_block(BlockPos::new(0, 64, 0), &state("minecraft:stone"))
            .unwrap();
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 65, 0)).unwrap();
        let rendered = render_cross_section(
            &dimension,
            bounds,
            Slice {
                axis: SliceAxis::X,
                coordinate: 0,
            },
        )
        .unwrap();
        assert_eq!((rendered.width, rendered.height), (1, 2));
        let (_, _, pixels) = decode_png(&rendered.png);
        // Row 0 is y=65 (air), row 1 is y=64 (stone).
        assert_eq!(
            pixel(&pixels, 1, 0, 0),
            [AIR_COLOR[0], AIR_COLOR[1], AIR_COLOR[2], 255]
        );
        assert_eq!(pixel(&pixels, 1, 0, 1), [138, 143, 152, 255]);
        assert_eq!(rendered.non_air_units, 1);
    }

    #[test]
    fn revision_diff_reports_exact_counts_and_column_priority() {
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)).unwrap();
        let mut deltas = BTreeMap::new();
        deltas.insert(BlockPos::new(0, 64, 0), BlockDelta::Added);
        deltas.insert(BlockPos::new(1, 64, 0), BlockDelta::Removed);
        let rendered = render_revision_diff(bounds, &deltas).unwrap();
        assert_eq!(
            rendered.counts,
            DeltaCounts {
                added: 1,
                removed: 1,
                changed: 0
            }
        );
        let (_, _, pixels) = decode_png(&rendered.png);
        assert_eq!(pixel(&pixels, 2, 0, 0), [74, 222, 128, 255]);
        assert_eq!(pixel(&pixels, 2, 1, 0), [248, 113, 113, 255]);
        assert!(rendered
            .warnings
            .iter()
            .any(|warning| warning.contains("projection")));
    }

    #[test]
    fn budgets_are_fail_closed() {
        let dimension = Dimension::overworld();
        let wide = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(512, 60, 0)).unwrap();
        assert!(matches!(
            render_surface(&dimension, wide, ObservationMode::TopDown),
            Err(ObservationError::TooLarge { .. })
        ));

        let tall = BlockBox::new(BlockPos::new(0, -64, 0), BlockPos::new(160, 319, 160)).unwrap();
        assert!(matches!(
            render_surface(&dimension, tall, ObservationMode::TopDown),
            Err(ObservationError::TooManyBlocks { .. })
        ));

        let outside = BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(0, 0, 0)).unwrap();
        let rendered = render_cross_section(
            &dimension,
            outside,
            Slice {
                axis: SliceAxis::Y,
                coordinate: 400,
            },
        );
        assert!(matches!(
            rendered,
            Err(ObservationError::SliceOutOfBounds { .. })
        ));
    }

    #[test]
    fn legend_is_bounded_and_metadata_is_stable() {
        let mut dimension = Dimension::overworld();
        for x in 0..20 {
            let block = format!("minecraft:test_block_{x}");
            // The registry in the domain only validates syntax, not known block ids.
            dimension
                .set_block(BlockPos::new(x, 64, 0), &state(&block))
                .unwrap();
        }
        let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(19, 64, 0)).unwrap();
        let rendered = render_surface(&dimension, bounds, ObservationMode::MaterialMap).unwrap();
        assert_eq!(rendered.legend.len(), MAX_LEGEND_ENTRIES);
        assert!(rendered.legend_truncated);
        assert!(rendered
            .warnings
            .iter()
            .any(|warning| warning.contains("most common")));
    }
}
