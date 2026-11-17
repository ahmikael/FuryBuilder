use serde::{Deserialize, Serialize};

use crate::error::ProjectError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceManifest {
    pub schema_version: u32,
    pub immutable: bool,
    pub files: Vec<SourceFile>,
}

impl Default for SourceManifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            immutable: true,
            files: Vec::new(),
        }
    }
}

pub fn read_source_manifest(path: &std::path::Path) -> Result<SourceManifest, ProjectError> {
    if !path.exists() {
        return Ok(SourceManifest::default());
    }
    let bytes = std::fs::read(path)
        .map_err(|error| ProjectError::io(format!("reading {}", path.display()), error))?;
    let manifest: SourceManifest = serde_json::from_slice(&bytes)?;
    if manifest.schema_version != 1 {
        return Err(ProjectError::UnsupportedSchema {
            found: manifest.schema_version,
            supported: 1,
        });
    }
    Ok(manifest)
}
