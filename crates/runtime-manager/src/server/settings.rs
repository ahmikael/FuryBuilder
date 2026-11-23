use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use minecraft_version::VersionId;
use serde::{Deserialize, Serialize};

use crate::error::ServerError;

use super::types::{SavedEula, SavedServerJar, SERVER_SETTINGS_SCHEMA_VERSION};

/// Bounded number of remembered selections, one per exact Minecraft version.
const MAX_SERVER_SELECTIONS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerSettingsFile {
    #[serde(default)]
    schema_version: u32,
    /// One remembered selection per Minecraft version. The list is kept unique by version.
    #[serde(default)]
    selected: Vec<SavedServerJar>,
    #[serde(default)]
    eula: Option<SavedEula>,
}

/// Stores explicit manual and managed selections in `<app-data>/server.json`. The JAR itself
/// is revalidated on every report; this record is a pointer plus the last verified facts.
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
                    selected: Vec::new(),
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

    pub fn selected(&self, version: &VersionId) -> Option<SavedServerJar> {
        self.state.lock().ok().and_then(|state| {
            state
                .selected
                .iter()
                .find(|selected| selected.minecraft_version == version.as_str())
                .cloned()
        })
    }

    pub fn selections(&self) -> Vec<SavedServerJar> {
        self.state
            .lock()
            .map(|state| state.selected.clone())
            .unwrap_or_default()
    }

    pub fn save_selected(&self, selected: SavedServerJar) -> Result<(), ServerError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServerError::Busy("server settings lock poisoned".to_owned()))?;
        state
            .selected
            .retain(|entry| entry.minecraft_version != selected.minecraft_version);
        if state.selected.len() >= MAX_SERVER_SELECTIONS {
            return Err(ServerError::Limit(format!(
                "at most {MAX_SERVER_SELECTIONS} server JAR selections are remembered"
            )));
        }
        state.selected.push(selected);
        state.schema_version = SERVER_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }

    pub fn clear_selection(&self, version: &VersionId) -> Result<(), ServerError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| ServerError::Busy("server settings lock poisoned".to_owned()))?;
        state
            .selected
            .retain(|entry| entry.minecraft_version != version.as_str());
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
}

fn load(path: &Path) -> Result<ServerSettingsFile, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let settings: ServerSettingsFile = serde_json::from_slice(&bytes)
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
            if settings.schema_version != SERVER_SETTINGS_SCHEMA_VERSION {
                return Err(format!(
                    "unsupported server settings schema {}; expected {SERVER_SETTINGS_SCHEMA_VERSION}",
                    settings.schema_version
                ));
            }
            if settings.selected.len() > MAX_SERVER_SELECTIONS {
                return Err(format!(
                    "{} remembers more than {MAX_SERVER_SELECTIONS} server selections",
                    path.display()
                ));
            }
            let mut seen = std::collections::BTreeSet::new();
            for selected in &settings.selected {
                let version: VersionId = selected.minecraft_version.parse().map_err(|_| {
                    format!(
                        "{} contains an invalid Minecraft version `{}`",
                        path.display(),
                        selected.minecraft_version
                    )
                })?;
                if !seen.insert(version.as_str().to_owned()) {
                    return Err(format!(
                        "{} selects Minecraft {version} more than once",
                        path.display()
                    ));
                }
                if selected.data_version <= 0 || selected.size == 0 {
                    return Err(format!(
                        "{} contains an empty selection for Minecraft {version}",
                        path.display()
                    ));
                }
                if selected.sha1.len() != 40
                    || !selected.sha1.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(format!(
                        "{} contains a selection for Minecraft {version} without a valid SHA-1",
                        path.display()
                    ));
                }
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ServerSettingsFile {
            schema_version: SERVER_SETTINGS_SCHEMA_VERSION,
            selected: Vec::new(),
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

    fn version(raw: &str) -> VersionId {
        raw.parse().expect("version")
    }

    fn selection(raw: &str, data_version: i32) -> SavedServerJar {
        SavedServerJar {
            minecraft_version: raw.to_owned(),
            path: format!("C:\\servers\\{raw}\\server.jar"),
            sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            size: 56_880_250,
            data_version,
        }
    }

    #[test]
    fn only_the_current_schema_is_read() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-server-settings-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("server.json");
        fs::write(&path, br#"{"schemaVersion":1,"selected":[]}"#).expect("legacy settings");
        let store = ServerSettingsStore::new(&path);
        assert!(store.selected(&version("1.21.4")).is_none());
        assert!(store
            .load_warning()
            .is_some_and(|warning| warning.contains("unsupported")));

        fs::write(&path, br#"{"schemaVersion":99,"selected":[]}"#).expect("bad settings");
        let store = ServerSettingsStore::new(&path);
        assert!(store.selections().is_empty());
        assert!(store
            .load_warning()
            .is_some_and(|warning| warning.contains("unsupported")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn selections_are_kept_per_exact_version_and_the_eula_is_separate() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-server-selections-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("server.json");
        let store = ServerSettingsStore::new(&path);

        store
            .save_selected(selection("1.21.4", 4189))
            .expect("save");
        store
            .save_selected(selection("1.20.6", 3839))
            .expect("save");
        assert_eq!(store.selections().len(), 2);
        assert_eq!(
            store
                .selected(&version("1.20.6"))
                .expect("secondary selection")
                .data_version,
            3839
        );

        // Saving the same version replaces its stale facts instead of adding a duplicate.
        let mut replaced = selection("1.21.4", 4189);
        replaced.size = 1;
        // The size guard only rejects an empty selection, so a changed size still loads; it
        // is treated as a stale pointer by the report layer.
        store.save_selected(replaced).expect("replace");
        assert_eq!(store.selections().len(), 2);

        let eula = SavedEula {
            accepted_at: 1_763_500_000,
            minecraft_version: "1.21.4".to_owned(),
            jar_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            eula_url: "https://aka.ms/MinecraftEULA".to_owned(),
        };
        store.save_eula(eula.clone()).expect("save eula");
        store
            .clear_selection(&version("1.21.4"))
            .expect("clear one");
        assert!(store.selected(&version("1.21.4")).is_none());
        assert!(store.selected(&version("1.20.6")).is_some());
        assert_eq!(store.eula().expect("eula"), eula);

        store
            .clear_selection(&version("1.20.6"))
            .expect("clear the other");
        assert!(store.selections().is_empty());
        assert!(store.eula().is_some());

        store.clear_eula().expect("clear eula");
        assert!(store.eula().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_or_duplicate_selections_are_refused_on_load() {
        let dir =
            std::env::temp_dir().join(format!("furybuilder-server-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("server.json");
        let duplicate = serde_json::json!({
            "schemaVersion": SERVER_SETTINGS_SCHEMA_VERSION,
            "selected": [
                {
                    "minecraftVersion": "1.21.4",
                    "path": "C:\\a.jar",
                    "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001",
                    "size": 56880250,
                    "dataVersion": 4189
                },
                {
                    "minecraftVersion": "1.21.4",
                    "path": "C:\\b.jar",
                    "sha1": "4707d00eb834b446575d89a61a11b5d548d8c001",
                    "size": 56880250,
                    "dataVersion": 4189
                }
            ]
        });
        fs::write(&path, serde_json::to_vec(&duplicate).expect("json")).expect("settings");
        let store = ServerSettingsStore::new(&path);
        assert!(store.selections().is_empty());
        assert!(store
            .load_warning()
            .is_some_and(|warning| warning.contains("more than once")));
        let _ = fs::remove_dir_all(&dir);
    }
}
