use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::discovery::inspect_asset_path;
use crate::types::AssetPathReport;

/// Version of the small settings file that records the selected asset source.
pub const ASSET_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSourceSetting {
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAssetSource {
    pub source: Option<AssetSourceSetting>,
    pub report: Option<AssetPathReport>,
    pub load_warning: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetSettingsFile {
    schema_version: u32,
    source: Option<AssetSourceSetting>,
}

#[derive(Debug, Error)]
pub enum AssetSettingsError {
    #[error("asset settings cannot be read or written: {0}")]
    Io(String),
    #[error("asset settings are not valid: {0}")]
    Invalid(String),
}

impl AssetSettingsError {
    pub fn code(&self) -> &'static str {
        match self {
            AssetSettingsError::Io(_) => "io_error",
            AssetSettingsError::Invalid(_) => "invalid_argument",
        }
    }
}

/// Persists one manually selected asset source in the application data directory.
///
/// This file contains only a kind and a path. It is not part of a project and never
/// stores Minecraft assets themselves.
pub struct AssetSettingsStore {
    path: PathBuf,
    state: Mutex<AssetSettingsFile>,
    load_warning: Option<String>,
}

impl AssetSettingsStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let mut load_warning = None;
        let state = match load_settings(&path) {
            Ok(settings) => settings,
            Err(error) => {
                load_warning = Some(error);
                AssetSettingsFile {
                    schema_version: ASSET_SETTINGS_SCHEMA_VERSION,
                    source: None,
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

    pub fn saved_source(&self) -> SavedAssetSource {
        let source = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.source.clone());
        let report = source
            .as_ref()
            .map(|source| inspect_asset_path(&source.path));
        SavedAssetSource {
            source,
            report,
            load_warning: self.load_warning.clone(),
        }
    }

    pub fn save(&self, source: AssetSourceSetting) -> Result<SavedAssetSource, AssetSettingsError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AssetSettingsError::Io("settings lock poisoned".to_owned()))?;
            state.source = Some(source);
            state.schema_version = ASSET_SETTINGS_SCHEMA_VERSION;
            persist(&self.path, &state)?;
        }
        Ok(self.saved_source())
    }

    pub fn clear(&self) -> Result<SavedAssetSource, AssetSettingsError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AssetSettingsError::Io("settings lock poisoned".to_owned()))?;
            state.source = None;
            state.schema_version = ASSET_SETTINGS_SCHEMA_VERSION;
            persist(&self.path, &state)?;
        }
        Ok(self.saved_source())
    }
}

fn load_settings(path: &Path) -> Result<AssetSettingsFile, String> {
    match fs::read(path) {
        Ok(bytes) => {
            let settings: AssetSettingsFile = serde_json::from_slice(&bytes)
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))?;
            if settings.schema_version != ASSET_SETTINGS_SCHEMA_VERSION {
                return Err(format!(
                    "unsupported asset settings schema {}; expected {}",
                    settings.schema_version, ASSET_SETTINGS_SCHEMA_VERSION
                ));
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AssetSettingsFile {
            schema_version: ASSET_SETTINGS_SCHEMA_VERSION,
            source: None,
        }),
        Err(error) => Err(format!("cannot read {}: {error}", path.display())),
    }
}

fn persist(path: &Path, settings: &AssetSettingsFile) -> Result<(), AssetSettingsError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AssetSettingsError::Io(format!("{}: {error}", parent.display())))?;
    }
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|error| AssetSettingsError::Invalid(error.to_string()))?;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, &bytes)
        .map_err(|error| AssetSettingsError::Io(format!("{}: {error}", temp.display())))?;
    if let Ok(file) = fs::File::open(&temp) {
        let _ = file.sync_all();
    }
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| AssetSettingsError::Io(format!("{}: {error}", path.display())))?;
    }
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        AssetSettingsError::Io(format!("{}: {error}", path.display()))
    })
}
