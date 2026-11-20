//! MCP server assembly.
//!
//! `rmcp` negotiates the protocol era (`2026-07-28` inline lifecycle or a legacy
//! `initialize` handshake) before this handler is called. The handler is stateless apart
//! from its per-process identity: every tool call carries its own project and revision.

use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, GetPromptRequestParams, GetPromptResponse,
    GetPromptResult, Implementation, InitializeResult, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParams, PromptMessage, ProtocolVersion,
    ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Role, ServerCapabilities,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{json, Map, Value};

use crate::apps::{
    observation_resource, observation_resource_contents, APPS_EXTENSION_ID,
    APPS_RESOURCE_MIME_TYPE, OBSERVATION_APP_URI,
};
use crate::capabilities::{ProtocolEra, BRIDGE_INSTRUCTIONS, BRIDGE_NAME, BRIDGE_VERSION};
use crate::prompts::{
    build_plan_prompt, build_report_prompt, prompt_definitions, PlanArgs, ReportArgs,
    PLAN_PROMPT_NAME, REPORT_PROMPT_NAME,
};
use crate::tools::{self, RequestInfo, ToolContext};

#[derive(Debug, Clone)]
pub struct FuryBuilderServer {
    instance_id: String,
    started_at_ms: u64,
}

impl Default for FuryBuilderServer {
    fn default() -> Self {
        Self::new()
    }
}

impl FuryBuilderServer {
    pub fn new() -> Self {
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        Self {
            instance_id: uuid::Uuid::new_v4().to_string(),
            started_at_ms,
        }
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    fn tool_context(&self) -> ToolContext {
        ToolContext {
            client_id: self.instance_id.clone(),
            started_at_ms: self.started_at_ms,
            runtime_path: None,
        }
    }

    fn capabilities() -> ServerCapabilities {
        let mut extensions = rmcp::model::ExtensionCapabilities::new();
        extensions.insert(
            APPS_EXTENSION_ID.to_owned(),
            json!({ "mimeTypes": [APPS_RESOURCE_MIME_TYPE] })
                .as_object()
                .cloned()
                .expect("apps settings"),
        );
        ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_prompts()
            .enable_extensions_with(extensions)
            .build()
    }
}

fn protocol_era(context: &RequestContext<RoleServer>) -> ProtocolEra {
    if context.meta.protocol_version().is_some() {
        ProtocolEra::Modern
    } else {
        ProtocolEra::Legacy
    }
}

fn request_info(context: &RequestContext<RoleServer>) -> RequestInfo {
    let era = protocol_era(context);
    let protocol_version = match era {
        ProtocolEra::Modern => context
            .protocol_version()
            .map(|version| version.as_str().to_owned()),
        ProtocolEra::Legacy => None,
    };
    RequestInfo {
        era,
        protocol_version,
        client_capabilities: context.client_capabilities(),
    }
}

fn arguments_map(arguments: Option<Map<String, Value>>) -> Map<String, Value> {
    arguments.unwrap_or_default()
}

fn required_prompt_string(
    arguments: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<String, McpError> {
    let value = arguments.get(key).and_then(Value::as_str).ok_or_else(|| {
        McpError::invalid_params(format!("prompt argument `{key}` is required"), None)
    })?;
    if value.is_empty() || value.len() > max {
        return Err(McpError::invalid_params(
            format!("prompt argument `{key}` must be between 1 and {max} characters"),
            None,
        ));
    }
    Ok(value.to_owned())
}

fn optional_prompt_string(
    arguments: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<String>, McpError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let value = value.as_str().ok_or_else(|| {
                McpError::invalid_params(format!("prompt argument `{key}` must be a string"), None)
            })?;
            if value.len() > max {
                return Err(McpError::invalid_params(
                    format!("prompt argument `{key}` must be at most {max} characters"),
                    None,
                ));
            }
            Ok(Some(value.to_owned()))
        }
    }
}

impl ServerHandler for FuryBuilderServer {
    fn get_info(&self) -> InitializeResult {
        let mut info = InitializeResult::new(Self::capabilities());
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        info.server_info = Implementation::new(BRIDGE_NAME, BRIDGE_VERSION);
        info.instructions = Some(BRIDGE_INSTRUCTIONS.to_owned());
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(tools::tool_definitions()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let result = tools::call_tool(
            &request.name,
            Some(arguments_map(request.arguments)),
            &self.tool_context(),
            &request_info(&context),
        )
        .await?;
        Ok(CallToolResponse::Complete(result))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(vec![
            observation_resource(),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri != OBSERVATION_APP_URI {
            return Err(McpError::resource_not_found(
                format!("unknown resource: {}", request.uri),
                None,
            ));
        }
        Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
            vec![observation_resource_contents(&request.uri)],
        )))
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompt_definitions()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let arguments = arguments_map(request.arguments);
        let result = match request.name.as_str() {
            PLAN_PROMPT_NAME => {
                let plan = PlanArgs {
                    project_id: required_prompt_string(&arguments, "project_id", 128)?,
                    goal: required_prompt_string(&arguments, "goal", 2_000)?,
                    bounds: optional_prompt_string(&arguments, "bounds", 200)?,
                    constraints: optional_prompt_string(&arguments, "constraints", 1_000)?,
                };
                GetPromptResult::new(vec![PromptMessage::new_text(
                    Role::User,
                    build_plan_prompt(&plan),
                )])
                .with_description("FuryBuilder complex build playbook")
            }
            REPORT_PROMPT_NAME => {
                let report = ReportArgs {
                    project_id: required_prompt_string(&arguments, "project_id", 128)?,
                    goal: required_prompt_string(&arguments, "goal", 2_000)?,
                    outcome: required_prompt_string(&arguments, "outcome", 32)?,
                    notes: optional_prompt_string(&arguments, "notes", 2_000)?,
                };
                if !["succeeded", "partially_succeeded", "failed", "cancelled"]
                    .contains(&report.outcome.as_str())
                {
                    return Err(McpError::invalid_params(
                        "prompt argument `outcome` must be succeeded, partially_succeeded, failed or cancelled",
                        None,
                    ));
                }
                GetPromptResult::new(vec![PromptMessage::new_text(
                    Role::User,
                    build_report_prompt(&report),
                )])
                .with_description("FuryBuilder complex task report template")
            }
            other => {
                return Err(McpError::invalid_params(
                    format!("unknown prompt `{other}`"),
                    None,
                ));
            }
        };
        Ok(GetPromptResponse::Complete(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_tools_resources_prompts_and_apps() {
        let server = FuryBuilderServer::new();
        let info = server.get_info();
        assert_eq!(info.server_info.name, BRIDGE_NAME);
        assert_eq!(info.server_info.version, BRIDGE_VERSION);
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.resources.is_some());
        assert!(info.capabilities.prompts.is_some());
        let extensions = info.capabilities.extensions.expect("extensions");
        assert!(extensions.contains_key(APPS_EXTENSION_ID));
        assert!(!server.instance_id().is_empty());
    }

    #[test]
    fn exposes_the_observation_resource() {
        let resource = observation_resource();
        assert_eq!(resource.uri, OBSERVATION_APP_URI);
        assert_eq!(resource.mime_type.as_deref(), Some(APPS_RESOURCE_MIME_TYPE));
    }
}
