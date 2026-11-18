use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use project_store::{EditorState, ProjectStore};
use serde::Serialize;
use world_application::WorldService;

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
}

impl AppState {
    pub fn new(projects_root: PathBuf) -> Self {
        Self {
            service: Mutex::new(WorldService::new()),
            projects: Mutex::new(ProjectSession::new(ProjectStore::new(projects_root))),
        }
    }
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
