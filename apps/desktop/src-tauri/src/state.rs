use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use project_store::{EditorState, ProjectStore};
use serde::Serialize;
use world_application::{AppError, WorldService};

use crate::local_bridge::{McpWriteAccess, RendererBridge};

/// Project namespace used by the local editor until a project is opened. The MCP bridge
/// must supply this identifier explicitly instead of inferring context from a session.
pub const LOCAL_PROJECT_ID: &str = "local-project";

/// State of the project currently backing the world service.
#[derive(Debug, Clone)]
pub struct CurrentProject {
    pub folder: String,
    pub project_id: String,
    pub name: String,
    pub saved_revision: u64,
    pub autosaved_revision: Option<u64>,
    pub editor_state: EditorState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatus {
    pub folder: Option<String>,
    pub name: Option<String>,
    pub saved_revision: u64,
    pub autosaved_revision: Option<u64>,
    pub revision: u64,
    pub dirty: bool,
    pub editor_state: Option<EditorState>,
    pub available_checkpoints: usize,
}

#[derive(Debug)]
pub struct ProjectSession {
    pub store: ProjectStore,
    pub current: Option<CurrentProject>,
    /// Checkpoint revisions per folder, refreshed after each save/checkpoint.
    pub checkpoints: BTreeMap<String, Vec<u64>>,
}

impl ProjectSession {
    pub fn new(store: ProjectStore) -> Self {
        Self {
            store,
            current: None,
            checkpoints: BTreeMap::new(),
        }
    }

    pub fn refresh_checkpoints(&mut self, folder: &str) {
        let revisions = self
            .store
            .checkpoints(folder)
            .map(|entries| entries.into_iter().map(|entry| entry.revision).collect())
            .unwrap_or_default();
        self.checkpoints.insert(folder.to_owned(), revisions);
    }
}

/// Shared authoritative services for the Tauri process. The project session and the world
/// service are separate locks; commands never hold both while doing blocking disk I/O.
pub struct AppState {
    pub service: Mutex<WorldService>,
    pub projects: Mutex<ProjectSession>,
    pub renderer: RendererBridge,
    pub mcp_writes: McpWriteAccess,
}

impl AppState {
    pub fn new(projects_root: PathBuf) -> Self {
        Self {
            service: Mutex::new(WorldService::new()),
            projects: Mutex::new(ProjectSession::new(ProjectStore::new(projects_root))),
            renderer: RendererBridge::default(),
            mcp_writes: McpWriteAccess::default(),
        }
    }
}

/// Session summary shared by the Tauri command and the local MCP bridge. It reports the
/// authoritative world revision and never infers unsaved state from the UI.
pub fn session_status(state: &AppState) -> Result<SessionStatus, AppError> {
    let revision = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().revision()
    };
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let checkpoint_count = projects
        .current
        .as_ref()
        .and_then(|current| projects.checkpoints.get(&current.folder))
        .map(Vec::len)
        .unwrap_or(0);
    Ok(match &projects.current {
        Some(current) => SessionStatus {
            folder: Some(current.folder.clone()),
            name: Some(current.name.clone()),
            saved_revision: current.saved_revision,
            autosaved_revision: current.autosaved_revision,
            revision,
            dirty: revision != current.saved_revision,
            editor_state: Some(current.editor_state.clone()),
            available_checkpoints: checkpoint_count,
        },
        None => SessionStatus {
            folder: None,
            name: None,
            saved_revision: 0,
            autosaved_revision: None,
            revision,
            dirty: false,
            editor_state: None,
            available_checkpoints: 0,
        },
    })
}

/// Last-chance persistence when the window closes. Unsaved work becomes a verified
/// autosave that the next open can adopt; a clean session only loses its marker.
pub fn autosave_on_exit(state: &AppState) {
    let Ok(projects) = state.projects.lock() else {
        return;
    };
    let Some(current) = projects.current.clone() else {
        return;
    };
    let Ok(world) = state.service.lock().map(|service| service.world().clone()) else {
        return;
    };
    if world.revision() != current.saved_revision {
        let _ = projects
            .store
            .autosave(&current.folder, &world, current.editor_state.clone(), "");
    } else {
        let _ = projects.store.end_session(&current.folder);
    }
}
