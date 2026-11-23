use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::discovery::inspect_asset_path;
use crate::resource_pack::inspect_resource_pack;
use crate::types::{AssetPathReport, ResourcePackReport};

/// Version of the small settings file that records the selected asset source and pack order.
pub const ASSET_SETTINGS_SCHEMA_VERSION: u32 = 2;
/// Maximum number of user resource packs. Bounded so a single render request stays predictable.
pub const MAX_RESOURCE_PACKS: usize = 16;

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
    /// Saved packs in priority order, highest priority first.
    pub packs: Vec<ResourcePackReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssetSettingsFile {
    #[serde(default)]
    schema_version: u32,
    source: Option<AssetSourceSetting>,
    #[serde(default)]
    packs: Vec<String>,
}

#[derive(Debug, Error)]
pub enum AssetSettingsError {
    #[error("asset settings cannot be read or written: {0}")]
    Io(String),
    #[error("asset settings are not valid: {0}")]
    Invalid(String),
    #[error("resource pack cannot be used: {0}")]
    UnsupportedPack(String),
}

impl AssetSettingsError {
    pub fn code(&self) -> &'static str {
        match self {
            AssetSettingsError::Io(_) => "io_error",
            AssetSettingsError::Invalid(_) => "invalid_argument",
            AssetSettingsError::UnsupportedPack(_) => "unsupported_format",
        }
    }
}

/// Persists the selected source and ordered pack stack in the application data directory.
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
                    packs: Vec::new(),
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
        let (source, pack_paths) = self
            .state
            .lock()
            .map(|state| (state.source.clone(), state.packs.clone()))
            .unwrap_or_else(|_| (None, Vec::new()));
        let report = source
            .as_ref()
            .map(|source| inspect_asset_path(&source.path));
        let packs = pack_paths
            .iter()
            .map(|path| inspect_resource_pack(Path::new(path)))
            .collect();
        SavedAssetSource {
            source,
            report,
            load_warning: self.load_warning.clone(),
            packs,
        }
    }

    /// Returns the saved pack paths in priority order, highest priority first.
    pub fn pack_stack(&self) -> Vec<String> {
        self.state
            .lock()
            .map(|state| state.packs.clone())
            .unwrap_or_default()
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

    /// Validates and stores the ordered pack stack. Highest priority first.
    pub fn save_pack_stack(
        &self,
        paths: Vec<String>,
    ) -> Result<SavedAssetSource, AssetSettingsError> {
        let mut normalized: Vec<String> = Vec::new();
        for path in paths {
            if path.trim().is_empty() {
                return Err(AssetSettingsError::Invalid(
                    "a resource pack path is empty".to_owned(),
                ));
            }
            let canonical = fs::canonicalize(&path).map_err(|error| {
                AssetSettingsError::Invalid(format!("cannot resolve {path}: {error}"))
            })?;
            let text = canonical.display().to_string();
            if normalized
                .iter()
                .any(|existing| path_key(existing) == path_key(&text))
            {
                return Err(AssetSettingsError::Invalid(format!(
                    "duplicate resource pack: {text}"
                )));
            }
            normalized.push(text);
            if normalized.len() > MAX_RESOURCE_PACKS {
                return Err(AssetSettingsError::Invalid(format!(
                    "at most {MAX_RESOURCE_PACKS} resource packs can be stacked"
                )));
            }
        }
        for path in &normalized {
            let report = inspect_resource_pack(Path::new(path));
            if report.status != crate::types::ResourcePackStatus::Compatible
                || !report.errors.is_empty()
            {
                let reason = report
                    .errors
                    .first()
                    .cloned()
                    .unwrap_or_else(|| format!("status {:?}", report.status));
                return Err(AssetSettingsError::UnsupportedPack(format!(
                    "{path}: {reason}"
                )));
            }
        }
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AssetSettingsError::Io("settings lock poisoned".to_owned()))?;
            state.packs = normalized;
            state.schema_version = ASSET_SETTINGS_SCHEMA_VERSION;
            persist(&self.path, &state)?;
        }
        Ok(self.saved_source())
    }

    pub fn clear_pack_stack(&self) -> Result<SavedAssetSource, AssetSettingsError> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| AssetSettingsError::Io("settings lock poisoned".to_owned()))?;
            state.packs.clear();
            state.schema_version = ASSET_SETTINGS_SCHEMA_VERSION;
            persist(&self.path, &state)?;
        }
        Ok(self.saved_source())
    }
}

fn path_key(path: &str) -> String {
    if cfg!(windows) {
        path.to_ascii_lowercase()
    } else {
        path.to_owned()
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
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(AssetSettingsFile {
            schema_version: ASSET_SETTINGS_SCHEMA_VERSION,
            source: None,
            packs: Vec::new(),
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
