mod support;

use serde_json::{json, Value};
use support::{result_text, tool_json, BridgeProcess, MockDesktop};

fn png() -> &'static str {
    "data:image/png;base64,iVBORw0KGgo="
}

fn hit(method: &str, request: &Value) -> Value {
    let params = &request["params"];
    match method {
        "world_info" => support::ok(
            request,
            json!({
                "targetMinecraftVersion": "1.21.4",
                "revision": 9,
                "dimensions": ["minecraft:overworld"]
            }),
        ),
        "query" => {
            let query = &params["query"];
            match query["type"].as_str() {
                Some("region_summary") => support::ok(
                    request,
                    json!({
                        "type": "region_summary",
                        "worldRevision": 9,
                        "data": {
                            "dimension": "minecraft:overworld",
                            "bounds": query["bounds"],
                            "blocks": 27,
                            "nonAirBlocks": 1,
                            "palette": [{ "state": "minecraft:stone", "count": 1 }]
                        }
                    }),
                ),
                _ => support::ok(
                    request,
                    json!({
                        "type": "block",
                        "worldRevision": 9,
                        "data": {
                            "dimension": "minecraft:overworld",
                            "pos": query["pos"],
                            "state": { "name": "minecraft:stone", "properties": {} }
                        }
                    }),
                ),
            }
        }
        "get_camera" => support::ok(
            request,
            json!({
                "kind": "camera",
                "dimension": "minecraft:overworld",
                "worldRevision": 9,
                "pose": {
                    "position": [1, 2, 3],
                    "target": [0, 0, 0],
                    "yaw": 0.5,
                    "pitch": -0.2,
                    "fov": 70
                },
                "capturedAtMs": 1_700_000_000_000_i64,
                "verification": { "authoritativeRevision": 9, "renderRevision": 9, "stale": false }
            }),
        ),
        "capture_viewport" => support::ok(
            request,
            json!({
                "kind": "capture",
                "dataUrl": png(),
                "metadata": {
                    "schemaVersion": 1,
                    "mode": "viewport",
                    "dimension": "minecraft:overworld",
                    "worldRevision": 9,
                    "partial": true,
                    "missingChunks": [{ "x": 0, "z": 0 }]
                },
                "capturedAtMs": 1_700_000_000_000_i64,
                "verification": {
                    "authoritativeRevision": 9,
                    "renderRevision": 9,
                    "stale": false,
                    "partial": true
                }
            }),
        ),
        "observe" => support::ok(
            request,
            json!({
                "image": png(),
                "mode": params["observation"]["mode"],
                "dimension": params["observation"]["dimension"],
                "bounds": params["observation"]["bounds"],
                "worldRevision": 9,
                "legend": [],
                "warnings": []
            }),
        ),
        "preview_operation" => support::ok(
            request,
            json!({
                "affectedBlocks": 2,
                "changedBlocks": 1,
                "dirtyChunks": [{ "x": 0, "z": 0 }],
                "warnings": [],
                "worldRevision": 9
            }),
        ),
        "apply_operation" => support::ok(
            request,
            json!({
                "operationId": "op-1",
                "worldRevision": 10,
                "changedBlocks": 2,
                "dirtyChunks": [{ "x": 0, "z": 0 }],
                "warnings": [],
                "replayed": false
            }),
        ),
        "insert_template" => support::ok(
            request,
            json!({
                "operationId": "op-template",
                "worldRevision": 10,
                "changedBlocks": 4,
                "dirtyChunks": [],
                "warnings": [],
                "replayed": false,
                "template": { "name": "Watchtower" },
                "bounds": {
                    "min": { "x": 0, "y": 64, "z": 0 },
                    "max": { "x": 3, "y": 70, "z": 3 }
                }
            }),
        ),
        "undo_operation" => support::ok(
            request,
            json!({
                "operationId": "op-2",
                "worldRevision": 11,
                "changedBlocks": 2,
                "dirtyChunks": [],
                "warnings": [],
                "replayed": false
            }),
        ),
        "operation_status" => support::ok(
            request,
            json!({
                "operationId": "op-1",
                "status": "completed",
                "changedBlocks": 2,
                "currentRevision": 10,
                "canUndo": true,
                "job": { "available": false, "reason": "job_api_unavailable" }
            }),
        ),
        "start_job" => {
            if params["kind"] == "undo_operation" {
                return support::failed(request, "permission_denied", "writes are disabled");
            }
            support::ok(
                request,
                json!({
                    "jobId": "job-1",
                    "projectId": params["projectId"],
                    "kind": params["kind"],
                    "state": "queued",
                    "progress": 0,
                    "message": "queued",
                    "worldRevision": Value::Null,
                    "result": Value::Null,
                    "cancelRequested": false,
                    "replayed": false
                }),
            )
        }
        "job_status" => support::ok(
            request,
            json!({
                "jobId": params["jobId"],
                "projectId": params["projectId"],
                "kind": "apply_operation",
                "state": "running",
                "progress": 42,
                "message": "applying",
                "worldRevision": Value::Null,
                "result": Value::Null
            }),
        ),
        "cancel_job" => support::ok(
            request,
            json!({
                "jobId": params["jobId"],
                "projectId": params["projectId"],
                "kind": "apply_operation",
                "state": "cancelled",
                "progress": 42,
                "message": "cancellation requested",
                "worldRevision": Value::Null,
                "result": Value::Null,
                "cancelRequested": true
            }),
        ),
        other => support::failed(
            request,
            "invalid_argument",
            &format!("unexpected method {other}"),
        ),
    }
}

async fn bridge_with_desktop() -> (BridgeProcess, MockDesktop) {
    let handler = std::sync::Arc::new(|request: Value| {
        let method = request["method"].as_str().unwrap_or_default().to_owned();
        hit(&method, &request)
    });
    let desktop = MockDesktop::start(handler).await;
    let mut bridge = BridgeProcess::spawn(&desktop.runtime_path);
    bridge.initialize_legacy().await;
    bridge.notify("notifications/initialized", json!({}));
    (bridge, desktop)
}

#[tokio::test]
async fn reads_and_writes_forward_exact_params_to_the_desktop() {
    let (mut bridge, desktop) = bridge_with_desktop().await;

    let world = bridge
        .request(
            "tools/call",
            json!({
                "name": "world_info",
                "arguments": { "project_id": "project-live", "world_revision": 9 }
            }),
        )
        .await;
    assert_eq!(tool_json(&world)["targetMinecraftVersion"], "1.21.4");

    let block = bridge
        .request(
            "tools/call",
            json!({
                "name": "get_block",
                "arguments": {
                    "project_id": "project-live",
                    "dimension": "minecraft:overworld",
                    "pos": { "x": 0, "y": 64, "z": 0 },
                    "world_revision": 9
                }
            }),
        )
        .await;
    let block = tool_json(&block);
    assert_eq!(block["type"], "block");
    assert_eq!(block["worldRevision"], 9);

    let region = bridge
        .request(
            "tools/call",
            json!({
                "name": "inspect_region",
                "arguments": {
                    "project_id": "project-live",
                    "dimension": "minecraft:overworld",
                    "bounds": {
                        "min": { "x": -1, "y": 63, "z": -1 },
                        "max": { "x": 1, "y": 65, "z": 1 }
                    },
                    "world_revision": 9
                }
            }),
        )
        .await;
    let region = tool_json(&region);
    assert_eq!(region["type"], "region_summary");
    assert_eq!(region["data"]["blocks"], 27);

    let camera = bridge
        .request(
            "tools/call",
            json!({
                "name": "get_camera",
                "arguments": { "project_id": "project-live" }
            }),
        )
        .await;
    assert_eq!(tool_json(&camera)["pose"]["fov"], 70);

    let preview = bridge
        .request(
            "tools/call",
            json!({
                "name": "preview_operation",
                "arguments": {
                    "project_id": "project-live",
                    "world_revision": 9,
                    "operation": {
                        "type": "generate_road",
                        "dimension": "minecraft:overworld",
                        "bounds": {
                            "min": { "x": 0, "y": 60, "z": 0 },
                            "max": { "x": 15, "y": 80, "z": 15 }
                        },
                        "from": { "x": 0, "y": 64, "z": 7 },
                        "to": { "x": 15, "y": 64, "z": 7 },
                        "surface": { "name": "minecraft:stone_bricks" }
                    }
                }
            }),
        )
        .await;
    assert!(!preview["result"]
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false));

    let applied = bridge
        .request(
            "tools/call",
            json!({
                "name": "apply_operations_batch",
                "arguments": {
                    "project_id": "project-live",
                    "world_revision": 9,
                    "dimension": "minecraft:overworld",
                    "changes": [{
                        "pos": { "x": 0, "y": 64, "z": 0 },
                        "state": { "name": "minecraft:stone" }
                    }],
                    "idempotency_key": "batch-1"
                }
            }),
        )
        .await;
    assert_eq!(tool_json(&applied)["operationId"], "op-1");

    let undone = bridge
        .request(
            "tools/call",
            json!({
                "name": "undo_operation",
                "arguments": {
                    "project_id": "project-live",
                    "world_revision": 10,
                    "operation_id": "op-1"
                }
            }),
        )
        .await;
    assert_eq!(tool_json(&undone)["worldRevision"], 11);

    let status = bridge
        .request(
            "tools/call",
            json!({
                "name": "get_operation_status",
                "arguments": { "project_id": "project-live", "operation_id": "op-1" }
            }),
        )
        .await;
    assert_eq!(tool_json(&status)["job"]["available"], false);

    let template = bridge
        .request(
            "tools/call",
            json!({
                "name": "insert_template",
                "arguments": {
                    "project_id": "project-live",
                    "world_revision": 10,
                    "template_id": "123e4567-e89b-12d3-a456-426614174000",
                    "dimension": "minecraft:overworld",
                    "origin": [0, 64, 0],
                    "idempotency_key": "template-1"
                }
            }),
        )
        .await;
    assert_eq!(tool_json(&template)["operationId"], "op-template");

    let requests = desktop.requests();
    let world_request = requests
        .iter()
        .find(|request| request["method"] == "world_info")
        .expect("world_info request");
    assert_eq!(world_request["params"]["projectId"], "project-live");
    assert_eq!(world_request["params"]["worldRevision"], 9);
    assert_eq!(world_request["token"], support::TOKEN);

    let block_request = requests
        .iter()
        .find(|request| {
            request["method"] == "query" && request["params"]["query"]["type"] == "get_block"
        })
        .expect("query request");
    assert_eq!(
        block_request["params"]["query"]["pos"],
        json!({ "x": 0, "y": 64, "z": 0 })
    );

    let preview_request = requests
        .iter()
        .find(|request| request["method"] == "preview_operation")
        .expect("preview request");
    assert_eq!(preview_request["params"]["worldRevision"], 9);
    assert_eq!(preview_request["params"]["operation"]["width"], 3);
    assert_eq!(preview_request["params"]["operation"]["meander"], 4);
    assert_eq!(preview_request["params"]["operation"]["clearance"], 2);
    assert_eq!(preview_request["params"]["operation"]["seed"], 0);
    assert_eq!(
        preview_request["params"]["operation"]["surface"]["properties"],
        json!({})
    );

    let apply_request = requests
        .iter()
        .find(|request| request["method"] == "apply_operation")
        .expect("apply request");
    assert_eq!(apply_request["params"]["worldRevision"], 9);
    assert_eq!(apply_request["params"]["idempotencyKey"], "batch-1");
    assert_eq!(apply_request["params"]["operation"]["type"], "batch");
    assert_eq!(
        apply_request["params"]["operation"]["changes"][0]["state"]["properties"],
        json!({})
    );

    let undo_request = requests
        .iter()
        .find(|request| request["method"] == "undo_operation")
        .expect("undo request");
    assert_eq!(undo_request["params"]["operationId"], "op-1");
    assert_eq!(undo_request["params"]["worldRevision"], 10);

    let template_request = requests
        .iter()
        .find(|request| request["method"] == "insert_template")
        .expect("insert template request");
    assert_eq!(
        template_request["params"]["templateId"],
        "123e4567-e89b-12d3-a456-426614174000"
    );
    assert_eq!(template_request["params"]["origin"], json!([0, 64, 0]));
    assert_eq!(template_request["params"]["includeAir"], false);
}

#[tokio::test]
async fn capture_and_observation_return_real_png_blocks_and_digests() {
    let (mut bridge, desktop) = bridge_with_desktop().await;

    let capture = bridge
        .request(
            "tools/call",
            json!({
                "name": "capture_viewport",
                "arguments": {
                    "project_id": "project-live",
                    "dimension": "minecraft:overworld",
                    "world_revision": 9
                }
            }),
        )
        .await;
    let content = capture["result"]["content"].as_array().expect("content");
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["mimeType"], "image/png");
    assert_eq!(content[0]["data"], "iVBORw0KGgo=");
    let metadata: Value =
        serde_json::from_str(content[1]["text"].as_str().expect("text")).expect("capture metadata");
    assert_eq!(metadata["metadata"]["partial"], true);
    assert_eq!(metadata["verification"]["partial"], true);
    assert_eq!(metadata["image"]["bytes"], 8);
    assert_eq!(metadata["image"]["sha256"].as_str().expect("sha").len(), 64);

    let observed = bridge
        .request(
            "tools/call",
            json!({
                "name": "observe",
                "arguments": {
                    "project_id": "project-live",
                    "dimension": "minecraft:overworld",
                    "bounds": {
                        "min": { "x": 0, "y": 60, "z": 0 },
                        "max": { "x": 15, "y": 80, "z": 15 }
                    },
                    "mode": "top_down"
                }
            }),
        )
        .await;
    let content = observed["result"]["content"].as_array().expect("content");
    assert_eq!(content[0]["type"], "image");
    let metadata: Value = serde_json::from_str(content[1]["text"].as_str().expect("text"))
        .expect("observation metadata");
    assert_eq!(metadata["mode"], "top_down");
    assert_eq!(metadata["worldRevision"], 9);
    assert_eq!(metadata["image"]["bytes"], 8);

    let requests = desktop.requests();
    let capture_request = requests
        .iter()
        .find(|request| request["method"] == "capture_viewport")
        .expect("capture request");
    assert_eq!(
        capture_request["params"]["dimension"],
        "minecraft:overworld"
    );
    let observe_request = requests
        .iter()
        .find(|request| request["method"] == "observe")
        .expect("observe request");
    assert_eq!(observe_request["params"]["observation"]["mode"], "top_down");
    assert_eq!(
        observe_request["params"]["observation"]["bounds"]["max"]["x"],
        15
    );
}

#[tokio::test]
async fn job_tools_and_failures_are_transparent() {
    let (mut bridge, desktop) = bridge_with_desktop().await;

    let started = bridge
        .request(
            "tools/call",
            json!({
                "name": "start_job",
                "arguments": {
                    "project_id": "project-live",
                    "kind": "apply_operation",
                    "parameters": {
                        "operation": {
                            "type": "set_block",
                            "dimension": "minecraft:overworld",
                            "pos": { "x": 1, "y": 64, "z": 1 },
                            "state": { "name": "minecraft:stone" }
                        }
                    },
                    "idempotency_key": "job-key-1",
                    "world_revision": 9
                }
            }),
        )
        .await;
    assert_eq!(tool_json(&started)["jobId"], "job-1");

    let status = bridge
        .request(
            "tools/call",
            json!({
                "name": "get_job_status",
                "arguments": { "project_id": "project-live", "job_id": "job-1" }
            }),
        )
        .await;
    assert_eq!(tool_json(&status)["state"], "running");

    let cancelled = bridge
        .request(
            "tools/call",
            json!({
                "name": "cancel_job",
                "arguments": { "project_id": "project-live", "job_id": "job-1" }
            }),
        )
        .await;
    assert_eq!(tool_json(&cancelled)["cancelRequested"], true);

    let denied = bridge
        .request(
            "tools/call",
            json!({
                "name": "start_job",
                "arguments": {
                    "project_id": "project-live",
                    "kind": "undo_operation",
                    "parameters": {},
                    "idempotency_key": "job-denied",
                    "world_revision": 10
                }
            }),
        )
        .await;
    assert_eq!(denied["result"]["isError"], true);
    assert!(result_text(&denied).starts_with("permission_denied: "));

    let missing_revision = bridge
        .request(
            "tools/call",
            json!({
                "name": "apply_operations_batch",
                "arguments": {
                    "project_id": "project-live",
                    "dimension": "minecraft:overworld",
                    "changes": [{
                        "pos": { "x": 0, "y": 64, "z": 0 },
                        "state": { "name": "minecraft:stone" }
                    }],
                    "idempotency_key": "batch-1"
                }
            }),
        )
        .await;
    assert_eq!(missing_revision["result"]["isError"], true);
    assert!(result_text(&missing_revision).starts_with("bridge_error: world_revision is required"));

    let requests = desktop.requests();
    let start = requests
        .iter()
        .find(|request| request["method"] == "start_job")
        .expect("start job");
    assert_eq!(start["params"]["projectId"], "project-live");
    assert_eq!(start["params"]["kind"], "apply_operation");
    assert_eq!(start["params"]["idempotencyKey"], "job-key-1");
    assert_eq!(start["params"]["worldRevision"], 9);
    assert_eq!(
        start["params"]["parameters"]["operation"]["type"],
        "set_block"
    );
}

#[tokio::test]
async fn desktop_errors_are_reported_with_their_authoritative_code() {
    let handler = std::sync::Arc::new(|request: Value| {
        support::failed(
            &request,
            "permission_denied",
            "writes are disabled for this project",
        )
    });
    let desktop = MockDesktop::start(handler).await;
    let mut bridge = BridgeProcess::spawn(&desktop.runtime_path);
    bridge.initialize_legacy().await;
    bridge.notify("notifications/initialized", json!({}));

    let response = bridge
        .request(
            "tools/call",
            json!({
                "name": "preview_operation",
                "arguments": {
                    "project_id": "project-live",
                    "operation": {
                        "type": "set_block",
                        "dimension": "minecraft:overworld",
                        "pos": { "x": 0, "y": 64, "z": 0 },
                        "state": { "name": "minecraft:stone" }
                    }
                }
            }),
        )
        .await;
    assert_eq!(response["result"]["isError"], true);
    assert_eq!(
        result_text(&response),
        "permission_denied: writes are disabled for this project"
    );
}
