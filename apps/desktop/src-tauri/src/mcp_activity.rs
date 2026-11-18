//! Bounded, in-memory journal of authenticated MCP requests.
//!
//! The journal is intentionally separate from the authoritative world history: it records
//! tool-call metadata (including denied and failed calls) and links successful writes to
//! their `operation_id`. Full operation payloads are never duplicated here, and a broken
//! journal must never break a tool call.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use world_application::ErrorCode;

pub const MAX_MCP_ACTIVITY_ENTRIES: usize = 1_000;
pub const MAX_MCP_ACTIVITY_QUERY: usize = 100;
pub const DEFAULT_MCP_ACTIVITY_QUERY: usize = 25;
pub const MAX_MCP_ACTIVITY_OPERATION_LINK: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpActivityStatus {
    Ok,
    Denied,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpActivityEntry {
    pub sequence: u64,
    pub request_id: String,
    pub client_id: Option<String>,
    pub method: String,
    pub status: McpActivityStatus,
    pub error_code: Option<ErrorCode>,
    pub project_id: Option<String>,
    pub requested_revision: Option<u64>,
    pub operation_id: Option<String>,
    pub changed_blocks: Option<u64>,
    pub confirmation: Option<String>,
    pub timestamp_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Default)]
struct ActivityState {
    next_sequence: u64,
    entries: VecDeque<McpActivityEntry>,
}

/// Per-process journal shared by the local endpoint and the editor UI.
#[derive(Debug, Default)]
pub struct McpActivityLog {
    state: Mutex<ActivityState>,
}

impl McpActivityLog {
    /// Assigns a sequence number and appends the entry, evicting the oldest records.
    /// Errors are ignored on purpose: journaling must not turn a successful write into a
    /// failed tool call.
    pub fn record(&self, mut entry: McpActivityEntry) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        state.next_sequence += 1;
        entry.sequence = state.next_sequence;
        state.entries.push_back(entry);
        while state.entries.len() > MAX_MCP_ACTIVITY_ENTRIES {
            state.entries.pop_front();
        }
    }

    /// Newest first, optionally narrowed to one operation. `limit` is clamped by callers.
    pub fn query(
        &self,
        project_id: &str,
        operation_id: Option<&str>,
        limit: usize,
    ) -> Vec<McpActivityEntry> {
        let Ok(state) = self.state.lock() else {
            return Vec::new();
        };
        state
            .entries
            .iter()
            .rev()
            .filter(|entry| entry.project_id.as_deref() == Some(project_id))
            .filter(|entry| {
                operation_id
                    .map(|expected| entry.operation_id.as_deref() == Some(expected))
                    .unwrap_or(true)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// Entries that name this operation, newest first. Used to bind a tool call to the
    /// authoritative operation status without exposing the whole journal.
    pub fn linked_operation(&self, project_id: &str, operation_id: &str) -> Vec<McpActivityEntry> {
        self.query(
            project_id,
            Some(operation_id),
            MAX_MCP_ACTIVITY_OPERATION_LINK,
        )
    }

    pub fn len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.entries.len())
            .unwrap_or(0)
    }
}

/// Removes control characters and caps journal fields that came from a client.
pub fn clean_activity_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(request_id: &str, project_id: &str, operation_id: Option<&str>) -> McpActivityEntry {
        McpActivityEntry {
            sequence: 0,
            request_id: request_id.to_owned(),
            client_id: Some("bridge-1".to_owned()),
            method: "apply_operation".to_owned(),
            status: McpActivityStatus::Ok,
            error_code: None,
            project_id: Some(project_id.to_owned()),
            requested_revision: Some(4),
            operation_id: operation_id.map(str::to_owned),
            changed_blocks: Some(3),
            confirmation: Some("approved".to_owned()),
            timestamp_ms: 100,
            duration_ms: 2,
        }
    }

    #[test]
    fn assigns_sequence_and_filters_by_project_and_operation() {
        let log = McpActivityLog::default();
        log.record(entry("r1", "project-a", Some("op-1")));
        log.record(entry("r2", "project-b", Some("op-2")));
        log.record(entry("r3", "project-a", Some("op-3")));

        let all = log.query("project-a", None, 10);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].request_id, "r3");
        assert_eq!(all[1].request_id, "r1");
        assert!(all[0].sequence > all[1].sequence);

        let linked = log.linked_operation("project-a", "op-1");
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].request_id, "r1");
        assert!(log.query("project-c", None, 10).is_empty());
    }

    #[test]
    fn journal_is_bounded_without_losing_order() {
        let log = McpActivityLog::default();
        for index in 0..(MAX_MCP_ACTIVITY_ENTRIES + 5) {
            log.record(entry(&format!("r{index}"), "project-a", None));
        }
        assert_eq!(log.len(), MAX_MCP_ACTIVITY_ENTRIES);
        let recent = log.query("project-a", None, 2);
        assert_eq!(
            recent[0].request_id,
            format!("r{}", MAX_MCP_ACTIVITY_ENTRIES + 4)
        );
        assert_eq!(
            recent[1].request_id,
            format!("r{}", MAX_MCP_ACTIVITY_ENTRIES + 3)
        );
    }

    #[test]
    fn text_is_cleaned_and_clipped() {
        assert_eq!(
            clean_activity_text("apply\n_operation\t\u{0007}", 32),
            "apply_operation"
        );
        assert_eq!(clean_activity_text("abcdef", 3), "abc");
    }
}
