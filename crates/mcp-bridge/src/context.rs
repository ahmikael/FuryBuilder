//! Explicit request context for every MCP tool call.
//!
//! FuryBuilder never stores a project, selection or world revision in the transport
//! connection. Callers name the project they intend to inspect, and revision-sensitive
//! calls may also pin the revision they observed. A stale revision is a conflict, never a
//! silent read of newer data.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestedContext {
    pub project_id: String,
    pub world_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveContext {
    pub project_id: String,
    pub world_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextState {
    Ready,
    DesktopUnavailable,
    ProjectMismatch,
    RevisionMismatch,
}

impl ContextState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::DesktopUnavailable => "desktop_unavailable",
            Self::ProjectMismatch => "project_mismatch",
            Self::RevisionMismatch => "revision_mismatch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextEvaluation {
    pub state: ContextState,
    pub valid: bool,
    pub code: ContextState,
    pub message: String,
    pub requested: RequestedContext,
    pub active: Option<ActiveContext>,
}

impl ContextEvaluation {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("context evaluation is serializable")
    }
}

pub fn evaluate_context(
    requested: RequestedContext,
    active: Option<ActiveContext>,
) -> ContextEvaluation {
    let Some(active) = active else {
        return ContextEvaluation {
            state: ContextState::DesktopUnavailable,
            valid: false,
            code: ContextState::DesktopUnavailable,
            message: "The FuryBuilder desktop app is not connected; no world context is available."
                .to_owned(),
            requested,
            active: None,
        };
    };
    if requested.project_id != active.project_id {
        return ContextEvaluation {
            state: ContextState::ProjectMismatch,
            valid: false,
            code: ContextState::ProjectMismatch,
            message: "project_id does not match the project currently open in FuryBuilder."
                .to_owned(),
            requested,
            active: Some(active),
        };
    }
    if requested
        .world_revision
        .is_some_and(|revision| revision != active.world_revision)
    {
        let revision = requested.world_revision.expect("checked above");
        return ContextEvaluation {
            state: ContextState::RevisionMismatch,
            valid: false,
            code: ContextState::RevisionMismatch,
            message: format!(
                "Requested world revision {revision}, but the open world is at {}.",
                active.world_revision
            ),
            requested,
            active: Some(active),
        };
    }
    ContextEvaluation {
        state: ContextState::Ready,
        valid: true,
        code: ContextState::Ready,
        message: "Project and world revision match the open editor.".to_owned(),
        requested,
        active: Some(active),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requested(project: &str, revision: Option<u64>) -> RequestedContext {
        RequestedContext {
            project_id: project.to_owned(),
            world_revision: revision,
        }
    }

    fn active(project: &str, revision: u64) -> ActiveContext {
        ActiveContext {
            project_id: project.to_owned(),
            world_revision: revision,
        }
    }

    #[test]
    fn reports_desktop_unavailability_without_inventing_context() {
        let evaluation = evaluate_context(requested("p", Some(1)), None);
        assert_eq!(evaluation.state, ContextState::DesktopUnavailable);
        assert!(!evaluation.valid);
        assert!(evaluation.active.is_none());
    }

    #[test]
    fn detects_project_and_revision_mismatches() {
        assert_eq!(
            evaluate_context(requested("other", None), Some(active("p", 4))).state,
            ContextState::ProjectMismatch
        );
        assert_eq!(
            evaluate_context(requested("p", Some(3)), Some(active("p", 4))).state,
            ContextState::RevisionMismatch
        );
        assert!(evaluate_context(requested("p", Some(4)), Some(active("p", 4))).valid);
        assert!(evaluate_context(requested("p", None), Some(active("p", 4))).valid);
    }
}
