//! Validation and normalization of MCP tool arguments.
//!
//! The bridge validates before it contacts the desktop so malformed requests never reach
//! the authoritative Core, and so defaults are applied exactly once in a documented place.
//! Every error is a short caller-facing string; it never contains a token or local path.

use serde_json::{json, Map, Value};

pub const MAX_BATCH_CHANGES: usize = 8_192;
const MAX_JS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const MAX_RECIPE_DEPTH: usize = 32;

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object, got {}", type_name(value)))
}

fn array<'a>(value: &'a Value, label: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{label} must be an array, got {}", type_name(value)))
}

fn string_in(value: &Value, label: &str, min: usize, max: usize) -> Result<String, String> {
    let string = value
        .as_str()
        .ok_or_else(|| format!("{label} must be a string"))?;
    if string.len() < min || string.len() > max {
        return Err(format!(
            "{label} must be between {min} and {max} characters"
        ));
    }
    Ok(string.to_owned())
}

fn integer_in(value: &Value, label: &str, min: i64, max: i64) -> Result<i64, String> {
    let integer = value
        .as_i64()
        .ok_or_else(|| format!("{label} must be an integer"))?;
    if integer < min || integer > max {
        return Err(format!("{label} must be between {min} and {max}"));
    }
    Ok(integer)
}

fn non_negative(value: &Value, label: &str, max: u64) -> Result<u64, String> {
    let number = value
        .as_u64()
        .ok_or_else(|| format!("{label} must be a non-negative integer"))?;
    if number > max {
        return Err(format!("{label} must be between 0 and {max}"));
    }
    Ok(number)
}

fn required<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a Value, String> {
    map.get(key).ok_or_else(|| format!("{key} is required"))
}

pub fn required_string(
    map: &Map<String, Value>,
    key: &str,
    min: usize,
    max: usize,
) -> Result<String, String> {
    string_in(required(map, key)?, key, min, max)
}

pub fn optional_string(
    map: &Map<String, Value>,
    key: &str,
    min: usize,
    max: usize,
) -> Result<Option<String>, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => string_in(value, key, min, max).map(Some),
    }
}

pub fn required_object<'a>(
    map: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Map<String, Value>, String> {
    object(required(map, key)?, key)
}

pub fn required_array<'a>(
    map: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Vec<Value>, String> {
    array(required(map, key)?, key)
}

pub fn optional_revision(map: &Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => non_negative(value, key, MAX_JS_SAFE_INTEGER).map(Some),
    }
}

pub fn required_revision(map: &Map<String, Value>, key: &str) -> Result<u64, String> {
    optional_revision(map, key)?
        .ok_or_else(|| format!("{key} is required for a write; preview the operation first"))
}

pub fn optional_u64(
    map: &Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
    default: u64,
) -> Result<u64, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => {
            let number = value
                .as_u64()
                .ok_or_else(|| format!("{key} must be a non-negative integer"))?;
            if number < min || number > max {
                return Err(format!("{key} must be between {min} and {max}"));
            }
            Ok(number)
        }
    }
}

pub fn required_u64(
    map: &Map<String, Value>,
    key: &str,
    min: u64,
    max: u64,
) -> Result<u64, String> {
    let value = required(map, key)?;
    let number = value
        .as_u64()
        .ok_or_else(|| format!("{key} must be a non-negative integer"))?;
    if number < min || number > max {
        return Err(format!("{key} must be between {min} and {max}"));
    }
    Ok(number)
}

pub fn optional_i64(
    map: &Map<String, Value>,
    key: &str,
    min: i64,
    max: i64,
) -> Result<Option<i64>, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => integer_in(value, key, min, max).map(Some),
    }
}

pub fn required_i64(
    map: &Map<String, Value>,
    key: &str,
    min: i64,
    max: i64,
) -> Result<i64, String> {
    integer_in(required(map, key)?, key, min, max)
}

pub fn optional_bool(map: &Map<String, Value>, key: &str, default: bool) -> Result<bool, String> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| format!("{key} must be a boolean")),
    }
}

pub fn block_pos(value: &Value, label: &str) -> Result<Value, String> {
    let map = object(value, label)?;
    Ok(json!({
        "x": required_i64(map, "x", i64::MIN, i64::MAX)?,
        "y": required_i64(map, "y", i64::MIN, i64::MAX)?,
        "z": required_i64(map, "z", i64::MIN, i64::MAX)?,
    }))
}

pub fn bounds(value: &Value, label: &str) -> Result<Value, String> {
    let map = object(value, label)?;
    Ok(json!({
        "min": block_pos(required(map, "min")?, &format!("{label}.min"))?,
        "max": block_pos(required(map, "max")?, &format!("{label}.max"))?,
    }))
}

pub fn block_state(value: &Value, label: &str) -> Result<Value, String> {
    let map = object(value, label)?;
    let name = string_in(required(map, "name")?, &format!("{label}.name"), 1, 128)?;
    let properties = match map.get("properties") {
        None | Some(Value::Null) => Map::new(),
        Some(value) => {
            let properties = object(value, &format!("{label}.properties"))?;
            let mut normalized = Map::new();
            for (key, value) in properties {
                let value = value
                    .as_str()
                    .ok_or_else(|| format!("{label}.properties.{key} must be a string"))?;
                normalized.insert(key.clone(), Value::String(value.to_owned()));
            }
            normalized
        }
    };
    Ok(json!({ "name": name, "properties": properties }))
}

fn state_field(map: &Map<String, Value>, key: &str) -> Result<Value, String> {
    block_state(required(map, key)?, key)
}

fn string_field(map: &Map<String, Value>, key: &str) -> Result<String, String> {
    required_string(map, key, 1, 128)
}

fn set_default_i64(
    map: &mut Map<String, Value>,
    key: &str,
    min: i64,
    max: i64,
    default: i64,
) -> Result<(), String> {
    let value = match map.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => integer_in(value, key, min, max)?,
    };
    map.insert(key.to_owned(), json!(value));
    Ok(())
}

fn set_default_u64(
    map: &mut Map<String, Value>,
    key: &str,
    max: u64,
    default: u64,
) -> Result<(), String> {
    let value = match map.get(key) {
        None | Some(Value::Null) => default,
        Some(value) => non_negative(value, key, max)?,
    };
    map.insert(key.to_owned(), json!(value));
    Ok(())
}

fn set_enum(
    map: &mut Map<String, Value>,
    key: &str,
    values: &[&str],
    default: Option<&str>,
) -> Result<(), String> {
    let value = match map.get(key) {
        None | Some(Value::Null) => default
            .map(str::to_owned)
            .ok_or_else(|| format!("{key} is required"))?,
        Some(value) => value
            .as_str()
            .ok_or_else(|| format!("{key} must be a string"))?
            .to_owned(),
    };
    if !values.contains(&value.as_str()) {
        return Err(format!("{key} must be one of {}", values.join(", ")));
    }
    map.insert(key.to_owned(), Value::String(value));
    Ok(())
}

/// Validates a typed operation and fills the documented defaults. The returned object is
/// the exact operation sent to the desktop.
pub fn normalize_operation(value: &Value) -> Result<Value, String> {
    let operation = object(value, "operation")?;
    let kind = operation
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "operation.type is required".to_owned())?
        .to_owned();
    let dimension = string_field(operation, "dimension")?;
    let mut normalized = Map::new();
    normalized.insert("type".to_owned(), Value::String(kind.clone()));
    normalized.insert("dimension".to_owned(), Value::String(dimension));

    match kind.as_str() {
        "set_block" => {
            normalized.insert(
                "pos".to_owned(),
                block_pos(required(operation, "pos")?, "pos")?,
            );
            normalized.insert("state".to_owned(), state_field(operation, "state")?);
        }
        "batch" => {
            let changes = required_array(operation, "changes")?;
            if changes.is_empty() || changes.len() > MAX_BATCH_CHANGES {
                return Err(format!(
                    "changes must contain between 1 and {MAX_BATCH_CHANGES} entries"
                ));
            }
            let normalized_changes = changes
                .iter()
                .enumerate()
                .map(|(index, change)| {
                    let change = object(change, &format!("changes[{index}]"))?;
                    Ok(json!({
                        "pos": block_pos(required(change, "pos")?, &format!("changes[{index}].pos"))?,
                        "state": state_field(change, "state")?,
                    }))
                })
                .collect::<Result<Vec<Value>, String>>()?;
            normalized.insert("changes".to_owned(), Value::Array(normalized_changes));
        }
        "fill_cuboid" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            normalized.insert("state".to_owned(), state_field(operation, "state")?);
        }
        "replace" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            normalized.insert("from".to_owned(), state_field(operation, "from")?);
            normalized.insert("to".to_owned(), state_field(operation, "to")?);
        }
        "terrain_brush" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            let mut options = operation.clone();
            set_enum(
                &mut options,
                "kind",
                &[
                    "raise", "lower", "smooth", "flatten", "noise", "erode", "terrace",
                ],
                None,
            )?;
            set_default_i64(&mut options, "amount", 1, 16, 1)?;
            set_default_u64(&mut options, "seed", MAX_JS_SAFE_INTEGER, 0)?;
            set_default_i64(&mut options, "step", 2, 16, 4)?;
            normalized.insert("kind".to_owned(), options["kind"].clone());
            normalized.insert("material".to_owned(), state_field(operation, "material")?);
            normalized.insert("amount".to_owned(), options["amount"].clone());
            normalized.insert("seed".to_owned(), options["seed"].clone());
            normalized.insert("step".to_owned(), options["step"].clone());
            if let Some(target_y) = optional_i64(operation, "targetY", i64::MIN, i64::MAX)? {
                normalized.insert("targetY".to_owned(), json!(target_y));
            }
        }
        "generate_heightmap" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            let mut options = operation.clone();
            set_default_u64(&mut options, "seed", MAX_JS_SAFE_INTEGER, 0)?;
            set_default_i64(&mut options, "scale", 1, 64, 8)?;
            set_default_i64(&mut options, "octaves", 1, 6, 3)?;
            set_default_i64(&mut options, "amplitude", 1, 256, 8)?;
            set_default_i64(&mut options, "surfaceDepth", 1, 32, 1)?;
            normalized.insert("seed".to_owned(), options["seed"].clone());
            normalized.insert("scale".to_owned(), options["scale"].clone());
            normalized.insert("octaves".to_owned(), options["octaves"].clone());
            normalized.insert("amplitude".to_owned(), options["amplitude"].clone());
            normalized.insert("surfaceDepth".to_owned(), options["surfaceDepth"].clone());
            normalized.insert(
                "baseY".to_owned(),
                json!(required_i64(operation, "baseY", i64::MIN, i64::MAX)?),
            );
            normalized.insert("surface".to_owned(), state_field(operation, "surface")?);
            normalized.insert("filler".to_owned(), state_field(operation, "filler")?);
        }
        "generate_road" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            normalized.insert(
                "from".to_owned(),
                block_pos(required(operation, "from")?, "from")?,
            );
            normalized.insert(
                "to".to_owned(),
                block_pos(required(operation, "to")?, "to")?,
            );
            let mut options = operation.clone();
            set_default_i64(&mut options, "width", 1, 16, 3)?;
            set_default_i64(&mut options, "meander", 0, 64, 4)?;
            set_default_u64(&mut options, "seed", MAX_JS_SAFE_INTEGER, 0)?;
            set_default_i64(&mut options, "clearance", 0, 8, 2)?;
            for key in ["width", "meander", "seed", "clearance"] {
                normalized.insert(key.to_owned(), options[key].clone());
            }
            normalized.insert("surface".to_owned(), state_field(operation, "surface")?);
        }
        "generate_river" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            normalized.insert(
                "from".to_owned(),
                block_pos(required(operation, "from")?, "from")?,
            );
            normalized.insert(
                "to".to_owned(),
                block_pos(required(operation, "to")?, "to")?,
            );
            let mut options = operation.clone();
            set_default_i64(&mut options, "width", 1, 16, 3)?;
            set_default_i64(&mut options, "meander", 0, 64, 4)?;
            set_default_u64(&mut options, "seed", MAX_JS_SAFE_INTEGER, 0)?;
            set_default_i64(&mut options, "depth", 1, 16, 3)?;
            for key in ["width", "meander", "seed", "depth"] {
                normalized.insert(key.to_owned(), options[key].clone());
            }
            normalized.insert("water".to_owned(), state_field(operation, "water")?);
            normalized.insert("bed".to_owned(), state_field(operation, "bed")?);
        }
        "generate_vegetation" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            let mut options = operation.clone();
            set_default_u64(&mut options, "seed", MAX_JS_SAFE_INTEGER, 0)?;
            set_default_i64(&mut options, "groundCover", 0, 100, 35)?;
            set_default_i64(&mut options, "treeChance", 0, 100, 2)?;
            set_default_i64(&mut options, "treeSpacing", 1, 32, 5)?;
            for key in ["seed", "groundCover", "treeChance", "treeSpacing"] {
                normalized.insert(key.to_owned(), options[key].clone());
            }
            normalized.insert("plant".to_owned(), state_field(operation, "plant")?);
            normalized.insert("trunk".to_owned(), state_field(operation, "trunk")?);
            normalized.insert("leaves".to_owned(), state_field(operation, "leaves")?);
        }
        "set_biome" => {
            normalized.insert(
                "bounds".to_owned(),
                bounds(required(operation, "bounds")?, "bounds")?,
            );
            normalized.insert(
                "biome".to_owned(),
                Value::String(required_string(operation, "biome", 1, 128)?),
            );
        }
        other => {
            return Err(format!(
                "operation.type `{other}` is not supported by this bridge"
            ));
        }
    }
    Ok(Value::Object(normalized))
}

fn normalize_build_node(value: &Value, depth: usize) -> Result<Value, String> {
    if depth > MAX_RECIPE_DEPTH {
        return Err("build recipe nodes are nested too deeply".to_owned());
    }
    let node = object(value, "build node")?;
    let kind = node
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "build node type is required".to_owned())?;
    match kind {
        "house" => {
            let at = array(required(node, "at")?, "house.at")?;
            if at.len() != 3 {
                return Err("house.at must contain exactly 3 integers".to_owned());
            }
            let at = at
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    integer_in(value, &format!("house.at[{index}]"), i64::MIN, i64::MAX)
                        .map(Value::from)
                })
                .collect::<Result<Vec<Value>, String>>()?;
            let params = object(required(node, "params")?, "house.params")?;
            let width = required_i64(params, "width", 3, 32)?;
            let depth_value = required_i64(params, "depth", 3, 32)?;
            let height = required_i64(params, "height", 2, 32)?;
            let wall = required_string(params, "wall", 1, 128)?;
            let floor = required_string(params, "floor", 1, 128)?;
            let roof = required_string(params, "roof", 1, 128)?;
            let window = optional_string(params, "window", 1, 128)?;
            let rotation = node
                .get("rotation")
                .and_then(Value::as_str)
                .unwrap_or("none");
            if !["none", "cw90", "cw180", "cw270"].contains(&rotation) {
                return Err("house.rotation must be one of none, cw90, cw180, cw270".to_owned());
            }
            let mut normalized_params = Map::new();
            normalized_params.insert("width".to_owned(), json!(width));
            normalized_params.insert("depth".to_owned(), json!(depth_value));
            normalized_params.insert("height".to_owned(), json!(height));
            normalized_params.insert("wall".to_owned(), Value::String(wall));
            normalized_params.insert("floor".to_owned(), Value::String(floor));
            normalized_params.insert("roof".to_owned(), Value::String(roof));
            if let Some(window) = window {
                normalized_params.insert("window".to_owned(), Value::String(window));
            } else if node
                .get("params")
                .and_then(Value::as_object)
                .is_some_and(|params| params.get("window") == Some(&Value::Null))
            {
                normalized_params.insert("window".to_owned(), Value::Null);
            }
            Ok(json!({
                "type": "house",
                "at": at,
                "rotation": rotation,
                "params": normalized_params,
            }))
        }
        "repeat" => {
            let axis = required_string(node, "axis", 1, 8)?;
            if !["x", "y", "z"].contains(&axis.as_str()) {
                return Err("repeat.axis must be one of x, y, z".to_owned());
            }
            Ok(json!({
                "type": "repeat",
                "axis": axis,
                "count": required_i64(node, "count", 1, 16)?,
                "spacing": required_i64(node, "spacing", 1, 64)?,
                "child": normalize_build_node(required(node, "child")?, depth + 1)?,
            }))
        }
        other => Err(format!("build node type `{other}` is not supported")),
    }
}

pub fn normalize_build_recipe(value: &Value) -> Result<Value, String> {
    let recipe = object(value, "recipe")?;
    let schema_version = match recipe.get("schemaVersion") {
        None | Some(Value::Null) => 1,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| "recipe.schemaVersion must be 1".to_owned())?,
    };
    if schema_version != 1 {
        return Err("recipe.schemaVersion must be 1".to_owned());
    }
    let dimension = required_string(recipe, "dimension", 1, 128)?;
    let nodes = required_array(recipe, "nodes")?;
    if nodes.is_empty() || nodes.len() > 64 {
        return Err("recipe.nodes must contain between 1 and 64 nodes".to_owned());
    }
    let nodes = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            normalize_build_node(node, 0).map_err(|error| format!("recipe.nodes[{index}]: {error}"))
        })
        .collect::<Result<Vec<Value>, String>>()?;
    Ok(json!({
        "schemaVersion": 1,
        "dimension": dimension,
        "nodes": nodes,
    }))
}

pub fn normalize_validation_request(args: &Map<String, Value>) -> Result<Value, String> {
    let dimension = required_string(args, "dimension", 1, 128)?;
    let region = bounds(required(args, "bounds")?, "bounds")?;
    let mut request = Map::new();
    request.insert("dimension".to_owned(), Value::String(dimension));
    request.insert("bounds".to_owned(), region);
    if let Some(value) = args.get("rules") {
        let rules = array(value, "rules")?;
        if rules.is_empty() || rules.len() > 7 {
            return Err("rules must contain between 1 and 7 entries".to_owned());
        }
        let allowed = [
            "floating_blocks",
            "out_of_bounds",
            "path_continuity",
            "palette",
            "export_compatibility",
            "block_state_orientation",
            "protected_region",
        ];
        let mut normalized = Vec::with_capacity(rules.len());
        for rule in rules {
            let rule = rule
                .as_str()
                .ok_or_else(|| "rules entries must be strings".to_owned())?;
            if !allowed.contains(&rule) {
                return Err(format!("rule `{rule}` is not supported"));
            }
            normalized.push(Value::String(rule.to_owned()));
        }
        request.insert("rules".to_owned(), Value::Array(normalized));
    }
    if let Some(value) = args.get("allowed_bounds") {
        request.insert("allowedBounds".to_owned(), bounds(value, "allowed_bounds")?);
    }
    if let Some(value) = args.get("path") {
        let path = object(value, "path")?;
        let from = object(required(path, "from")?, "path.from")?;
        let to = object(required(path, "to")?, "path.to")?;
        request.insert(
            "path".to_owned(),
            json!({
                "from": {
                    "x": required_i64(from, "x", i64::MIN, i64::MAX)?,
                    "z": required_i64(from, "z", i64::MIN, i64::MAX)?,
                },
                "to": {
                    "x": required_i64(to, "x", i64::MIN, i64::MAX)?,
                    "z": required_i64(to, "z", i64::MIN, i64::MAX)?,
                },
                "baseY": required_i64(path, "base_y", i64::MIN, i64::MAX)?,
                "width": optional_u64(path, "width", 1, 16, 1)?,
                "material": state_field(path, "material")?,
            }),
        );
    }
    if let Some(value) = args.get("max_surface_materials") {
        request.insert(
            "maxSurfaceMaterials".to_owned(),
            json!(integer_in(value, "max_surface_materials", 1, 64)?),
        );
    }
    if let Some(value) = args.get("floating_fix") {
        let mode = value
            .as_str()
            .ok_or_else(|| "floating_fix must be a string".to_owned())?;
        if !["none", "remove", "support"].contains(&mode) {
            return Err("floating_fix must be one of none, remove, support".to_owned());
        }
        request.insert("floatingFix".to_owned(), Value::String(mode.to_owned()));
    }
    Ok(Value::Object(request))
}

pub fn normalize_observation(args: &Map<String, Value>) -> Result<Value, String> {
    let dimension = required_string(args, "dimension", 1, 128)?;
    let region = bounds(required(args, "bounds")?, "bounds")?;
    let mode = required_string(args, "mode", 1, 32)?;
    if ![
        "top_down",
        "heightmap",
        "material_map",
        "biome_map",
        "cross_section",
        "revision_diff",
    ]
    .contains(&mode.as_str())
    {
        return Err(
            "mode must be one of top_down, heightmap, material_map, biome_map, cross_section, revision_diff"
                .to_owned(),
        );
    }
    let mut observation = Map::new();
    observation.insert("dimension".to_owned(), Value::String(dimension));
    observation.insert("bounds".to_owned(), region);
    observation.insert("mode".to_owned(), Value::String(mode));
    if let Some(value) = args.get("slice") {
        let slice = object(value, "slice")?;
        let axis = required_string(slice, "axis", 1, 1)?;
        if !["x", "y", "z"].contains(&axis.as_str()) {
            return Err("slice.axis must be one of x, y, z".to_owned());
        }
        observation.insert(
            "slice".to_owned(),
            json!({
                "axis": axis,
                "coordinate": required_i64(slice, "coordinate", i64::MIN, i64::MAX)?,
            }),
        );
    }
    if let Some(revision) = optional_revision(args, "from_revision")? {
        observation.insert("fromRevision".to_owned(), json!(revision));
    }
    Ok(Value::Object(observation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_a_set_block_and_defaults_properties() {
        let operation = normalize_operation(&json!({
            "type": "set_block",
            "dimension": "minecraft:overworld",
            "pos": { "x": 1, "y": 64, "z": 3 },
            "state": { "name": "minecraft:stone" }
        }))
        .expect("operation");
        assert_eq!(operation["state"]["properties"], json!({}));
        assert_eq!(operation["pos"]["y"], 64);
    }

    #[test]
    fn applies_generator_defaults() {
        let operation = normalize_operation(&json!({
            "type": "generate_road",
            "dimension": "minecraft:overworld",
            "bounds": {
                "min": { "x": 0, "y": 60, "z": 0 },
                "max": { "x": 15, "y": 80, "z": 15 }
            },
            "from": { "x": 0, "y": 64, "z": 7 },
            "to": { "x": 15, "y": 64, "z": 7 },
            "surface": { "name": "minecraft:stone_bricks" }
        }))
        .expect("road");
        assert_eq!(operation["width"], 3);
        assert_eq!(operation["meander"], 4);
        assert_eq!(operation["seed"], 0);
        assert_eq!(operation["clearance"], 2);
    }

    #[test]
    fn rejects_batches_over_the_limit_and_unknown_operations() {
        let changes: Vec<Value> = (0..8_193)
            .map(|index| {
                json!({
                    "pos": { "x": index, "y": 64, "z": 0 },
                    "state": { "name": "minecraft:stone" }
                })
            })
            .collect();
        let error = normalize_operation(&json!({
            "type": "batch",
            "dimension": "minecraft:overworld",
            "changes": changes
        }))
        .expect_err("too large");
        assert!(error.contains("8192"));

        assert!(normalize_operation(&json!({
            "type": "nope",
            "dimension": "minecraft:overworld"
        }))
        .is_err());
    }

    #[test]
    fn normalizes_build_recipes_and_defaults() {
        let recipe = normalize_build_recipe(&json!({
            "dimension": "minecraft:overworld",
            "nodes": [{
                "type": "house",
                "at": [0, 64, 0],
                "params": {
                    "width": 5,
                    "depth": 5,
                    "height": 4,
                    "wall": "minecraft:stone_bricks",
                    "floor": "minecraft:oak_planks",
                    "roof": "minecraft:oak_planks"
                }
            }]
        }))
        .expect("recipe");
        assert_eq!(recipe["schemaVersion"], 1);
        assert_eq!(recipe["nodes"][0]["rotation"], "none");
        assert_eq!(recipe["nodes"][0]["params"]["window"], Value::Null);
    }

    #[test]
    fn validation_and_observation_requests_are_canonicalized() {
        let request = normalize_validation_request(
            json!({
                "dimension": "minecraft:overworld",
                "bounds": {
                    "min": { "x": 0, "y": 0, "z": 0 },
                    "max": { "x": 1, "y": 1, "z": 1 }
                },
                "path": {
                    "from": { "x": 0, "z": 0 },
                    "to": { "x": 5, "z": 5 },
                    "base_y": 64,
                    "material": { "name": "minecraft:stone" }
                }
            })
            .as_object()
            .expect("object"),
        )
        .expect("validation");
        assert_eq!(request["path"]["baseY"], 64);
        assert_eq!(request["path"]["width"], 1);

        let observation = normalize_observation(
            json!({
                "dimension": "minecraft:overworld",
                "bounds": {
                    "min": { "x": 0, "y": 0, "z": 0 },
                    "max": { "x": 1, "y": 1, "z": 1 }
                },
                "mode": "revision_diff",
                "from_revision": 3
            })
            .as_object()
            .expect("object"),
        )
        .expect("observation");
        assert_eq!(observation["fromRevision"], 3);
    }
}
