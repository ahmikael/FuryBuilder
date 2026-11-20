use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::RuntimeError;
use crate::types::RUNTIME_SETTINGS_SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSettingsFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    selected_java: Option<String>,
}

/// Persists only the selected `java` executable path in the application data directory.
/// Discovery results are recomputed; stale or missing paths are reported, not trusted.
pub struct RuntimeSettingsStore {
    path: PathBuf,
    state: Mutex<RuntimeSettingsFile>,
    load_warning: Option<String>,
}

impl RuntimeSettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut load_warning = None;
        let state = match load(&path) {
            Ok(settings) => settings,
            Err(error) => {
                load_warning = Some(error);
                RuntimeSettingsFile {
                    schema_version: RUNTIME_SETTINGS_SCHEMA_VERSION,
                    selected_java: None,
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

    pub fn selected(&self) -> Option<PathBuf> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.selected_java.clone())
            .map(PathBuf::from)
    }

    pub fn save_selected(&self, path: &Path) -> Result<(), RuntimeError> {
        let text = path.display().to_string();
        let mut state = self
            .state
            .lock()
            .map_err(|_| RuntimeError::Busy("runtime settings lock poisoned".to_owned()))?;
        state.selected_java = Some(text);
        state.schema_version = RUNTIME_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }

    pub fn clear(&self) -> Result<(), RuntimeError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| RuntimeError::Busy("runtime settings lock poisoned".to_owned()))?;
        state.selected_java = None;
        state.schema_version = RUNTIME_SETTINGS_SCHEMA_VERSION;
        persist(&self.path, &state)
    }
}

fn load(path: &Path) -> Result<RuntimeSettingsFile, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let mut settings: RuntimeSettingsFile = serde_json::from_slice(&bytes)
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
            match settings.schema_version {
                0 => {
                    settings.schema_version = RUNTIME_SETTINGS_SCHEMA_VERSION;
                }
                RUNTIME_SETTINGS_SCHEMA_VERSION => {}
                other => {
                    return Err(format!(
                        "unsupported runtime settings schema {other}; expected {RUNTIME_SETTINGS_SCHEMA_VERSION}"
                    ));
                }
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RuntimeSettingsFile {
            schema_version: RUNTIME_SETTINGS_SCHEMA_VERSION,
            selected_java: None,
        }),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn persist(path: &Path, settings: &RuntimeSettingsFile) -> Result<(), RuntimeError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", parent.display())))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| RuntimeError::Invalid(error.to_string()))?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, &bytes)
        .map_err(|error| RuntimeError::Io(format!("{}: {error}", temp.display())))?;
    if let Ok(file) = fs::File::open(&temp) {
        let _ = file.sync_all();
    }
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| RuntimeError::Io(format!("{}: {error}", path.display())))?;
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        RuntimeError::Io(format!("{}: {error}", path.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_invalid_schema_are_handled_explicitly() {
        let dir = std::env::temp_dir().join(format!(
            "furybuilder-runtime-settings-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("runtime.json");
        fs::write(&path, br#"{"selectedJava":"C:\\java\\bin\\java.exe"}"#).expect("legacy file");
        let store = RuntimeSettingsStore::new(&path);
        assert!(store.load_warning().is_none());
        assert_eq!(
            store.selected().expect("legacy selection"),
            PathBuf::from("C:\\java\\bin\\java.exe")
        );
        store.clear().expect("clear");
        assert!(store.selected().is_none());
        let text = fs::read_to_string(&path).expect("settings file");
        let value: serde_json::Value = serde_json::from_str(&text).expect("settings JSON");
        assert_eq!(value["schemaVersion"], 1);

        fs::write(&path, br#"{"schemaVersion":99,"selectedJava":null}"#).expect("bad file");
        let store = RuntimeSettingsStore::new(&path);
        assert!(store.selected().is_none());
        assert!(store
            .load_warning()
            .is_some_and(|warning| warning.contains("unsupported")));
        let _ = fs::remove_dir_all(&dir);
    }
}
