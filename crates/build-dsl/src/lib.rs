use std::collections::BTreeMap;

use scene_model::{
    GeneratorKind, HouseParams, ParametricBaseline, ParametricObject, Rotation,
    HOUSE_GENERATOR_VERSION,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BlockBox, BlockPos, BlockState};
use world_operations::{BlockChange, Operation};

/// Version of the declarative recipe schema. It is independent from the world operation
/// schema and from the project schema.
pub const BUILD_DSL_SCHEMA_VERSION: u32 = 1;
/// Maximum number of top-level nodes in one recipe.
pub const MAX_DSL_NODES: usize = 64;
/// Maximum number of expanded house instances after repeating nodes.
pub const MAX_DSL_HOUSES: usize = 64;
/// Maximum number of blocks one compiled recipe may write.
pub const MAX_DSL_BLOCKS: u64 = 16_384;
/// Maximum nesting depth of repeat nodes.
pub const MAX_DSL_DEPTH: usize = 4;
/// Maximum number of copies produced by one repeat node.
pub const MAX_REPEAT_COUNT: u32 = 16;
/// Maximum spacing between repeated copies on one axis.
pub const MAX_REPEAT_SPACING: u32 = 64;
/// Maximum dimension identifier length accepted by the recipe schema.
pub const MAX_DSL_DIMENSION_CHARS: usize = 128;

fn default_schema_version() -> u32 {
    BUILD_DSL_SCHEMA_VERSION
}

/// A versioned, declarative build recipe. It contains data only: there is no expression
/// language, script hook or filesystem access. The Core validates and compiles it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildRecipe {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub dimension: String,
    #[serde(default)]
    pub nodes: Vec<BuildNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BuildNode {
    House(HouseNode),
    Repeat(RepeatNode),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HouseNode {
    pub at: [i32; 3],
    #[serde(default)]
    pub rotation: Rotation,
    pub params: HouseParams,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepeatNode {
    pub axis: RepeatAxis,
    pub count: u32,
    pub spacing: u32,
    pub child: Box<BuildNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatAxis {
    X,
    Y,
    Z,
}

/// Metadata returned to UI/MCP without the potentially large block list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildDslSummary {
    pub dimension: String,
    pub bounds: BlockBox,
    pub houses: u64,
    pub blocks: u64,
    pub warnings: Vec<String>,
}

/// Deterministic compilation output, ready to be wrapped in the common Operation API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledBuild {
    summary: BuildDslSummary,
    changes: Vec<BlockChange>,
}

impl CompiledBuild {
    pub fn summary(&self) -> &BuildDslSummary {
        &self.summary
    }

    pub fn changes(&self) -> &[BlockChange] {
        &self.changes
    }

    /// The only operation produced by the DSL. Everything downstream (preview, policy,
    /// confirmation, revision, history, undo) is the ordinary Batch path.
    pub fn operation(&self) -> Operation {
        Operation::Batch {
            dimension: self.summary.dimension.clone(),
            changes: self.changes.clone(),
        }
    }

    pub fn into_operation(self) -> Operation {
        Operation::Batch {
            dimension: self.summary.dimension,
            changes: self.changes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildDslError {
    #[error("build DSL schema {found} is not supported; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("build DSL recipe has no nodes")]
    EmptyRecipe,
    #[error("build DSL dimension `{0}` is invalid")]
    InvalidDimension(String),
    #[error("build DSL recipe has {requested} top-level nodes, limit is {limit}")]
    TooManyNodes { requested: u64, limit: usize },
    #[error("build DSL expands to {requested} houses, limit is {limit}")]
    TooManyHouses { requested: u64, limit: usize },
    #[error("build DSL writes {requested} blocks, limit is {limit}")]
    TooManyBlocks { requested: u64, limit: u64 },
    #[error("build DSL repeat nesting exceeds depth {0}")]
    DepthExceeded(usize),
    #[error("invalid build DSL repeat: {0}")]
    InvalidRepeat(String),
    #[error("build DSL coordinates overflow: {0}")]
    InvalidCoordinates(String),
    #[error("invalid build DSL parameters: {0}")]
    InvalidParams(String),
    #[error("build DSL nodes overlap at {0:?}")]
    Overlap(BlockPos),
    #[error("build DSL produced no blocks")]
    EmptyOutput,
}

impl BuildDslError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedSchema { .. } => "unsupported_version",
            Self::TooManyNodes { .. }
            | Self::TooManyHouses { .. }
            | Self::TooManyBlocks { .. }
            | Self::DepthExceeded(_) => "limit_exceeded",
            Self::Overlap(_) => "conflict",
            Self::EmptyRecipe
            | Self::InvalidDimension(_)
            | Self::InvalidRepeat(_)
            | Self::InvalidCoordinates(_)
            | Self::InvalidParams(_)
            | Self::EmptyOutput => "invalid_argument",
        }
    }
}

/// Validates a recipe and compiles it into deterministic block changes. No world state is
/// read or written here, so the same recipe and generator version always produce the same
/// operation.
pub fn compile(recipe: &BuildRecipe) -> Result<CompiledBuild, BuildDslError> {
    if recipe.schema_version != BUILD_DSL_SCHEMA_VERSION {
        return Err(BuildDslError::UnsupportedSchema {
            found: recipe.schema_version,
            expected: BUILD_DSL_SCHEMA_VERSION,
        });
    }
    let dimension = recipe.dimension.trim();
    if dimension.is_empty()
        || dimension.chars().count() > MAX_DSL_DIMENSION_CHARS
        || dimension.chars().any(char::is_whitespace)
    {
        return Err(BuildDslError::InvalidDimension(recipe.dimension.clone()));
    }
    if recipe.nodes.is_empty() {
        return Err(BuildDslError::EmptyRecipe);
    }
    if recipe.nodes.len() > MAX_DSL_NODES {
        return Err(BuildDslError::TooManyNodes {
            requested: recipe.nodes.len() as u64,
            limit: MAX_DSL_NODES,
        });
    }

    let mut state = CompileState {
        dimension,
        blocks: BTreeMap::new(),
        houses: 0,
        min: None,
        max: None,
    };
    for node in &recipe.nodes {
        expand(node, [0, 0, 0], 1, &mut state)?;
    }
    let (min, max) = match (state.min, state.max) {
        (Some(min), Some(max)) => (min, max),
        _ => return Err(BuildDslError::EmptyOutput),
    };
    let bounds = BlockBox::new(min, max)
        .map_err(|error| BuildDslError::InvalidCoordinates(error.to_string()))?;
    let changes: Vec<BlockChange> = state
        .blocks
        .into_iter()
        .map(|(pos, state): (BlockPos, BlockState)| BlockChange { pos, state })
        .collect();
    if changes.is_empty() {
        return Err(BuildDslError::EmptyOutput);
    }
    Ok(CompiledBuild {
        summary: BuildDslSummary {
            dimension: dimension.to_owned(),
            bounds,
            houses: state.houses,
            blocks: changes.len() as u64,
            warnings: Vec::new(),
        },
        changes,
    })
}

struct CompileState<'a> {
    dimension: &'a str,
    blocks: BTreeMap<BlockPos, BlockState>,
    houses: u64,
    min: Option<BlockPos>,
    max: Option<BlockPos>,
}

fn expand(
    node: &BuildNode,
    offset: [i64; 3],
    depth: usize,
    state: &mut CompileState<'_>,
) -> Result<(), BuildDslError> {
    if depth > MAX_DSL_DEPTH {
        return Err(BuildDslError::DepthExceeded(MAX_DSL_DEPTH));
    }
    match node {
        BuildNode::House(house) => {
            if state.houses >= MAX_DSL_HOUSES as u64 {
                return Err(BuildDslError::TooManyHouses {
                    requested: state.houses + 1,
                    limit: MAX_DSL_HOUSES,
                });
            }
            let origin = position(house.at, offset)?;
            let object = ParametricObject {
                id: "build-dsl-house".to_owned(),
                name: "Build DSL house".to_owned(),
                generator: GeneratorKind::House,
                generator_version: HOUSE_GENERATOR_VERSION,
                dimension: state.dimension.to_owned(),
                origin,
                rotation: house.rotation,
                params: house.params.clone(),
                layer_id: None,
                applied_revision: 0,
                last_operation_id: None,
                baseline: ParametricBaseline::default(),
            };
            let generated = object
                .generate()
                .map_err(|error| BuildDslError::InvalidParams(error.to_string()))?;
            for block in generated {
                if state.blocks.contains_key(&block.pos) {
                    return Err(BuildDslError::Overlap(block.pos));
                }
                if state.blocks.len() as u64 >= MAX_DSL_BLOCKS {
                    return Err(BuildDslError::TooManyBlocks {
                        requested: state.blocks.len() as u64 + 1,
                        limit: MAX_DSL_BLOCKS,
                    });
                }
                state.blocks.insert(block.pos, block.state);
                state.min = Some(match state.min {
                    Some(current) => BlockPos::new(
                        current.x.min(block.pos.x),
                        current.y.min(block.pos.y),
                        current.z.min(block.pos.z),
                    ),
                    None => block.pos,
                });
                state.max = Some(match state.max {
                    Some(current) => BlockPos::new(
                        current.x.max(block.pos.x),
                        current.y.max(block.pos.y),
                        current.z.max(block.pos.z),
                    ),
                    None => block.pos,
                });
            }
            state.houses += 1;
            Ok(())
        }
        BuildNode::Repeat(repeat) => {
            if repeat.count == 0 || repeat.count > MAX_REPEAT_COUNT {
                return Err(BuildDslError::InvalidRepeat(format!(
                    "count must be 1..={MAX_REPEAT_COUNT}"
                )));
            }
            if repeat.spacing == 0 || repeat.spacing > MAX_REPEAT_SPACING {
                return Err(BuildDslError::InvalidRepeat(format!(
                    "spacing must be 1..={MAX_REPEAT_SPACING}"
                )));
            }
            for index in 0..repeat.count {
                let distance = i64::from(index) * i64::from(repeat.spacing);
                let mut next = offset;
                match repeat.axis {
                    RepeatAxis::X => next[0] = add(next[0], distance)?,
                    RepeatAxis::Y => next[1] = add(next[1], distance)?,
                    RepeatAxis::Z => next[2] = add(next[2], distance)?,
                }
                expand(&repeat.child, next, depth + 1, state)?;
            }
            Ok(())
        }
    }
}

fn add(base: i64, distance: i64) -> Result<i64, BuildDslError> {
    base.checked_add(distance)
        .ok_or_else(|| BuildDslError::InvalidCoordinates("offset overflow".to_owned()))
}

fn position(at: [i32; 3], offset: [i64; 3]) -> Result<BlockPos, BuildDslError> {
    let x = i64::from(at[0]) + offset[0];
    let y = i64::from(at[1]) + offset[1];
    let z = i64::from(at[2]) + offset[2];
    let convert = |value: i64, axis: &str| {
        i32::try_from(value).map_err(|_| {
            BuildDslError::InvalidCoordinates(format!("{axis} coordinate {value} is out of range"))
        })
    };
    Ok(BlockPos::new(
        convert(x, "X")?,
        convert(y, "Y")?,
        convert(z, "Z")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(width: u32, depth: u32, height: u32) -> HouseParams {
        HouseParams {
            width,
            depth,
            height,
            wall: "minecraft:stone_bricks".to_owned(),
            floor: "minecraft:oak_planks".to_owned(),
            roof: "minecraft:dark_oak_planks".to_owned(),
            window: Some("minecraft:glass".to_owned()),
        }
    }

    fn house(at: [i32; 3], rotation: Rotation) -> BuildNode {
        BuildNode::House(HouseNode {
            at,
            rotation,
            params: params(5, 4, 3),
        })
    }

    fn recipe(nodes: Vec<BuildNode>) -> BuildRecipe {
        BuildRecipe {
            schema_version: BUILD_DSL_SCHEMA_VERSION,
            dimension: "minecraft:overworld".to_owned(),
            nodes,
        }
    }

    #[test]
    fn compiles_deterministically_to_one_batch_with_rotated_states() {
        let mut rotated = house([10, 64, -20], Rotation::Cw90);
        if let BuildNode::House(ref mut node) = rotated {
            node.params.wall = "minecraft:oak_stairs[facing=north]".to_owned();
            node.params.floor = "minecraft:oak_log[axis=x]".to_owned();
            node.params.window = None;
        }
        let recipe = recipe(vec![rotated]);
        let first = compile(&recipe).expect("first compile");
        let second = compile(&recipe).expect("second compile");
        assert_eq!(first, second);
        assert_eq!(first.summary().houses, 1);
        assert!(first.summary().blocks > 50);
        assert_eq!(first.summary().bounds.min, BlockPos::new(10, 64, -20));
        assert_eq!(first.summary().bounds.max, BlockPos::new(13, 68, -16));
        let operation = first.operation();
        let Operation::Batch { dimension, changes } = operation else {
            panic!("the DSL must compile to a batch");
        };
        assert_eq!(dimension, "minecraft:overworld");
        assert_eq!(changes.len() as u64, first.summary().blocks);
        let at = |pos: BlockPos| {
            changes
                .iter()
                .find(|change| change.pos == pos)
                .map(|change| change.state.to_string())
        };
        assert_eq!(
            at(BlockPos::new(13, 65, -20)).as_deref(),
            Some("minecraft:oak_stairs[facing=east]")
        );
        assert_eq!(
            at(BlockPos::new(13, 64, -20)).as_deref(),
            Some("minecraft:oak_log[axis=z]")
        );
    }

    #[test]
    fn repeat_expands_a_quarter_without_overlap() {
        let mut node = house([0, 64, 0], Rotation::None);
        if let BuildNode::House(ref mut house) = node {
            house.params = params(5, 4, 3);
        }
        let quarter = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::X,
            count: 3,
            spacing: 6,
            child: Box::new(node),
        });
        let compiled = compile(&recipe(vec![quarter])).expect("quarter");
        assert_eq!(compiled.summary().houses, 3);
        assert_eq!(compiled.summary().bounds.min, BlockPos::new(0, 64, 0));
        assert_eq!(compiled.summary().bounds.max, BlockPos::new(16, 68, 3));
    }

    #[test]
    fn rejects_overlap_bad_materials_and_empty_recipes() {
        let overlap = recipe(vec![
            house([0, 64, 0], Rotation::None),
            house([0, 64, 0], Rotation::None),
        ]);
        assert!(matches!(compile(&overlap), Err(BuildDslError::Overlap(_))));

        let mut bad = house([0, 64, 0], Rotation::None);
        if let BuildNode::House(ref mut node) = bad {
            node.params.wall = "not-a-block".to_owned();
        }
        assert!(matches!(
            compile(&recipe(vec![bad])),
            Err(BuildDslError::InvalidParams(_))
        ));
        assert_eq!(
            compile(&recipe(Vec::new())).unwrap_err(),
            BuildDslError::EmptyRecipe
        );
        let mut future = recipe(vec![house([0, 64, 0], Rotation::None)]);
        future.schema_version = 2;
        assert_eq!(compile(&future).unwrap_err().code(), "unsupported_version");
    }

    #[test]
    fn enforces_depth_house_and_block_budgets() {
        let deep = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::X,
            count: 2,
            spacing: 8,
            child: Box::new(BuildNode::Repeat(RepeatNode {
                axis: RepeatAxis::Z,
                count: 2,
                spacing: 8,
                child: Box::new(BuildNode::Repeat(RepeatNode {
                    axis: RepeatAxis::Y,
                    count: 2,
                    spacing: 8,
                    child: Box::new(BuildNode::Repeat(RepeatNode {
                        axis: RepeatAxis::X,
                        count: 2,
                        spacing: 8,
                        child: Box::new(house([0, 64, 0], Rotation::None)),
                    })),
                })),
            })),
        });
        assert!(matches!(
            compile(&recipe(vec![deep])),
            Err(BuildDslError::DepthExceeded(MAX_DSL_DEPTH))
        ));

        let inner = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::X,
            count: 16,
            spacing: 6,
            child: Box::new(house([0, 64, 0], Rotation::None)),
        });
        let outer = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::Z,
            count: 16,
            spacing: 5,
            child: Box::new(inner),
        });
        let result = compile(&recipe(vec![outer]));
        assert!(
            matches!(result, Err(BuildDslError::TooManyHouses { .. })),
            "{result:?}"
        );

        let mut nodes = Vec::new();
        for index in 0..MAX_DSL_NODES + 1 {
            nodes.push(house([index as i32 * 6, 64, 0], Rotation::None));
        }
        assert!(matches!(
            compile(&recipe(nodes)),
            Err(BuildDslError::TooManyNodes { .. })
        ));

        let mut large = house([0, 64, 0], Rotation::None);
        if let BuildNode::House(ref mut node) = large {
            node.params = params(32, 32, 32);
        }
        let mut nodes = Vec::new();
        for index in 0..MAX_DSL_HOUSES as i32 {
            let mut copy = large.clone();
            if let BuildNode::House(ref mut node) = copy {
                node.at = [index * 33, 64, 0];
            }
            nodes.push(copy);
        }
        assert!(matches!(
            compile(&recipe(nodes)),
            Err(BuildDslError::TooManyBlocks { .. })
        ));
    }

    #[test]
    fn parses_camel_case_recipe_and_denies_unknown_fields() {
        let json = r#"{
            "schemaVersion": 1,
            "dimension": "minecraft:overworld",
            "nodes": [
                { "type": "house", "at": [1, 64, 2], "rotation": "cw180",
                  "params": { "width": 5, "depth": 4, "height": 3,
                    "wall": "minecraft:stone_bricks", "floor": "minecraft:oak_planks",
                    "roof": "minecraft:dark_oak_planks" } }
            ]
        }"#;
        let parsed: BuildRecipe = serde_json::from_str(json).expect("recipe");
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.nodes.len(), 1);
        let compiled = compile(&parsed).expect("compile");
        assert_eq!(compiled.summary().bounds.min, BlockPos::new(1, 64, 2));

        let unknown = json.replace(
            "\"dimension\": \"minecraft:overworld\",",
            "\"dimension\": \"minecraft:overworld\", \"script\": \"alert(1)\",",
        );
        assert!(serde_json::from_str::<BuildRecipe>(&unknown).is_err());
        let bad_schema = json.replace("\"schemaVersion\": 1", "\"schemaVersion\": 9");
        assert!(matches!(
            compile(&serde_json::from_str::<BuildRecipe>(&bad_schema).expect("parsed")),
            Err(BuildDslError::UnsupportedSchema { .. })
        ));
    }

    #[test]
    fn repeat_limits_are_fail_closed() {
        let node = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::X,
            count: 0,
            spacing: 4,
            child: Box::new(house([0, 64, 0], Rotation::None)),
        });
        assert!(matches!(
            compile(&recipe(vec![node])),
            Err(BuildDslError::InvalidRepeat(_))
        ));
        let node = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::X,
            count: 2,
            spacing: 0,
            child: Box::new(house([0, 64, 0], Rotation::None)),
        });
        assert!(matches!(
            compile(&recipe(vec![node])),
            Err(BuildDslError::InvalidRepeat(_))
        ));
        let node = BuildNode::Repeat(RepeatNode {
            axis: RepeatAxis::X,
            count: MAX_REPEAT_COUNT + 1,
            spacing: 4,
            child: Box::new(house([0, 64, 0], Rotation::None)),
        });
        assert!(matches!(
            compile(&recipe(vec![node])),
            Err(BuildDslError::InvalidRepeat(_))
        ));
    }
}
