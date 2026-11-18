use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Map, Value};

use crate::bundle::stack::ResourceStack;
use crate::bundle::{
    normalize_id, BlockStateRequest, BundleBlock, BundleElement, BundleElementRotation,
    BundleError, BundleFace, BundlePart,
};

const MAX_MODEL_DEPTH: usize = 8;
const MAX_MODELS_PER_STATE: usize = 64;
const MAX_ELEMENTS: usize = 64;
const MAX_TEXTURES: usize = 64;
const MAX_CONDITION_DEPTH: usize = 8;
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

pub(crate) struct ResolvedBlock {
    pub block: BundleBlock,
    pub textures: BTreeSet<String>,
}

#[derive(Clone)]
pub(crate) struct CachedModel {
    textures: BTreeMap<String, String>,
    elements: Option<Value>,
    warnings: Vec<String>,
    unsupported: Vec<String>,
}

struct ModelRef {
    id: String,
    rotation_x: i32,
    rotation_y: i32,
    uvlock: bool,
}

pub(crate) fn resolve_block(
    stack: &mut ResourceStack,
    request: &BlockStateRequest,
    cache: &mut HashMap<String, Option<CachedModel>>,
) -> Result<ResolvedBlock, BundleError> {
    let name = normalize_id(&request.name).ok_or_else(|| {
        BundleError::InvalidArgument(format!("invalid block state name: {}", request.name))
    })?;
    let (namespace, path) = name.split_once(':').expect("validated id");
    let blockstate_path = format!("assets/{namespace}/blockstates/{path}.json");
    let mut warnings = Vec::new();
    let mut unsupported = Vec::new();
    let mut textures: BTreeSet<String> = BTreeSet::new();
    if let Some(fluid) = fluid_texture(&name) {
        textures.insert(fluid.to_owned());
    }
    let document = stack.read_json(&blockstate_path)?;
    let Some(document) = document else {
        return Ok(missing_block(
            name,
            request,
            format!("no blockstate file at {blockstate_path}"),
        ));
    };
    if !document.is_object() {
        return Ok(missing_block(
            name,
            request,
            format!("{blockstate_path} is not a JSON object"),
        ));
    }

    let mut refs: Vec<ModelRef> = Vec::new();
    if let Some(variants) = document.get("variants").and_then(Value::as_object) {
        match select_variant(variants, &request.properties, &mut warnings) {
            Some(value) => refs.extend(parse_model_refs(value, &mut warnings)),
            None => warnings.push(format!(
                "no blockstate variant matches {}",
                describe_properties(&request.properties)
            )),
        }
    } else if let Some(parts) = document.get("multipart").and_then(Value::as_array) {
        for part in parts.iter().take(MAX_MODELS_PER_STATE) {
            let matches = match part.get("when") {
                Some(condition) => condition_matches(condition, &request.properties, 0)
                    .unwrap_or_else(|error| {
                        warnings.push(error);
                        false
                    }),
                None => true,
            };
            if !matches {
                continue;
            }
            if let Some(apply) = part.get("apply") {
                refs.extend(parse_model_refs(apply, &mut warnings));
            }
            if refs.len() > MAX_MODELS_PER_STATE {
                warnings.push(format!(
                    "more than {MAX_MODELS_PER_STATE} models match; the rest were ignored"
                ));
                refs.truncate(MAX_MODELS_PER_STATE);
                break;
            }
        }
        if refs.is_empty() {
            warnings.push(format!(
                "no multipart condition matches {}",
                describe_properties(&request.properties)
            ));
        }
    } else {
        warnings.push(format!(
            "{blockstate_path} has neither variants nor multipart"
        ));
    }

    let mut parts = Vec::new();
    for model_ref in refs {
        if model_ref.uvlock {
            unsupported.push("uvlock is recorded but not applied".to_owned());
        }
        let model = load_model(stack, &model_ref.id, cache, &mut Vec::new())?;
        let Some(model) = model else {
            warnings.push(format!("model {} was not found", model_ref.id));
            continue;
        };
        warnings.extend(model.warnings);
        unsupported.extend(model.unsupported);
        let Some(elements_value) = &model.elements else {
            warnings.push(format!(
                "model {} has no elements (item or builtin model)",
                model_ref.id
            ));
            continue;
        };
        let elements = parse_elements(
            elements_value,
            &model.textures,
            &mut warnings,
            &mut unsupported,
            &mut textures,
        );
        if elements.is_empty() {
            warnings.push(format!("model {} has no usable elements", model_ref.id));
            continue;
        }
        parts.push(BundlePart {
            model: model_ref.id,
            rotation_x: model_ref.rotation_x,
            rotation_y: model_ref.rotation_y,
            uvlock: model_ref.uvlock,
            elements,
        });
    }
    unsupported.sort();
    unsupported.dedup();
    warnings.sort();
    warnings.dedup();
    let missing_model = parts.is_empty();
    Ok(ResolvedBlock {
        block: BundleBlock {
            name,
            properties: request.properties.clone(),
            missing_model,
            parts,
            warnings,
            unsupported,
        },
        textures,
    })
}

fn missing_block(name: String, request: &BlockStateRequest, warning: String) -> ResolvedBlock {
    let mut textures = BTreeSet::new();
    if let Some(fluid) = fluid_texture(&name) {
        textures.insert(fluid.to_owned());
    }
    ResolvedBlock {
        block: BundleBlock {
            name,
            properties: request.properties.clone(),
            missing_model: true,
            parts: Vec::new(),
            warnings: vec![warning],
            unsupported: Vec::new(),
        },
        textures,
    }
}

/// Vanilla fluids are rendered by a dedicated fluid renderer, not by block models. The
/// renderer still needs the real texture so it can show a documented approximation instead
/// of the missing checker.
fn fluid_texture(name: &str) -> Option<&'static str> {
    match name {
        "minecraft:water" => Some("minecraft:block/water_still"),
        "minecraft:flowing_water" => Some("minecraft:block/water_flow"),
        "minecraft:lava" => Some("minecraft:block/lava_still"),
        "minecraft:flowing_lava" => Some("minecraft:block/lava_flow"),
        _ => None,
    }
}

fn describe_properties(properties: &BTreeMap<String, String>) -> String {
    if properties.is_empty() {
        return "(none)".to_owned();
    }
    properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn select_variant<'a>(
    variants: &'a Map<String, Value>,
    properties: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
) -> Option<&'a Value> {
    let mut best: Option<(usize, &Value)> = None;
    let mut fallback: Option<(i32, &str, &Value)> = None;
    for (key, value) in variants {
        let Some(pairs) = parse_variant_key(key) else {
            warnings.push(format!("invalid blockstate variant key: {key}"));
            continue;
        };
        let matches = pairs.iter().all(|(name, expected)| {
            properties
                .get(name)
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected))
        });
        if matches {
            if best.map_or(true, |(specificity, _)| pairs.len() > specificity) {
                best = Some((pairs.len(), value));
            }
            continue;
        }
        // A world state may omit properties the block always has (for example the demo
        // world places `minecraft:grass_block` without `snowy`). Vanilla fills those
        // defaults from code, which is not part of the resource pack. Pick a deterministic
        // default-like variant and report the approximation instead of drawing the missing
        // checker for every such block.
        let score: i32 = pairs
            .iter()
            .map(|(name, expected)| default_value_score(name, expected))
            .sum();
        if fallback.as_ref().map_or(true, |(best_score, best_key, _)| {
            score > *best_score || (score == *best_score && key.as_str() < *best_key)
        }) {
            fallback = Some((score, key.as_str(), value));
        }
    }
    if best.is_none() {
        if let Some((_, key, value)) = fallback {
            warnings.push(format!(
                "no variant exactly matches {}; using the deterministic default variant {key}",
                describe_properties(properties)
            ));
            return Some(value);
        }
    }
    best.map(|(_, value)| value)
}

fn default_value_score(name: &str, value: &str) -> i32 {
    let value = value.to_ascii_lowercase();
    let is = |expected: &str| i32::from(value == expected);
    match name {
        "axis" => is("y") * 4 + is("x") + is("z"),
        "facing" => is("north") * 3 + is("up") + is("south"),
        "half" => is("bottom") * 3,
        "shape" => is("straight") * 3,
        "type" => is("bottom") * 2 + is("single") * 2,
        "level" => i32::from(value == "0") * 2,
        "rotation" | "age" | "moisture" | "distance" | "power" | "delay" => {
            i32::from(value == "0") * 2
        }
        "snowy" | "waterlogged" | "powered" | "lit" | "open" | "in_wall" | "attached"
        | "disarmed" | "persistent" | "occupied" | "conditional" | "unstable" | "locked"
        | "has_record" | "has_book" | "extended" | "eye" | "north" | "east" | "south" | "west"
        | "up" => is("false") * 2,
        "part" => is("head") + is("foot") + is("bottom"),
        _ => 0,
    }
}

fn parse_variant_key(key: &str) -> Option<Vec<(String, String)>> {
    if key.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut pairs = Vec::new();
    for part in key.split(',') {
        let (name, value) = part.split_once('=')?;
        let name = name.trim();
        let value = value.trim();
        if name.is_empty() || value.is_empty() {
            return None;
        }
        if pairs
            .iter()
            .any(|(existing, _): &(String, String)| existing == name)
        {
            return None;
        }
        pairs.push((name.to_owned(), value.to_owned()));
    }
    Some(pairs)
}

fn parse_model_refs(value: &Value, warnings: &mut Vec<String>) -> Vec<ModelRef> {
    if let Some(array) = value.as_array() {
        if array.len() > 1 {
            warnings.push(format!(
                "weighted variant list has {} models; using the first deterministically",
                array.len()
            ));
        }
        return array
            .iter()
            .take(1)
            .filter_map(|item| parse_model_ref(item, warnings))
            .collect();
    }
    parse_model_ref(value, warnings).into_iter().collect()
}

fn parse_model_ref(value: &Value, warnings: &mut Vec<String>) -> Option<ModelRef> {
    let model = value.get("model")?.as_str()?;
    let id = normalize_id(model)?;
    let rotation_x = rotation_degrees(value.get("x"), warnings, model);
    let rotation_y = rotation_degrees(value.get("y"), warnings, model);
    Some(ModelRef {
        id,
        rotation_x,
        rotation_y,
        uvlock: value
            .get("uvlock")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn rotation_degrees(value: Option<&Value>, warnings: &mut Vec<String>, model: &str) -> i32 {
    let Some(value) = value.and_then(Value::as_i64) else {
        return 0;
    };
    if value % 90 != 0 {
        warnings.push(format!(
            "model {model} declares a rotation of {value} degrees; only multiples of 90 are supported"
        ));
        return 0;
    }
    (value % 360).rem_euclid(360) as i32
}

fn load_model(
    stack: &mut ResourceStack,
    id: &str,
    cache: &mut HashMap<String, Option<CachedModel>>,
    visiting: &mut Vec<String>,
) -> Result<Option<CachedModel>, BundleError> {
    if let Some(cached) = cache.get(id) {
        return Ok(cached.clone());
    }
    if visiting.iter().any(|entry| entry == id) {
        return Err(BundleError::UnsupportedFormat(format!(
            "model parent cycle: {} -> {id}",
            visiting.join(" -> ")
        )));
    }
    if visiting.len() >= MAX_MODEL_DEPTH {
        return Err(BundleError::LimitExceeded(format!(
            "model parent chain is deeper than {MAX_MODEL_DEPTH} at {id}"
        )));
    }
    let (namespace, path) = id.split_once(':').expect("validated model id");
    let model_path = format!("assets/{namespace}/models/{path}.json");
    let Some(document) = stack.read_json(&model_path)? else {
        cache.insert(id.to_owned(), None);
        return Ok(None);
    };
    if !document.is_object() {
        return Err(BundleError::UnsupportedFormat(format!(
            "{model_path} is not a JSON object"
        )));
    }
    visiting.push(id.to_owned());
    let parent = document.get("parent").and_then(Value::as_str);
    let mut resolved = if let Some(parent) = parent {
        if parent.starts_with("builtin/") {
            Some(CachedModel {
                textures: BTreeMap::new(),
                elements: None,
                warnings: Vec::new(),
                unsupported: vec!["builtin model".to_owned()],
            })
        } else {
            let parent_id = normalize_id(parent).ok_or_else(|| {
                BundleError::UnsupportedFormat(format!("invalid parent model id: {parent}"))
            })?;
            load_model(stack, &parent_id, cache, visiting)?
        }
    } else {
        None
    };
    visiting.pop();
    let mut model = resolved.take().unwrap_or_else(|| CachedModel {
        textures: BTreeMap::new(),
        elements: None,
        warnings: Vec::new(),
        unsupported: Vec::new(),
    });
    if let Some(textures) = document.get("textures").and_then(Value::as_object) {
        if textures.len() > MAX_TEXTURES {
            return Err(BundleError::LimitExceeded(format!(
                "model {id} declares more than {MAX_TEXTURES} textures"
            )));
        }
        for (key, value) in textures {
            if let Some(texture) = value.as_str() {
                model.textures.insert(key.clone(), texture.to_owned());
            }
        }
    }
    if let Some(elements) = document.get("elements") {
        model.elements = Some(elements.clone());
    }
    if document.get("ambientocclusion").and_then(Value::as_bool) == Some(false) {
        model.unsupported.push("ambientocclusion=false".to_owned());
    }
    cache.insert(id.to_owned(), Some(model.clone()));
    Ok(Some(model))
}

fn parse_elements(
    value: &Value,
    textures: &BTreeMap<String, String>,
    warnings: &mut Vec<String>,
    unsupported: &mut Vec<String>,
    texture_keys: &mut BTreeSet<String>,
) -> Vec<BundleElement> {
    let Some(items) = value.as_array() else {
        warnings.push("model elements are not an array".to_owned());
        return Vec::new();
    };
    if items.len() > MAX_ELEMENTS {
        warnings.push(format!(
            "model has more than {MAX_ELEMENTS} elements; the rest were ignored"
        ));
    }
    let mut elements = Vec::new();
    for item in items.iter().take(MAX_ELEMENTS) {
        let Some(from) = read_vec3(item.get("from")) else {
            warnings.push("element has an invalid `from` vector".to_owned());
            continue;
        };
        let Some(to) = read_vec3(item.get("to")) else {
            warnings.push("element has an invalid `to` vector".to_owned());
            continue;
        };
        if from.iter().any(|value| *value < -16.0 || *value > 32.0)
            || to.iter().any(|value| *value < -16.0 || *value > 32.0)
        {
            warnings.push("element bounds are outside -16..32".to_owned());
            continue;
        }
        if (0..3).any(|axis| from[axis] > to[axis]) {
            warnings.push("element `from` is greater than `to`".to_owned());
            continue;
        }
        let rotation = parse_element_rotation(item.get("rotation"), warnings);
        if item.get("rotation").is_some() && rotation.is_none() {
            unsupported.push("element rotation was ignored".to_owned());
        }
        let shade = item.get("shade").and_then(Value::as_bool).unwrap_or(true);
        let mut faces = Vec::new();
        if let Some(map) = item.get("faces").and_then(Value::as_object) {
            for direction in DIRECTIONS {
                let Some(face) = map.get(direction) else {
                    continue;
                };
                let texture = face
                    .get("texture")
                    .and_then(Value::as_str)
                    .and_then(|reference| resolve_texture(reference, textures, 0))
                    .unwrap_or_else(|| {
                        warnings.push(format!(
                            "element face {direction} has a missing or unresolved texture"
                        ));
                        "__missing__".to_owned()
                    });
                let uv = read_uv(face.get("uv"), warnings);
                let uv_rotation = face
                    .get("rotation")
                    .and_then(Value::as_i64)
                    .map(|value| {
                        let normalized = (value % 360).rem_euclid(360);
                        if !matches!(normalized, 0 | 90 | 180 | 270) {
                            warnings.push(format!(
                                "face rotation {value} is not a multiple of 90; using 0"
                            ));
                            0
                        } else {
                            normalized as u8
                        }
                    })
                    .unwrap_or(0);
                let cullface = face
                    .get("cullface")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .filter(|value| DIRECTIONS.contains(&value.as_str()));
                let tint_index = face
                    .get("tintindex")
                    .and_then(Value::as_i64)
                    .and_then(|value| u8::try_from(value).ok())
                    .filter(|value| *value <= 15);
                texture_keys.insert(texture.clone());
                faces.push(BundleFace {
                    direction: direction.to_owned(),
                    texture,
                    uv,
                    rotation: uv_rotation,
                    cullface,
                    tint_index,
                });
            }
        }
        if faces.is_empty() {
            continue;
        }
        elements.push(BundleElement {
            from,
            to,
            rotation,
            shade,
            faces,
        });
    }
    elements
}

fn read_vec3(value: Option<&Value>) -> Option<[f32; 3]> {
    let items = value?.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let mut output = [0.0f32; 3];
    for (index, item) in items.iter().enumerate() {
        let number = item.as_f64()?;
        if !number.is_finite() {
            return None;
        }
        output[index] = number as f32;
    }
    Some(output)
}

fn read_uv(value: Option<&Value>, warnings: &mut Vec<String>) -> [f32; 4] {
    let Some(value) = value else {
        return [0.0, 0.0, 16.0, 16.0];
    };
    let Some(items) = value.as_array() else {
        warnings.push("face uv is not an array; using the full texture".to_owned());
        return [0.0, 0.0, 16.0, 16.0];
    };
    if items.len() != 4 {
        warnings.push("face uv does not have four numbers; using the full texture".to_owned());
        return [0.0, 0.0, 16.0, 16.0];
    }
    let mut output = [0.0f32; 4];
    for (index, item) in items.iter().enumerate() {
        let Some(number) = item.as_f64() else {
            warnings.push("face uv contains a non-number; using the full texture".to_owned());
            return [0.0, 0.0, 16.0, 16.0];
        };
        if !number.is_finite() || !(-64.0..=64.0).contains(&number) {
            warnings.push("face uv is outside -64..64; using the full texture".to_owned());
            return [0.0, 0.0, 16.0, 16.0];
        }
        output[index] = number as f32;
    }
    output
}

fn parse_element_rotation(
    value: Option<&Value>,
    warnings: &mut Vec<String>,
) -> Option<BundleElementRotation> {
    let value = value?;
    let origin = read_vec3(value.get("origin")).unwrap_or([8.0, 8.0, 8.0]);
    if origin.iter().any(|value| *value < 0.0 || *value > 16.0) {
        warnings.push("element rotation origin is outside 0..16".to_owned());
        return None;
    }
    let axis = value.get("axis").and_then(Value::as_str).unwrap_or("y");
    if !matches!(axis, "x" | "y" | "z") {
        warnings.push(format!("invalid element rotation axis: {axis}"));
        return None;
    }
    let angle = value.get("angle").and_then(Value::as_f64).unwrap_or(0.0) as f32;
    if !angle.is_finite() || angle.abs() > 45.0 {
        warnings.push("element rotation angle is outside -45..45".to_owned());
        return None;
    }
    Some(BundleElementRotation {
        origin,
        axis: axis.to_owned(),
        angle,
        rescale: value
            .get("rescale")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn resolve_texture(
    reference: &str,
    textures: &BTreeMap<String, String>,
    depth: usize,
) -> Option<String> {
    if depth > MAX_MODEL_DEPTH {
        return None;
    }
    let reference = reference.trim();
    if reference.is_empty() || reference == "missing" {
        return None;
    }
    if let Some(name) = reference.strip_prefix('#') {
        let next = textures.get(name)?;
        return resolve_texture(next, textures, depth + 1);
    }
    normalize_id(reference)
}

fn condition_matches(
    value: &Value,
    properties: &BTreeMap<String, String>,
    depth: usize,
) -> Result<bool, String> {
    if depth > MAX_CONDITION_DEPTH {
        return Err("multipart condition nesting is too deep".to_owned());
    }
    match value {
        Value::String(text) => Ok(condition_string_matches(text, properties)),
        Value::Array(items) => {
            let mut result = true;
            for item in items {
                result = result && condition_matches(item, properties, depth + 1)?;
            }
            Ok(result)
        }
        Value::Object(map) => {
            if let Some(items) = map.get("OR").and_then(Value::as_array) {
                let mut result = false;
                for item in items {
                    result = result || condition_matches(item, properties, depth + 1)?;
                }
                return Ok(result);
            }
            if let Some(items) = map.get("AND").and_then(Value::as_array) {
                let mut result = true;
                for item in items {
                    result = result && condition_matches(item, properties, depth + 1)?;
                }
                return Ok(result);
            }
            let mut result = true;
            for (key, expected) in map {
                let Some(expected) = expected.as_str() else {
                    return Err(format!("unsupported multipart condition value for {key}"));
                };
                result = result
                    && properties
                        .get(key)
                        .is_some_and(|actual| actual.eq_ignore_ascii_case(expected));
            }
            Ok(result)
        }
        _ => Err("unsupported multipart condition".to_owned()),
    }
}

fn condition_string_matches(text: &str, properties: &BTreeMap<String, String>) -> bool {
    let text = text.trim();
    if text.is_empty() {
        return true;
    }
    if text.eq_ignore_ascii_case("true") {
        return true;
    }
    if text.eq_ignore_ascii_case("false") {
        return false;
    }
    text.split('|').any(|alternative| {
        alternative.split(',').all(|pair| {
            let Some((name, expected)) = pair.split_once('=') else {
                return false;
            };
            properties
                .get(name.trim())
                .is_some_and(|actual| actual.eq_ignore_ascii_case(expected.trim()))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn variant_keys_and_specificity_work() {
        let variants = json!({
            "": { "model": "block/stone" },
            "facing=north": { "model": "block/stone_north" },
            "facing=north,half=top": { "model": "block/stone_top" }
        });
        let map = variants.as_object().unwrap();
        let mut warnings = Vec::new();
        let mut properties = BTreeMap::new();
        properties.insert("facing".to_owned(), "north".to_owned());
        properties.insert("half".to_owned(), "top".to_owned());
        let selected = select_variant(map, &properties, &mut warnings).unwrap();
        assert_eq!(
            selected.get("model").and_then(Value::as_str),
            Some("block/stone_top")
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_properties_use_a_default_like_variant() {
        let variants = json!({
            "snowy=true": { "model": "block/grass_block_snow" },
            "snowy=false": { "model": "block/grass_block" }
        });
        let map = variants.as_object().unwrap();
        let mut warnings = Vec::new();
        let selected = select_variant(map, &BTreeMap::new(), &mut warnings).unwrap();
        assert_eq!(
            selected.get("model").and_then(Value::as_str),
            Some("block/grass_block")
        );
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("deterministic default variant")));

        let stairs = json!({
            "facing=east,half=bottom,shape=inner_left": { "model": "inner" },
            "facing=north,half=bottom,shape=straight,waterlogged=false": { "model": "straight" }
        });
        let mut warnings = Vec::new();
        let selected =
            select_variant(stairs.as_object().unwrap(), &BTreeMap::new(), &mut warnings).unwrap();
        assert_eq!(
            selected.get("model").and_then(Value::as_str),
            Some("straight")
        );
    }

    #[test]
    fn multipart_conditions_handle_and_or_and_values() {
        let properties = BTreeMap::from([
            ("north".to_owned(), "true".to_owned()),
            ("east".to_owned(), "false".to_owned()),
        ]);
        assert!(condition_matches(&json!("north=true"), &properties, 0).unwrap());
        assert!(!condition_matches(&json!("north=false"), &properties, 0).unwrap());
        assert!(condition_matches(
            &json!({ "OR": ["east=true", "north=true"] }),
            &properties,
            0
        )
        .unwrap());
        assert!(condition_matches(
            &json!({ "AND": ["north=true", { "OR": ["east=true", "east=false"] }] }),
            &properties,
            0
        )
        .unwrap());
        assert!(!condition_matches(
            &json!({ "AND": ["north=true", "east=true"] }),
            &properties,
            0
        )
        .unwrap());
    }

    #[test]
    fn uv_and_model_rotation_are_validated() {
        let mut warnings = Vec::new();
        assert_eq!(
            read_uv(Some(&json!([8, 0, 16, 8])), &mut warnings),
            [8.0, 0.0, 16.0, 8.0]
        );
        assert_eq!(
            read_uv(Some(&json!([1, 2, 3])), &mut warnings),
            [0.0, 0.0, 16.0, 16.0]
        );
        assert_eq!(rotation_degrees(Some(&json!(270)), &mut warnings, "m"), 270);
        assert_eq!(rotation_degrees(Some(&json!(-90)), &mut warnings, "m"), 270);
        assert_eq!(rotation_degrees(Some(&json!(45)), &mut warnings, "m"), 0);
        assert!(!warnings.is_empty());
    }
}
