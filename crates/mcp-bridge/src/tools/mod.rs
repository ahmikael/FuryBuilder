//! MCP tool catalog and dispatch.

mod bridge;
mod jobs;
mod read;
mod write;

use std::path::PathBuf;

use rmcp::model::{CallToolResult, ClientCapabilities, ContentBlock, Tool, ToolAnnotations};
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};

use crate::apps::observation_tool_meta;
use crate::capabilities::{require_project_id, ProtocolEra};
use crate::desktop::{self, CallOptions, DesktopError, JobError};
use crate::schema;

pub const JOB_TOOL_NAMES: [&str; 3] = ["start_job", "get_job_status", "cancel_job"];

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub client_id: String,
    pub started_at_ms: u64,
    /// Test-only override; production resolves the discovery file from the environment.
    pub runtime_path: Option<PathBuf>,
}

impl ToolContext {
    pub fn options(&self) -> CallOptions {
        let options = CallOptions::new(self.client_id.clone());
        match &self.runtime_path {
            Some(path) => options.with_runtime_path(path.clone()),
            None => options,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestInfo {
    pub era: ProtocolEra,
    pub protocol_version: Option<String>,
    pub client_capabilities: Option<ClientCapabilities>,
}

pub fn text_result(value: &Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned()),
    )])
}

pub fn bridge_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "bridge_error: {}",
        message.into()
    ))])
}

pub fn desktop_error(error: DesktopError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error.tool_text())])
}

pub fn job_error(error: JobError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error.tool_text())])
}

pub fn project_id(args: &Map<String, Value>) -> Result<String, String> {
    let raw = crate::args::required_string(args, "project_id", 1, 128)?;
    require_project_id(&raw)
}

pub fn with_revision(mut value: Value, revision: Option<u64>) -> Value {
    if let Some(revision) = revision {
        if let Some(object) = value.as_object_mut() {
            object.insert("worldRevision".to_owned(), json!(revision));
        }
    }
    value
}

fn tool(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    input_schema: Value,
    annotations: ToolAnnotations,
) -> Tool {
    let schema = input_schema
        .as_object()
        .cloned()
        .expect("tool input schema is an object");
    Tool::new(name, description, schema)
        .with_title(title)
        .with_annotations(annotations)
}

fn with_project(
    mut base: Vec<(&'static str, Value)>,
    extra: Vec<(&'static str, Value)>,
) -> Vec<(&'static str, Value)> {
    base.extend(extra);
    base
}

fn project_only_schema() -> Value {
    schema::object(schema::project_properties(), &["project_id"])
}

pub fn tool_definitions() -> Vec<Tool> {
    let read = schema::read_only_annotations();
    let write = schema::write_annotations();
    let mut tools = vec![
        tool(
            "bridge_info",
            "Bridge information",
            "Read-only handshake diagnostic. Returns the negotiated protocol era and confirms that project context is explicitly supplied.",
            project_only_schema(),
            read.clone(),
        ),
        tool(
            "start_job",
            "Start a long-running job",
            "Starts work through the authoritative Job API and returns a job_id. Requires idempotency_key.",
            schema::object(
                vec![
                    (
                        "project_id",
                        schema::description(
                            schema::string(1, 128),
                            "Project identifier; never inferred from the MCP connection.",
                        ),
                    ),
                    (
                        "kind",
                        schema::description(
                            schema::string(1, 64),
                            "Job kind understood by the Application API.",
                        ),
                    ),
                    (
                        "parameters",
                        schema::default_value(
                            json!({ "type": "object", "additionalProperties": true }),
                            json!({}),
                        ),
                    ),
                    ("idempotency_key", schema::string(1, 128)),
                    (
                        "expected_revision",
                        schema::description(
                            schema::non_negative_integer(),
                            "Deprecated alias for world_revision; accepted for clients written before the explicit context contract.",
                        ),
                    ),
                    (
                        "world_revision",
                        schema::description(
                            schema::non_negative_integer(),
                            "World revision the caller observed. Required for mutating jobs; the desktop rejects a missing revision.",
                        ),
                    ),
                ],
                &["project_id", "kind", "idempotency_key"],
            ),
            schema::non_destructive_write_annotations(),
        ),
        tool(
            "get_job_status",
            "Get job status",
            "Reads progress and the final result of a job from the Job API.",
            schema::object(
                vec![
                    ("project_id", schema::string(1, 128)),
                    ("job_id", schema::string(1, 128)),
                ],
                &["project_id", "job_id"],
            ),
            schema::job_query_annotations(),
        ),
        tool(
            "cancel_job",
            "Cancel job",
            "Requests cancellation through the Job API. Cancellation is not assumed to be instantaneous.",
            schema::object(
                vec![
                    ("project_id", schema::string(1, 128)),
                    ("job_id", schema::string(1, 128)),
                ],
                &["project_id", "job_id"],
            ),
            schema::non_destructive_write_annotations(),
        ),
        tool(
            "world_info",
            "World information",
            "Returns the target Minecraft version, dimensions, bounds, revision and load status of the open project.",
            project_only_schema(),
            read.clone(),
        ),
        tool(
            "get_workspace_policy",
            "Get workspace policy",
            "Returns edit layers, the active layer, the optional active mask and protected regions. Enforcement stays in the Core; locked layers, mask violations and protected regions are returned as permission_denied.",
            project_only_schema(),
            read.clone(),
        ),
        tool(
            "list_templates",
            "List structure templates",
            "Lists verified reusable Java .nbt templates in the open project's library with size, checksum, non-air block count and creation time. Read-only; insertion uses insert_template with explicit access and confirmation.",
            project_only_schema(),
            read.clone(),
        ),
        tool(
            "list_scene_objects",
            "List semantic objects",
            "Lists the project's editor-only semantic objects (districts, buildings, roads) with their kind, dimension, bounds, parent and tags. These are metadata; reading them never changes blocks.",
            project_only_schema(),
            read.clone(),
        ),
        tool(
            "inspect_scene_object",
            "Inspect semantic object",
            "Returns one semantic object, its current block palette, volume, non-air count and direct children at the authoritative revision. Read-only; the bounds are project metadata, not a generated structure.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    (
                        "object_id",
                        schema::description(
                            schema::string(1, 64),
                            "Semantic object id from list_scene_objects.",
                        ),
                    ),
                    ],
                ),
                &["project_id", "object_id"],
            ),
            read.clone(),
        ),
        tool(
            "list_parametric_objects",
            "List parametric objects",
            "Lists editor-only parametric recipes (generator, version, bounds, rotation, parameters, layer and applied revision). These are metadata; the generated voxels are ordinary reversible world content.",
            project_only_schema(),
            read.clone(),
        ),
        tool(
            "inspect_parametric_object",
            "Inspect parametric object",
            "Returns one parametric recipe plus bounded conflict details: positions where manual edits no longer match the generated overlay. Read-only; transform, bake and rollback are confirmed desktop actions.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    (
                        "object_id",
                        schema::description(
                            schema::string(1, 64),
                            "Parametric object id from list_parametric_objects.",
                        ),
                    ),
                    ],
                ),
                &["project_id", "object_id"],
            ),
            read.clone(),
        ),
        tool(
            "preview_build_dsl",
            "Preview Build DSL",
            "Validates a versioned declarative build recipe, compiles it deterministically into one typed Batch operation and returns the ordinary preview, budget metadata and the exact operation. Read-only: nothing is written. Pass the returned operation to apply_operation (which requires explicit write access and confirmation) to commit it.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("recipe", schema::build_recipe()),
                    ],
                ),
                &["project_id", "recipe"],
            ),
            read.clone(),
        ),
        tool(
            "get_block",
            "Get block",
            "Reads the exact block state at one position, including a canonical state string and the authoritative world revision it was read at.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("dimension", schema::string(1, 128)),
                    ("pos", schema::block_pos()),
                    ],
                ),
                &["project_id", "dimension", "pos"],
            ),
            read.clone(),
        ),
        tool(
            "get_biome",
            "Get biome",
            "Reads the biome covering one block position from the editable 4x4x4 cell grid, with the authoritative world revision. null means the source world has no biome data.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("dimension", schema::string(1, 128)),
                    ("pos", schema::block_pos()),
                    ],
                ),
                &["project_id", "dimension", "pos"],
            ),
            read.clone(),
        ),
        tool(
            "find_structure_sites",
            "Find structure sites",
            "Runs a bounded, read-only placement-rule check over a region and returns non-overlapping flat, dry, clear footprints. This is a deterministic heuristic, not a vanilla structure-generation claim.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("dimension", schema::string(1, 128)),
                    ("bounds", schema::bounds()),
                    ("width", schema::integer(1, 32)),
                    ("depth", schema::integer(1, 32)),
                    ("step", schema::default_value(schema::integer(1, 64), json!(5))),
                    (
                        "maxSlope",
                        schema::default_value(schema::integer(0, 32), json!(2)),
                    ),
                    (
                        "clearance",
                        schema::default_value(schema::integer(0, 32), json!(2)),
                    ),
                    ("limit", schema::default_value(schema::integer(1, 64), json!(8))),
                    ],
                ),
                &["project_id", "dimension", "bounds", "width", "depth"],
            ),
            read.clone(),
        ),
        tool(
            "inspect_region",
            "Inspect region",
            "Summarizes an inclusive block region: volume, non-air count, palette and the authoritative world revision. Use it before editing an area.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("dimension", schema::string(1, 128)),
                    ("bounds", schema::bounds()),
                    ],
                ),
                &["project_id", "dimension", "bounds"],
            ),
            read.clone(),
        ),
        tool(
            "get_region",
            "Get region blocks",
            "Returns the non-air blocks of the requested inclusive region up to the Core read limit, with the authoritative world revision. The result reports the volume so callers can detect an empty box.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("dimension", schema::string(1, 128)),
                    ("bounds", schema::bounds()),
                    ],
                ),
                &["project_id", "dimension", "bounds"],
            ),
            read.clone(),
        ),
        tool(
            "get_camera",
            "Get viewport camera",
            "Returns the current editor camera pose, the dimension it is showing and the render revision. The verification block marks the pose stale if the world advanced.",
            project_only_schema(),
            read.clone(),
        ),
        {
            let mut tool = tool(
                "observe",
                "Observe region",
                "Renders a deterministic PNG observation from authoritative voxel data at the given world revision. Modes: top_down, heightmap, material_map, biome_map, cross_section (requires slice) and revision_diff (requires from_revision). Returns the image plus legend, bounds, scale, completeness and warnings. Read-only; exact block queries remain authoritative.",
                schema::object(
                    with_project(
                        schema::project_properties(),
                        vec![
                        ("dimension", schema::string(1, 128)),
                        ("bounds", schema::bounds()),
                        ("mode", schema::observation_mode()),
                        (
                            "slice",
                            schema::description(
                                schema::object(
                                    vec![
                                        ("axis", schema::enum_string(&["x", "y", "z"])),
                                        ("coordinate", schema::any_integer()),
                                    ],
                                    &["axis", "coordinate"],
                                ),
                                "Required for cross_section; ignored by other modes.",
                            ),
                        ),
                        (
                            "from_revision",
                            schema::description(
                                schema::non_negative_integer(),
                                "Required for revision_diff; the current world revision is the upper bound.",
                            ),
                        ),
                        ],
                    ),
                    &["project_id", "dimension", "bounds", "mode"],
                ),
                read.clone(),
            );
            tool.meta = Some(observation_tool_meta());
            tool
        },
        tool(
            "validate_region",
            "Validate region",
            "Runs bounded, read-only build validators against authoritative voxel data. Findings carry rule_id, severity, confidence, bounds, position samples and optional suggested fix operations. Nothing is written: apply a suggested operation only through preview_operation and the confirmed apply_operation path. Floating-block support is a heuristic, not a Minecraft simulation.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("dimension", schema::string(1, 128)),
                    ("bounds", schema::bounds()),
                    (
                        "rules",
                        schema::description(
                            schema::array(schema::validation_rules(), 1, 7),
                            "Rules to run. Omit for the default set; out_of_bounds and path_continuity need their inputs.",
                        ),
                    ),
                    (
                        "allowed_bounds",
                        schema::description(
                            schema::bounds(),
                            "Required when running out_of_bounds explicitly.",
                        ),
                    ),
                    (
                        "path",
                        schema::description(
                            schema::path_spec(),
                            "Required when running path_continuity explicitly.",
                        ),
                    ),
                    ("max_surface_materials", schema::integer(1, 64)),
                    (
                        "floating_fix",
                        schema::description(
                            schema::enum_string(&["none", "remove", "support"]),
                            "Suggested repair mode for floating blocks; it is never auto-applied.",
                        ),
                    ),
                    ],
                ),
                &["project_id", "dimension", "bounds"],
            ),
            read.clone(),
        ),
        tool(
            "capture_viewport",
            "Capture viewport",
            "Renders the current editor viewport and returns a real PNG image plus observation metadata. partial=true means requested chunks were not loaded; stale=true means the rendered revision is behind the authoritative world.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    (
                        "dimension",
                        schema::description(
                            schema::string(1, 128),
                            "Expected visible dimension. The capture is refused if the viewport shows another one.",
                        ),
                    ),
                    ],
                ),
                &["project_id"],
            ),
            read.clone(),
        ),
        tool(
            "preview_operation",
            "Preview operation",
            "Validates a typed operation and reports affected/changed blocks, dirty chunks and warnings without modifying the world. Always preview before applying a large change.",
            schema::object(
                with_project(
                    schema::project_properties(),
                    vec![
                    ("operation", schema::operation()),
                    ],
                ),
                &["project_id", "operation"],
            ),
            read.clone(),
        ),
        tool(
            "apply_operation",
            "Apply operation",
            "Applies one typed, reversible operation. Requires explicit MCP write access in FuryBuilder; otherwise returns permission_denied. Use preview_operation first and send world_revision plus a unique idempotency_key.",
            schema::object(
                with_project(
                    schema::write_project_properties(),
                    vec![
                    ("operation", schema::operation()),
                    ("idempotency_key", schema::idempotency_key()),
                    ],
                ),
                &["project_id", "world_revision", "operation", "idempotency_key"],
            ),
            write.clone(),
        ),
        tool(
            "apply_operations_batch",
            "Apply block batch",
            "Atomically applies up to 8192 block changes in one dimension. Requires explicit MCP write access; preview first and reuse the same idempotency_key when retrying.",
            schema::object(
                with_project(
                    schema::write_project_properties(),
                    vec![
                    ("dimension", schema::dimension()),
                    ("changes", schema::array(schema::change(), 1, 8192)),
                    ("idempotency_key", schema::idempotency_key()),
                    ],
                ),
                &[
                    "project_id",
                    "world_revision",
                    "dimension",
                    "changes",
                    "idempotency_key",
                ],
            ),
            write.clone(),
        ),
        tool(
            "insert_template",
            "Insert structure template",
            "Inserts one verified template from the project library as a single reversible Batch operation. Requires explicit MCP write access and confirmation; the template is re-verified from disk and workspace masks/protected regions are enforced by the Core.",
            schema::object(
                with_project(
                    schema::write_project_properties(),
                    vec![
                    (
                        "template_id",
                        json!({ "type": "string", "format": "uuid" }),
                    ),
                    ("dimension", schema::dimension()),
                    (
                        "origin",
                        json!({
                            "type": "array",
                            "items": { "type": "integer" },
                            "minItems": 3,
                            "maxItems": 3
                        }),
                    ),
                    (
                        "include_air",
                        schema::default_value(schema::boolean(), json!(false)),
                    ),
                    ("layer_id", schema::string(1, 64)),
                    ("idempotency_key", schema::idempotency_key()),
                    ],
                ),
                &[
                    "project_id",
                    "world_revision",
                    "template_id",
                    "dimension",
                    "origin",
                    "idempotency_key",
                ],
            ),
            write.clone(),
        ),
        tool(
            "undo_operation",
            "Undo operation",
            "Undoes the latest operation, or the named operation_id when it is still the latest. Requires explicit MCP write access. The response reports the new revision and dirty chunks.",
            schema::object(
                with_project(
                    schema::write_project_properties(),
                    vec![
                    ("operation_id", schema::string(1, 128)),
                    ],
                ),
                &["project_id", "world_revision"],
            ),
            write.clone(),
        ),
        tool(
            "get_operation_status",
            "Get operation status",
            "Reads the authoritative status of one operation_id, or a bounded recent-operation summary. It also reports the job queue and, for a named operation, the retained job that produced it. Long-running work itself goes through the start_job/get_job_status/cancel_job tools.",
            schema::object(
                vec![
                    ("project_id", schema::string(1, 128)),
                    ("operation_id", schema::string(1, 128)),
                ],
                &["project_id"],
            ),
            read,
        ),
    ];
    tools.shrink_to_fit();
    tools
}

pub async fn call_tool(
    name: &str,
    arguments: Option<Map<String, Value>>,
    context: &ToolContext,
    request: &RequestInfo,
) -> Result<CallToolResult, McpError> {
    let args = arguments.unwrap_or_default();
    let result = match name {
        "bridge_info" => bridge::bridge_info(&args, context, request).await,
        "start_job" => jobs::start_job(&args, context).await,
        "get_job_status" => jobs::get_job_status(&args, context).await,
        "cancel_job" => jobs::cancel_job(&args, context).await,
        "world_info"
        | "get_workspace_policy"
        | "list_templates"
        | "list_scene_objects"
        | "inspect_scene_object"
        | "list_parametric_objects"
        | "inspect_parametric_object"
        | "preview_build_dsl"
        | "get_block"
        | "get_biome"
        | "find_structure_sites"
        | "inspect_region"
        | "get_region"
        | "get_camera"
        | "observe"
        | "validate_region"
        | "capture_viewport" => read::call(name, &args, context).await,
        "preview_operation"
        | "apply_operation"
        | "apply_operations_batch"
        | "insert_template"
        | "undo_operation"
        | "get_operation_status" => write::call(name, &args, context).await,
        other => bridge_error(format!("unknown tool `{other}`")),
    };
    Ok(result)
}

pub(crate) async fn call_desktop(
    context: &ToolContext,
    method: &str,
    params: Value,
    timeout: Option<std::time::Duration>,
) -> Result<Value, DesktopError> {
    let mut options = context.options();
    if let Some(timeout) = timeout {
        options = options.with_timeout(timeout);
    }
    desktop::call_desktop(method, params, &options).await
}
