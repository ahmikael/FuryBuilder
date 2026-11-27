use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::discovery::inspect_asset_path_for;
use crate::types::AssetPathReport;

/// Version of the small settings file that records the selected client asset source.
/// Schema 3 adds the explicit placeholder selection; older schemas are not read.
pub const ASSET_SETTINGS_SCHEMA_VERSION: u32 = 3;

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
    /// True when the user explicitly chose original placeholder textures instead of a client
    /// JAR. This is never reported as a verified asset source.
    pub placeholder: bool,
    pub load_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetSettingsFile {
    #[serde(default)]
    schema_version: u32,
    source: Option<AssetSourceSetting>,
    /// Mutually exclusive with `source`. A placeholder selection carries no path.
    #[serde(default)]
    placeholder: bool,
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

/// Persists the selected client source in the application data directory.
///
/// This file contains only kinds and paths. It is not part of a project and never stores
/// Minecraft assets themselves.
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
                    placeholder: false,
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
        self.saved_source_for(crate::target_profile())
    }

    pub fn saved_source_for(
        &self,
        profile: &minecraft_version::VersionProfile,
    ) -> SavedAssetSource {
        let (source, placeholder) = self
            .state
            .lock()
            .map(|state| (state.source.clone(), state.placeholder))
            .unwrap_or_default();
        let report = source
            .as_ref()
            .map(|source| inspect_asset_path_for(&source.path, profile));
        SavedAssetSource {
            source,
            report,
            placeholder,
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
            state.placeholder = false;
            state.schema_version = ASSET_SETTINGS_SCHEMA_VERSION;
            persist(&self.path, &state)?;
        }
        Ok(self.saved_source())
    }

    /// Explicitly selects original placeholder textures. This is a deliberate mode, not a
    /// silent fallback: no client JAR or catalog is available while it is active.
    pub fn use_placeholder(&self) -> Result<SavedAssetSource, AssetSettingsError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AssetSettingsError::Io("settings lock poisoned".to_owned()))?;
            state.source = None;
            state.placeholder = true;
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
            state.placeholder = false;
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
                    "unsupported asset settings schema {}; expected {ASSET_SETTINGS_SCHEMA_VERSION}",
                    settings.schema_version
                ));
            }
            if settings.placeholder && settings.source.is_some() {
                return Err(format!(
                    "{} selects both placeholder textures and a client asset source",
                    path.display()
                ));
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AssetSettingsFile {
            schema_version: ASSET_SETTINGS_SCHEMA_VERSION,
            source: None,
            placeholder: false,
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
