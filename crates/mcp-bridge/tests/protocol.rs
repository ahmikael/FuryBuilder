mod support;

use serde_json::{json, Value};
use support::{result_text, tool_json, BridgeProcess, MockDesktop};

fn missing_runtime() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "furybuilder-mcp-no-runtime-{}.json",
        uuid::Uuid::new_v4().simple()
    ))
}

fn tool_names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

#[tokio::test]
async fn legacy_clients_get_the_full_catalog_and_honest_failures() {
    let mut bridge = BridgeProcess::spawn(&missing_runtime());
    let initialize = bridge.initialize_legacy().await;
    assert_eq!(initialize["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(initialize["result"]["serverInfo"]["name"], "furybuilder");
    assert!(initialize["result"]["instructions"]
        .as_str()
        .expect("instructions")
        .contains("project_id"));
    bridge.notify("notifications/initialized", json!({}));

    let tools = bridge.request("tools/list", json!({})).await;
    let tools = tools["result"]["tools"].as_array().expect("tools").clone();
    let names = tool_names(&tools);
    assert_eq!(names.len(), 27, "unexpected tool catalog: {names:?}");
    for expected in [
        "bridge_info",
        "world_info",
        "get_block",
        "observe",
        "capture_viewport",
        "validate_region",
        "preview_operation",
        "apply_operation",
        "apply_operations_batch",
        "insert_template",
        "undo_operation",
        "get_operation_status",
        "start_job",
        "get_job_status",
        "cancel_job",
    ] {
        assert!(names.contains(&expected.to_owned()), "missing {expected}");
    }
    let observe = tools
        .iter()
        .find(|tool| tool["name"] == "observe")
        .expect("observe");
    assert_eq!(
        observe["_meta"],
        json!({
            "ui": {
                "resourceUri": "ui://furybuilder/observation",
                "visibility": ["model", "app"]
            }
        })
    );
    assert_eq!(observe["annotations"]["readOnlyHint"], true);
    let start_job = tools
        .iter()
        .find(|tool| tool["name"] == "start_job")
        .expect("start_job");
    let schema = start_job["inputSchema"].to_string();
    assert!(schema.contains("world_revision"));
    assert!(schema.contains("idempotency_key"));
    let preview = tools
        .iter()
        .find(|tool| tool["name"] == "preview_operation")
        .expect("preview");
    let preview_schema = preview["inputSchema"].to_string();
    for generator in [
        "set_biome",
        "generate_road",
        "generate_river",
        "generate_vegetation",
    ] {
        assert!(preview_schema.contains(generator), "missing {generator}");
    }

    let info = bridge
        .request(
            "tools/call",
            json!({
                "name": "bridge_info",
                "arguments": { "project_id": "project-legacy", "world_revision": 12 }
            }),
        )
        .await;
    let info = tool_json(&info);
    assert_eq!(info["request"]["protocolEra"], "legacy");
    assert_eq!(info["request"]["protocolVersion"], Value::Null);
    assert_eq!(info["request"]["projectId"], "project-legacy");
    assert_eq!(
        info["bridge"]["optionalExtensions"],
        json!({ "tasks": false, "apps": true })
    );
    assert_eq!(info["bridge"]["protocolTarget"], "2026-07-28");
    assert_eq!(info["bridge"]["contextPolicy"], "explicit-per-request");
    assert_eq!(
        info["bridge"]["jobs"]["fallbackTools"],
        json!(["start_job", "get_job_status", "cancel_job"])
    );
    assert_eq!(
        info["bridge"]["jobs"]["tasks"]["reason"],
        "client_did_not_declare_tasks"
    );
    assert_eq!(info["bridge"]["desktop"]["connected"], false);
    assert_eq!(info["bridge"]["desktop"]["reason"], "runtime_file_missing");
    assert_eq!(info["bridge"]["connection"]["state"], "desktop_unavailable");
    assert_eq!(info["bridge"]["context"]["state"], "desktop_unavailable");
    assert_eq!(
        info["bridge"]["workflow"]["stages"],
        json!([
            "inspect",
            "plan",
            "checkpoint",
            "execute",
            "verify",
            "report"
        ])
    );
    assert_eq!(info["bridge"]["workflow"]["limits"]["batchChanges"], 8_192);
    assert_eq!(
        info["bridge"]["apps"]["reason"],
        "client_did_not_declare_apps"
    );

    let resources = bridge.request("resources/list", json!({})).await;
    let resources = resources["result"]["resources"]
        .as_array()
        .expect("resources");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["uri"], "ui://furybuilder/observation");
    assert_eq!(resources[0]["mimeType"], "text/html;profile=mcp-app");
    let read = bridge
        .request(
            "resources/read",
            json!({ "uri": "ui://furybuilder/observation" }),
        )
        .await;
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("resource text");
    assert_eq!(text, mcp_bridge::apps::OBSERVATION_APP_HTML.as_str());
    let unknown = bridge
        .request(
            "resources/read",
            json!({ "uri": "ui://furybuilder/unknown" }),
        )
        .await;
    assert!(unknown.get("error").is_some());

    let prompts = bridge.request("prompts/list", json!({})).await;
    let prompts = prompts["result"]["prompts"].as_array().expect("prompts");
    assert_eq!(prompts.len(), 2);
    let plan = bridge
        .request(
            "prompts/get",
            json!({
                "name": "plan_complex_build",
                "arguments": { "project_id": "project-legacy", "goal": "Build a tower" }
            }),
        )
        .await;
    let plan_text = plan["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("plan text");
    assert!(plan_text.contains("Project: project-legacy"));
    assert!(plan_text.contains("Do not write before stages 1-3"));
    let report = bridge
        .request(
            "prompts/get",
            json!({
                "name": "report_complex_build",
                "arguments": {
                    "project_id": "project-legacy",
                    "goal": "Build a tower",
                    "outcome": "failed"
                }
            }),
        )
        .await;
    assert!(report["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("report text")
        .contains("Outcome: failed"));

    let missing_project = bridge
        .request(
            "tools/call",
            json!({ "name": "bridge_info", "arguments": {} }),
        )
        .await;
    assert_eq!(missing_project["result"]["isError"], true);

    let missing_desktop = bridge
        .request(
            "tools/call",
            json!({ "name": "world_info", "arguments": { "project_id": "project-legacy" } }),
        )
        .await;
    assert_eq!(missing_desktop["result"]["isError"], true);
    assert!(result_text(&missing_desktop)
        .starts_with("runtime_file_missing: FuryBuilder is not running"));

    let job = bridge
        .request(
            "tools/call",
            json!({
                "name": "start_job",
                "arguments": {
                    "project_id": "project-legacy",
                    "kind": "structure_export",
                    "parameters": {},
                    "idempotency_key": "job-1"
                }
            }),
        )
        .await;
    assert_eq!(job["result"]["isError"], true);
    assert!(result_text(&job).starts_with("job_api_unavailable:"));
}

#[tokio::test]
async fn modern_clients_negotiate_2026_07_28_and_apps() {
    let mut bridge = BridgeProcess::spawn(&missing_runtime());
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {
            "extensions": {
                "io.modelcontextprotocol/ui": {
                    "mimeTypes": ["text/html;profile=mcp-app"]
                }
            }
        }
    });
    let discover = bridge
        .request_with_meta("server/discover", json!({}), Some(meta.clone()))
        .await;
    let result = &discover["result"];
    assert!(result["supportedVersions"]
        .as_array()
        .expect("versions")
        .contains(&json!("2026-07-28")));
    assert_eq!(
        result["capabilities"]["extensions"]["io.modelcontextprotocol/ui"]["mimeTypes"],
        json!(["text/html;profile=mcp-app"])
    );

    let tools = bridge
        .request_with_meta("tools/list", json!({}), Some(meta.clone()))
        .await;
    assert_eq!(
        tools["result"]["tools"].as_array().expect("tools").len(),
        27
    );

    let info = bridge
        .request_with_meta(
            "tools/call",
            json!({
                "name": "bridge_info",
                "arguments": { "project_id": "project-modern", "world_revision": 12 }
            }),
            Some(meta),
        )
        .await;
    let info = tool_json(&info);
    assert_eq!(info["request"]["protocolEra"], "modern");
    assert_eq!(info["request"]["protocolVersion"], "2026-07-28");
    assert_eq!(info["bridge"]["apps"]["mode"], "apps");
    assert_eq!(info["bridge"]["apps"]["reason"], "apps_negotiated");
    assert_eq!(info["bridge"]["apps"]["extensionAdvertised"], true);
    assert_eq!(
        info["bridge"]["apps"]["fallback"],
        json!({
            "tools": ["observe", "capture_viewport"],
            "content": ["image/png", "application/json"]
        })
    );
    assert_eq!(info["bridge"]["context"]["requested"]["worldRevision"], 12);
    assert_eq!(
        info["request"]["clientCapabilities"]["extensions"]["io.modelcontextprotocol/ui"]
            ["mimeTypes"],
        json!(["text/html;profile=mcp-app"])
    );
}

#[tokio::test]
async fn live_desktop_context_and_apps_are_reported() {
    let handler = std::sync::Arc::new(|request: Value| {
        support::ok(
            &request,
            json!({
                "app": { "name": "FuryBuilder", "version": "0.1.0" },
                "project": {
                    "projectId": "project-live",
                    "name": "Harbour",
                    "worldRevision": 9
                },
                "bridge": { "protocolVersion": 1, "transport": "local_socket", "pid": 4242 },
                "mcp": {
                    "mode": "edit_in_selection",
                    "writesEnabled": true,
                    "maxBlocks": 4000,
                    "maxBatchChanges": 100,
                    "requireConfirmation": true
                },
                "renderer": { "listening": true },
                "jobs": {
                    "available": true,
                    "queued": 1,
                    "running": 0,
                    "succeeded": 2,
                    "failed": 0,
                    "cancelled": 0,
                    "retained": 3,
                    "maxQueued": 16
                },
                "pendingConfirmations": 1
            }),
        )
    });
    let desktop = MockDesktop::start(handler).await;
    let mut bridge = BridgeProcess::spawn(&desktop.runtime_path);
    bridge.initialize_legacy().await;
    bridge.notify("notifications/initialized", json!({}));

    let info = bridge
        .request(
            "tools/call",
            json!({
                "name": "bridge_info",
                "arguments": { "project_id": "project-live", "world_revision": 9 }
            }),
        )
        .await;
    let info = tool_json(&info);
    assert_eq!(info["bridge"]["desktop"]["connected"], true);
    assert_eq!(
        info["bridge"]["desktop"]["project"]["projectId"],
        "project-live"
    );
    assert_eq!(info["bridge"]["desktop"]["project"]["worldRevision"], 9);
    assert_eq!(
        info["bridge"]["desktop"]["access"]["mode"],
        "edit_in_selection"
    );
    assert_eq!(info["bridge"]["desktop"]["access"]["maxBlocks"], 4000);
    assert_eq!(info["bridge"]["desktop"]["renderer"]["listening"], true);
    assert_eq!(info["bridge"]["desktop"]["jobs"]["succeeded"], 2);
    assert_eq!(info["bridge"]["desktop"]["pendingConfirmations"], 1);
    assert_eq!(info["bridge"]["jobs"]["backendConnected"], true);
    assert_eq!(info["bridge"]["connection"]["state"], "ready");
    assert_eq!(info["bridge"]["context"]["state"], "ready");
    assert_eq!(info["bridge"]["context"]["valid"], true);
    assert_eq!(
        info["bridge"]["context"]["active"],
        json!({ "projectId": "project-live", "worldRevision": 9 })
    );

    let stale = bridge
        .request(
            "tools/call",
            json!({
                "name": "bridge_info",
                "arguments": { "project_id": "project-live", "world_revision": 8 }
            }),
        )
        .await;
    let stale = tool_json(&stale);
    assert_eq!(stale["bridge"]["connection"]["state"], "revision_mismatch");
    assert_eq!(stale["bridge"]["context"]["state"], "revision_mismatch");

    let requests = desktop.requests();
    let ping = requests
        .iter()
        .find(|request| request["method"] == "ping")
        .expect("ping request");
    assert_eq!(ping["token"], support::TOKEN);
    assert_eq!(ping["protocol"], 1);
    assert!(ping["clientId"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(
        ping["clientId"].as_str(),
        Some(
            info["bridge"]["connection"]["instanceId"]
                .as_str()
                .expect("instance id")
        )
    );
}
