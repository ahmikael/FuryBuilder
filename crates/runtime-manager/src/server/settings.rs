use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::ServerError;

use super::types::{SavedEula, SavedServerJar, SERVER_SETTINGS_SCHEMA_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerSettingsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    selected: Option<SavedServerJar>,
    #[serde(default)]
    eula: Option<SavedEula>,
}

/// Stores only the explicit manual selection in `<app-data>/server.json`. The JAR itself is
/// revalidated on every report; this record is a pointer plus the last verified facts.
pub struct ServerSettingsStore {
    path: PathBuf,
    state: Mutex<ServerSettingsFile>,
    load_warning: Option<String>,
}

impl ServerSettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut load_warning = None;
        let state = match load(&path) {
            Ok(settings) => settings,
            Err(error) => {
                load_warning = Some(error);
                ServerSettingsFile {
                    schema_version: SERVER_SETTINGS_SCHEMA_VERSION,
                    selected: None,
                    eula: None,
                }
            }
        };
        Self {
            path,
            state: Mutex::new(state),
            load_warning,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_warning(&self) -> Option<&str> {
        self.load_warning.as_deref()
    }

    pub fn selected(&self) -> Option<SavedServerJar> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.selected.clone())
    }

    pub fn save_selected(&self, selected: SavedServerJar) -> Result<(), ServerError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServerError::Busy("server settings lock poisoned".to_owned()))?;
        state.selected = Some(selected);
        state.schema_version = SERVER_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }

    pub fn eula(&self) -> Option<SavedEula> {
        self.state.lock().ok().and_then(|state| state.eula.clone())
    }

    pub fn save_eula(&self, eula: SavedEula) -> Result<(), ServerError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServerError::Busy("server settings lock poisoned".to_owned()))?;
        state.eula = Some(eula);
        state.schema_version = SERVER_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }

    pub fn clear_eula(&self) -> Result<(), ServerError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServerError::Busy("server settings lock poisoned".to_owned()))?;
        state.eula = None;
        state.schema_version = SERVER_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }

    pub fn clear(&self) -> Result<(), ServerError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServerError::Busy("server settings lock poisoned".to_owned()))?;
        state.selected = None;
        state.schema_version = SERVER_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }
}

fn load(path: &Path) -> Result<ServerSettingsFile, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut settings: ServerSettingsFile = serde_json::from_slice(&bytes)
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
            match settings.schema_version {
                0 => settings.schema_version = SERVER_SETTINGS_SCHEMA_VERSION,
                SERVER_SETTINGS_SCHEMA_VERSION => {}
                other => {
                    return Err(format!(
                        "unsupported server settings schema {other}; expected {SERVER_SETTINGS_SCHEMA_VERSION}"
                    ));
                }
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ServerSettingsFile {
            schema_version: SERVER_SETTINGS_SCHEMA_VERSION,
            selected: None,
            eula: None,
        }),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn persist(path: &Path, settings: &ServerSettingsFile) -> Result<(), ServerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| ServerError::Io(format!("{}: {error}", parent.display())))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| ServerError::Invalid(error.to_string()))?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, &bytes)
        .map_err(|error| ServerError::Io(format!("{}: {error}", temp.display())))?;
    if let Ok(file) = fs::File::open(&temp) {
        let _ = file.sync_all();
    }
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| ServerError::Io(format!("{}: {error}", path.display())))?;
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        ServerError::Io(format!("{}: {error}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_unsupported_schemas_are_explicit() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-server-settings-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("server.json");
        fs::write(
            &path,
            br#"{"selected":{"path":"C:\\server.jar","sha1":"4707d00eb834b446575d89a61a11b5d548d8c001","size":56880250,"dataVersion":4189}}"#,
        )
        .expect("legacy settings");
        let store = ServerSettingsStore::new(&path);
        assert!(store.load_warning().is_none());
        assert_eq!(
            store.selected().expect("legacy selection").data_version,
            4189
        );
        store.clear().expect("clear");
        assert!(store.selected().is_none());

        let eula = SavedEula {
            accepted_at: 1_763_500_000,
            minecraft_version: "1.21.4".to_owned(),
            jar_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            eula_url: "https://aka.ms/MinecraftEULA".to_owned(),
        };
        store.save_eula(eula.clone()).expect("save eula");
        assert_eq!(store.eula().expect("eula"), eula);
        store.clear().expect("clear selection keeps eula");
        assert!(store.selected().is_none());
        assert!(store.eula().is_some());
        store.clear_eula().expect("clear eula");
        assert!(store.eula().is_none());

        fs::write(&path, br#"{"schemaVersion":99,"selected":null}"#).expect("bad settings");
        let store = ServerSettingsStore::new(&path);
        assert!(store.selected().is_none());
        assert!(store
            .load_warning()
            .is_some_and(|warning| warning.contains("unsupported")));
        let _ = fs::remove_dir_all(&dir);
    }
}
