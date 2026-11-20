//! Protocol and request-context facts shared by the MCP bridge and its tests.

use serde_json::{json, Value};

pub const BRIDGE_NAME: &str = "furybuilder";
pub const BRIDGE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";

/// Optional protocol extensions are not advertised until they are implemented and tested.
/// Clients keep working through ordinary tools when they are absent.
pub fn optional_extensions() -> Value {
    json!({ "tasks": false, "apps": true })
}

/// Tasks stays an explicit option: the wire vocabulary is only advertised after the bridge
/// implements it and the authoritative Job API is connected. Until then every call uses
/// the ordinary start/status/cancel tools, which never invent a job or a result.
pub const BRIDGE_IMPLEMENTS_TASKS: bool = false;

/// MCP Apps is implemented only as an optional observation preview. The `observe` tool
/// always returns the ordinary PNG image and JSON metadata, so clients without the
/// extension keep working unchanged.
pub const BRIDGE_IMPLEMENTS_APPS: bool = true;

pub const BRIDGE_INSTRUCTIONS: &str = concat!(
    "FuryBuilder is a local Minecraft Java world editor. ",
    "Every call must identify its project explicitly with project_id. ",
    "The bridge never keeps a project, selection or job in the transport session. ",
    "Pass expected_revision for revision-sensitive reads and writes when the tool supports it. ",
    "For a multi-step build, load the plan_complex_build prompt: it defines planning, checkpoints, verification and reporting. ",
    "Writes are controlled by the desktop's MCP access policy and may wait for explicit user approval. ",
    "This bridge does not expose arbitrary filesystem or shell access.",
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolEra {
    Modern,
    Legacy,
}

impl ProtocolEra {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Modern => "modern",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TasksSupportDecision {
    pub mode: &'static str,
    pub reason: &'static str,
    pub extension_advertised: bool,
}

impl TasksSupportDecision {
    pub fn to_json(&self) -> Value {
        json!({
            "mode": self.mode,
            "reason": self.reason,
            "extensionAdvertised": self.extension_advertised,
        })
    }
}

/// Decides whether a caller may use the optional Tasks extension. All three conditions are
/// required, and the fallback is a normal result rather than an error: absence of Tasks is
/// not a failed request.
pub fn resolve_tasks_support(
    client_declares_tasks: bool,
    job_backend_connected: bool,
    bridge_implements_tasks: bool,
) -> TasksSupportDecision {
    if !client_declares_tasks {
        return TasksSupportDecision {
            mode: "fallback",
            reason: "client_did_not_declare_tasks",
            extension_advertised: false,
        };
    }
    if !bridge_implements_tasks {
        return TasksSupportDecision {
            mode: "fallback",
            reason: "tasks_not_implemented",
            extension_advertised: false,
        };
    }
    if !job_backend_connected {
        return TasksSupportDecision {
            mode: "fallback",
            reason: "job_backend_unavailable",
            extension_advertised: false,
        };
    }
    TasksSupportDecision {
        mode: "tasks",
        reason: "tasks_negotiated",
        extension_advertised: true,
    }
}

/// Rejects empty or unbounded project identifiers before a tool reaches the Core.
pub fn require_project_id(value: &str) -> Result<String, String> {
    let project_id = value.trim();
    if project_id.is_empty() || project_id.len() > 128 {
        return Err("project_id must be between 1 and 128 characters".to_owned());
    }
    Ok(project_id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_require_client_support_implementation_and_a_backend() {
        assert_eq!(
            resolve_tasks_support(false, true, true).reason,
            "client_did_not_declare_tasks"
        );
        assert_eq!(
            resolve_tasks_support(true, true, false).reason,
            "tasks_not_implemented"
        );
        assert_eq!(
            resolve_tasks_support(true, false, true).reason,
            "job_backend_unavailable"
        );
        let negotiated = resolve_tasks_support(true, true, true);
        assert_eq!(negotiated.mode, "tasks");
        assert!(negotiated.extension_advertised);
    }

    #[test]
    fn project_ids_are_trimmed_and_bounded() {
        assert_eq!(require_project_id("  project  ").unwrap(), "project");
        assert!(require_project_id("   ").is_err());
        assert!(require_project_id(&"x".repeat(129)).is_err());
    }
}
