//! JSON Schemas advertised to MCP clients.
//!
//! The schemas are written explicitly rather than derived from Rust structs: the bridge
//! forwards operations to the authoritative Rust Core, and the advertised shape must stay
//! stable even when an internal Rust type changes. They mirror the validation performed by
//! `args.rs`.

use serde_json::{json, Value};

pub fn object(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let properties: serde_json::Map<String, Value> = properties
        .into_iter()
        .map(|(name, schema)| (name.to_owned(), schema))
        .collect();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

pub fn string(min: usize, max: usize) -> Value {
    json!({ "type": "string", "minLength": min, "maxLength": max })
}

pub fn description(mut schema: Value, text: &str) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.insert("description".to_owned(), Value::String(text.to_owned()));
    }
    schema
}

pub fn default_value(mut schema: Value, value: Value) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.insert("default".to_owned(), value);
    }
    schema
}

pub fn integer(min: i64, max: i64) -> Value {
    json!({ "type": "integer", "minimum": min, "maximum": max })
}

pub fn any_integer() -> Value {
    json!({ "type": "integer" })
}

pub fn non_negative_integer() -> Value {
    json!({ "type": "integer", "minimum": 0 })
}

pub fn number() -> Value {
    json!({ "type": "number" })
}

pub fn boolean() -> Value {
    json!({ "type": "boolean" })
}

pub fn array(items: Value, min: usize, max: usize) -> Value {
    json!({ "type": "array", "items": items, "minItems": min, "maxItems": max })
}

pub fn string_map() -> Value {
    json!({ "type": "object", "additionalProperties": { "type": "string" } })
}

pub fn nullable(schema: Value) -> Value {
    json!({ "anyOf": [schema, { "type": "null" }] })
}

pub fn enum_string(values: &[&str]) -> Value {
    json!({ "type": "string", "enum": values })
}

pub fn block_pos() -> Value {
    object(
        vec![
            ("x", integer(i64::MIN, i64::MAX)),
            ("y", integer(i64::MIN, i64::MAX)),
            ("z", integer(i64::MIN, i64::MAX)),
        ],
        &["x", "y", "z"],
    )
}

pub fn bounds() -> Value {
    object(
        vec![("min", block_pos()), ("max", block_pos())],
        &["min", "max"],
    )
}

pub fn block_state() -> Value {
    object(
        vec![
            ("name", string(1, 128)),
            ("properties", default_value(string_map(), json!({}))),
        ],
        &["name"],
    )
}

pub fn change() -> Value {
    object(
        vec![("pos", block_pos()), ("state", block_state())],
        &["pos", "state"],
    )
}

pub fn project_properties() -> Vec<(&'static str, Value)> {
    vec![
        (
            "project_id",
            description(
                string(1, 128),
                "Project identifier; never inferred from the MCP connection.",
            ),
        ),
        (
            "world_revision",
            description(
                non_negative_integer(),
                "Revision the caller observed. A mismatch is reported instead of silently reading newer data.",
            ),
        ),
    ]
}

pub fn write_project_properties() -> Vec<(&'static str, Value)> {
    vec![
        (
            "project_id",
            description(
                string(1, 128),
                "Project identifier; never inferred from the MCP connection.",
            ),
        ),
        (
            "world_revision",
            description(
                non_negative_integer(),
                "Revision the caller previewed; the write fails if the world moved on.",
            ),
        ),
    ]
}

pub fn idempotency_key() -> Value {
    description(
        string(1, 128),
        "Unique key for this intended write. Repeating it with the same payload replays the result instead of applying twice.",
    )
}

pub fn dimension() -> Value {
    description(
        string(1, 128),
        "Dimension id, for example minecraft:overworld.",
    )
}

pub fn operation() -> Value {
    let state = block_state();
    let position = block_pos();
    let region = bounds();
    let block_change = change();
    let dimension = dimension();
    let seed = default_value(non_negative_integer(), json!(0));
    json!({
        "oneOf": [
            object(vec![
                ("type", json!({ "const": "set_block" })),
                ("dimension", dimension.clone()),
                ("pos", position.clone()),
                ("state", state.clone()),
            ], &["type", "dimension", "pos", "state"]),
            object(vec![
                ("type", json!({ "const": "batch" })),
                ("dimension", dimension.clone()),
                ("changes", array(block_change.clone(), 1, 8192)),
            ], &["type", "dimension", "changes"]),
            object(vec![
                ("type", json!({ "const": "fill_cuboid" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("state", state.clone()),
            ], &["type", "dimension", "bounds", "state"]),
            object(vec![
                ("type", json!({ "const": "replace" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("from", state.clone()),
                ("to", state.clone()),
            ], &["type", "dimension", "bounds", "from", "to"]),
            object(vec![
                ("type", json!({ "const": "terrain_brush" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("kind", enum_string(&["raise", "lower", "smooth", "flatten", "noise", "erode", "terrace"])),
                ("material", state.clone()),
                ("amount", default_value(integer(1, 16), json!(1))),
                ("seed", seed.clone()),
                ("step", default_value(integer(2, 16), json!(4))),
                ("targetY", any_integer()),
            ], &["type", "dimension", "bounds", "kind", "material"]),
            object(vec![
                ("type", json!({ "const": "generate_heightmap" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("seed", seed.clone()),
                ("scale", default_value(integer(1, 64), json!(8))),
                ("octaves", default_value(integer(1, 6), json!(3))),
                ("amplitude", default_value(integer(1, 256), json!(8))),
                ("baseY", any_integer()),
                ("surfaceDepth", default_value(integer(1, 32), json!(1))),
                ("surface", state.clone()),
                ("filler", state.clone()),
            ], &["type", "dimension", "bounds", "baseY", "surface", "filler"]),
            object(vec![
                ("type", json!({ "const": "generate_road" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("from", position.clone()),
                ("to", position.clone()),
                ("width", default_value(integer(1, 16), json!(3))),
                ("meander", default_value(integer(0, 64), json!(4))),
                ("seed", seed.clone()),
                ("clearance", default_value(integer(0, 8), json!(2))),
                ("surface", state.clone()),
            ], &["type", "dimension", "bounds", "from", "to", "surface"]),
            object(vec![
                ("type", json!({ "const": "generate_river" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("from", position.clone()),
                ("to", position.clone()),
                ("width", default_value(integer(1, 16), json!(3))),
                ("meander", default_value(integer(0, 64), json!(4))),
                ("seed", seed.clone()),
                ("depth", default_value(integer(1, 16), json!(3))),
                ("water", state.clone()),
                ("bed", state.clone()),
            ], &["type", "dimension", "bounds", "from", "to", "water", "bed"]),
            object(vec![
                ("type", json!({ "const": "generate_vegetation" })),
                ("dimension", dimension.clone()),
                ("bounds", region.clone()),
                ("seed", seed.clone()),
                ("groundCover", default_value(integer(0, 100), json!(35))),
                ("treeChance", default_value(integer(0, 100), json!(2))),
                ("treeSpacing", default_value(integer(1, 32), json!(5))),
                ("plant", state.clone()),
                ("trunk", state.clone()),
                ("leaves", state.clone()),
            ], &["type", "dimension", "bounds", "plant", "trunk", "leaves"]),
            object(vec![
                ("type", json!({ "const": "set_biome" })),
                ("dimension", dimension),
                ("bounds", region),
                ("biome", description(
                    string(1, 128),
                    "Namespaced lowercase biome id. The core refuses worlds without source biome data and never invents a default.",
                )),
            ], &["type", "dimension", "bounds", "biome"]),
        ]
    })
}

pub fn build_recipe() -> Value {
    let dimension = string(1, 128);
    let block = string(1, 128);
    let house_params = object(
        vec![
            ("width", integer(3, 32)),
            ("depth", integer(3, 32)),
            ("height", integer(2, 32)),
            ("wall", block.clone()),
            ("floor", block.clone()),
            ("roof", block.clone()),
            ("window", nullable(block.clone())),
        ],
        &["width", "depth", "height", "wall", "floor", "roof"],
    );
    let house = object(
        vec![
            ("type", json!({ "const": "house" })),
            (
                "at",
                json!({
                    "type": "array",
                    "items": { "type": "integer" },
                    "minItems": 3,
                    "maxItems": 3
                }),
            ),
            (
                "rotation",
                default_value(
                    enum_string(&["none", "cw90", "cw180", "cw270"]),
                    json!("none"),
                ),
            ),
            ("params", house_params),
        ],
        &["type", "at", "params"],
    );
    let repeat = object(
        vec![
            ("type", json!({ "const": "repeat" })),
            ("axis", enum_string(&["x", "y", "z"])),
            ("count", integer(1, 16)),
            ("spacing", integer(1, 64)),
            ("child", json!({ "$ref": "#/$defs/buildNode" })),
        ],
        &["type", "axis", "count", "spacing", "child"],
    );
    json!({
        "$defs": {
            "buildNode": {
                "oneOf": [house, repeat]
            }
        },
        "type": "object",
        "properties": {
            "schemaVersion": default_value(json!({ "const": 1 }), json!(1)),
            "dimension": dimension,
            "nodes": array(json!({ "$ref": "#/$defs/buildNode" }), 1, 64),
        },
        "required": ["dimension", "nodes"],
    })
}

pub fn path_spec() -> Value {
    object(
        vec![
            (
                "from",
                object(
                    vec![("x", any_integer()), ("z", any_integer())],
                    &["x", "z"],
                ),
            ),
            (
                "to",
                object(
                    vec![("x", any_integer()), ("z", any_integer())],
                    &["x", "z"],
                ),
            ),
            ("base_y", any_integer()),
            ("width", default_value(integer(1, 16), json!(1))),
            ("material", block_state()),
        ],
        &["from", "to", "base_y", "material"],
    )
}

pub fn validation_rules() -> Value {
    enum_string(&[
        "floating_blocks",
        "out_of_bounds",
        "path_continuity",
        "palette",
        "export_compatibility",
        "block_state_orientation",
        "protected_region",
    ])
}

pub fn observation_mode() -> Value {
    enum_string(&[
        "top_down",
        "heightmap",
        "material_map",
        "biome_map",
        "cross_section",
        "revision_diff",
    ])
}

pub fn read_only_annotations() -> rmcp::model::ToolAnnotations {
    rmcp::model::ToolAnnotations::from_raw(None, Some(true), Some(false), Some(true), Some(false))
}

pub fn write_annotations() -> rmcp::model::ToolAnnotations {
    rmcp::model::ToolAnnotations::from_raw(None, Some(false), Some(true), Some(false), Some(false))
}

pub fn non_destructive_write_annotations() -> rmcp::model::ToolAnnotations {
    rmcp::model::ToolAnnotations::from_raw(None, Some(false), Some(false), Some(false), Some(false))
}

pub fn job_query_annotations() -> rmcp::model::ToolAnnotations {
    rmcp::model::ToolAnnotations::from_raw(None, Some(true), Some(false), Some(true), Some(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_schema_names_every_supported_generator() {
        let schema = serde_json::to_string(&operation()).expect("schema json");
        for name in [
            "set_block",
            "batch",
            "fill_cuboid",
            "replace",
            "terrain_brush",
            "generate_heightmap",
            "generate_road",
            "generate_river",
            "generate_vegetation",
            "set_biome",
        ] {
            assert!(schema.contains(name), "missing {name}");
        }
    }

    #[test]
    fn project_schemas_require_project_id() {
        let schema = object(project_properties(), &["project_id"]);
        assert_eq!(schema["required"], json!(["project_id"]));
        assert_eq!(schema["properties"]["project_id"]["minLength"], 1);
    }
}
