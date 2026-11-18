use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use world_domain::{BlockPos, ChunkPos, World};

use crate::error::ProjectError;
use crate::manifest::{
    folder_component, is_hex_sha256, normalize_name, now_ms, validate_folder, CreationMode,
    EditorState, ProjectDetail, ProjectList, ProjectManifest, ProjectSummary, RecoveryInfo,
    SaveKind, SaveOutcome, SourceInfo, PROJECT_FILE, PROJECT_SCHEMA_VERSION,
};
use crate::snapshot::{decode_world, encode_world};
use crate::source::{read_source_manifest, SourceManifest};

pub const MAX_SNAPSHOTS: usize = 8;
pub const MAX_CHECKPOINTS: usize = 5;
pub const SESSION_MARKER: &str = "session.lock";
pub const AUTOSAVE_FILE: &str = "autosave.fwl";
pub const AUTOSAVE_META: &str = "autosave.json";
pub const CHECKPOINT_DIR: &str = "changes";
pub const DATA_DIR: &str = "data";
pub const CACHE_DIR: &str = "cache";
pub const SOURCE_DIR: &str = "source";
pub const EXPORTS_DIR: &str = "exports";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutosaveMeta {
    revision: u64,
    checksum: String,
    bytes: u64,
    saved_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointMeta {
    pub revision: u64,
    pub checksum: String,
    pub bytes: u64,
    pub saved_at_ms: u64,
    pub reason: String,
}

/// Result of publishing an interchange file under `exports/`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    pub relative_path: String,
    pub file_name: String,
    pub bytes: u64,
    pub checksum: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionMarker {
    pid: u32,
    started_at_ms: u64,
    app_version: String,
    revision: u64,
}

/// A project folder manager rooted at a single directory. Every mutating operation is
/// serialized by the caller (the Tauri app holds one service lock) and every publish is
/// atomic: bytes are written to a temporary file, verified by decoding and re-encoding,
/// then renamed into place.
#[derive(Debug, Clone)]
pub struct ProjectStore {
    root: PathBuf,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn io_error(context: impl Into<String>, error: std::io::Error) -> ProjectError {
    ProjectError::io(context, error)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProjectError> {
    let parent = path
        .parent()
        .ok_or_else(|| ProjectError::InvalidPath(path.display().to_string()))?;
    fs::create_dir_all(parent)
        .map_err(|error| io_error(format!("creating {}", parent.display()), error))?;
    let temporary = path.with_extension("tmp");
    {
        let mut file = File::create(&temporary)
            .map_err(|error| io_error(format!("creating {}", temporary.display()), error))?;
        file.write_all(bytes)
            .map_err(|error| io_error(format!("writing {}", temporary.display()), error))?;
        file.sync_all()
            .map_err(|error| io_error(format!("syncing {}", temporary.display()), error))?;
    }
    // Publishing is a single rename on the same volume, so readers never see a partial file.
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        io_error(format!("publishing {}", path.display()), error)
    })?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ProjectError> {
    let bytes =
        fs::read(path).map_err(|error| io_error(format!("reading {}", path.display()), error))?;
    serde_json::from_slice(&bytes).map_err(ProjectError::Json)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), ProjectError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

fn verify_round_trip(bytes: &[u8]) -> Result<World, ProjectError> {
    let world = decode_world(bytes)?;
    let reencoded = encode_world(&world)?;
    if reencoded != bytes {
        return Err(ProjectError::Corrupt(
            "snapshot verification failed: re-encoding produced different bytes".to_owned(),
        ));
    }
    Ok(world)
}

fn snapshot_file(revision: u64) -> String {
    format!("world-{revision:020}.fwl")
}

fn parse_snapshot_revision(name: &str) -> Option<u64> {
    name.strip_prefix("world-")?
        .strip_suffix(".fwl")?
        .parse()
        .ok()
}

/// Builds a deterministic flat base: a 33x33 stone platform at y=63..=64 around the
/// origin. It is deliberate manual geometry, not vanilla world generation.
pub fn create_flat_base(world: &mut World) -> Result<(), ProjectError> {
    let stone: world_domain::BlockState = "minecraft:stone"
        .parse()
        .map_err(|_| ProjectError::corrupt("built-in stone state is invalid"))?;
    let dirt: world_domain::BlockState = "minecraft:dirt".parse().unwrap();
    let grass: world_domain::BlockState = "minecraft:grass_block[snowy=false]".parse().unwrap();
    for x in -16..=16 {
        for z in -16..=16 {
            world
                .set_block("minecraft:overworld", BlockPos::new(x, 62, z), &stone)
                .map_err(|error| ProjectError::corrupt(error.to_string()))?;
            world
                .set_block("minecraft:overworld", BlockPos::new(x, 63, z), &dirt)
                .map_err(|error| ProjectError::corrupt(error.to_string()))?;
            world
                .set_block("minecraft:overworld", BlockPos::new(x, 64, z), &grass)
                .map_err(|error| ProjectError::corrupt(error.to_string()))?;
        }
    }
    Ok(())
}

pub fn build_world(mode: CreationMode) -> Result<World, ProjectError> {
    let mut world = World::new_empty();
    if mode == CreationMode::Flat {
        create_flat_base(&mut world)?;
    }
    Ok(world)
}

impl ProjectStore {
    /// `root` is the directory that contains project folders (created on demand).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn project_dir(&self, folder: &str) -> Result<PathBuf, ProjectError> {
        validate_folder(folder)?;
        Ok(self.root.join(folder))
    }

    fn manifest(&self, dir: &Path) -> Result<ProjectManifest, ProjectError> {
        let path = dir.join(PROJECT_FILE);
        let raw: serde_json::Value = read_json(&path).map_err(|error| match error {
            ProjectError::Io { .. } => {
                ProjectError::corrupt(format!("missing project manifest at {}", path.display()))
            }
            other => other,
        })?;
        let version = raw
            .get("projectSchemaVersion")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32;
        if version > PROJECT_SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedSchema {
                found: version,
                supported: PROJECT_SCHEMA_VERSION,
            });
        }
        if version < PROJECT_SCHEMA_VERSION {
            // Older drafts are migrated in place after writing a verbatim backup, so a
            // failed migration never destroys the only copy of the manifest.
            let backup = dir.join(format!("project.json.v{version}.bak"));
            let manifest = migrate_manifest(raw, &backup)?;
            write_json(&path, &manifest)?;
            return Ok(manifest);
        }
        let manifest: ProjectManifest = serde_json::from_value(raw)?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn warn_manifest_gap(&self, dir: &Path) -> Option<String> {
        let path = dir.join(PROJECT_FILE);
        if path.exists() {
            return None;
        }
        let backups: Vec<PathBuf> = fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_string_lossy().to_string();
                name.starts_with("project.json.").then_some(path)
            })
            .collect();
        backups.first().map(|path| {
            format!(
                "project.json is missing; backup {} can be recovered",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        })
    }

    /// Scans the root for `*.furybuilder` folders. Corrupt folders are reported as warnings
    /// instead of taking down the whole list.
    pub fn list(&self) -> ProjectList {
        let mut projects = Vec::new();
        let mut warnings = Vec::new();
        let Ok(entries) = fs::read_dir(&self.root) else {
            return ProjectList { projects, warnings };
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(folder) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if validate_folder(&folder).is_err() {
                continue;
            }
            match self.manifest(&path) {
                Ok(manifest) => {
                    if let Err(error) = manifest.validate() {
                        warnings.push(format!("{folder}: {error}"));
                        continue;
                    }
                    let recoverable = path.join(SESSION_MARKER).exists()
                        || self.pending_autosave(&path, &manifest).unwrap_or(false);
                    projects.push(ProjectSummary::from_manifest(
                        &folder,
                        &manifest,
                        recoverable,
                    ));
                }
                Err(error) => {
                    if let Some(hint) = self.warn_manifest_gap(&path) {
                        warnings.push(format!("{folder}: {hint}"));
                    } else {
                        warnings.push(format!("{folder}: {error}"));
                    }
                }
            }
        }
        projects.sort_by_key(|project| std::cmp::Reverse(project.updated_at_ms));
        ProjectList { projects, warnings }
    }

    pub fn detail(&self, folder: &str) -> Result<ProjectDetail, ProjectError> {
        let dir = self.project_dir(folder)?;
        let manifest = self.manifest(&dir)?;
        manifest.validate()?;
        let mut warnings = Vec::new();
        let source = read_source_manifest(&dir.join(SOURCE_DIR).join("source.json"))?;
        if !source.files.is_empty() {
            warnings.push(format!(
                "{} immutable source file(s) are referenced; edits are stored in the project layer",
                source.files.len()
            ));
        }
        let recoverable =
            dir.join(SESSION_MARKER).exists() || self.pending_autosave(&dir, &manifest)?;
        Ok(ProjectDetail {
            summary: ProjectSummary::from_manifest(folder, &manifest, recoverable),
            editor_state: manifest.editor_state.clone(),
            source: manifest.source.clone(),
            created_at_ms: manifest.created_at_ms,
            warnings,
        })
    }

    /// Creates a project folder with an initial snapshot and manifest.
    pub fn create(
        &self,
        name: &str,
        mode: CreationMode,
        editor_state: EditorState,
    ) -> Result<ProjectDetail, ProjectError> {
        let name = normalize_name(name)?;
        if !mode.is_available() {
            return Err(ProjectError::InvalidManifest(format!(
                "creation mode `{}` is not available yet",
                mode.label()
            )));
        }
        editor_state.validate()?;
        fs::create_dir_all(&self.root)
            .map_err(|error| io_error(format!("creating {}", self.root.display()), error))?;

        let base = folder_component(&name);
        let mut folder = base.clone();
        let mut counter = 1;
        while self.root.join(&folder).exists() {
            counter += 1;
            let stem = base.trim_end_matches(".furybuilder");
            folder = format!("{stem} ({counter}).furybuilder");
            if counter > 999 {
                return Err(ProjectError::AlreadyExists(name.clone()));
            }
        }
        let dir = self.root.join(&folder);
        fs::create_dir_all(dir.join(DATA_DIR))
            .map_err(|error| io_error(format!("creating {}", dir.display()), error))?;
        fs::create_dir_all(dir.join(CACHE_DIR))
            .map_err(|error| io_error(format!("creating {}", dir.display()), error))?;
        fs::create_dir_all(dir.join(SOURCE_DIR))
            .map_err(|error| io_error(format!("creating {}", dir.display()), error))?;

        let mut world = build_world(mode)?;
        world.bump_revision();
        let bytes = encode_world(&world)?;
        verify_round_trip(&bytes)?;
        let checksum = sha256_hex(&bytes);
        let file = snapshot_file(world.revision());
        write_atomic(&dir.join(DATA_DIR).join(&file), &bytes)?;

        let now = now_ms();
        let project_id = uuid::Uuid::new_v4().to_string();
        let mut manifest = ProjectManifest::new(project_id, name, mode, world.revision(), now);
        manifest.editor_state = editor_state;
        manifest.set_world_payload(format!("{DATA_DIR}/{file}"), checksum, bytes.len() as u64);
        manifest.validate()?;
        write_json(&dir.join(PROJECT_FILE), &manifest)?;
        write_json(
            &dir.join(SOURCE_DIR).join("source.json"),
            &SourceManifest::default(),
        )?;

        self.detail(&folder)
    }

    /// Renames the display name. The folder stays stable so external references and the
    /// operating system's file handles keep working.
    pub fn rename(&self, folder: &str, name: &str) -> Result<ProjectDetail, ProjectError> {
        let dir = self.project_dir(folder)?;
        let mut manifest = self.manifest(&dir)?;
        manifest.validate()?;
        manifest.name = normalize_name(name)?;
        manifest.updated_at_ms = now_ms();
        write_json(&dir.join(PROJECT_FILE), &manifest)?;
        self.detail(folder)
    }

    fn pending_autosave(
        &self,
        dir: &Path,
        manifest: &ProjectManifest,
    ) -> Result<bool, ProjectError> {
        let meta_path = dir.join(DATA_DIR).join(AUTOSAVE_META);
        if !meta_path.exists() {
            return Ok(false);
        }
        let Ok(meta) = read_json::<AutosaveMeta>(&meta_path) else {
            return Ok(true);
        };
        Ok(meta.revision > manifest.world_revision
            && dir.join(DATA_DIR).join(AUTOSAVE_FILE).exists())
    }

    /// Removes crash-recovery artifacts while keeping the last manual save.
    pub fn clear_autosave(&self, folder: &str) -> Result<(), ProjectError> {
        let dir = self.project_dir(folder)?;
        let data_dir = dir.join(DATA_DIR);
        for name in [AUTOSAVE_FILE, AUTOSAVE_META] {
            let path = data_dir.join(name);
            if path.exists() {
                fs::remove_file(&path)
                    .map_err(|error| io_error(format!("removing {}", path.display()), error))?;
            }
        }
        Ok(())
    }

    /// Opens a project. If a crashed session left a newer verified autosave behind, the
    /// caller receives it together with a `RecoveryInfo`; the on-disk project is untouched
    /// until the user explicitly saves.
    pub fn open(
        &self,
        folder: &str,
    ) -> Result<(ProjectDetail, World, Option<RecoveryInfo>), ProjectError> {
        let dir = self.project_dir(folder)?;
        let manifest = self.manifest(&dir)?;
        manifest.validate()?;
        let snapshot_path = dir.join(&manifest.world_file);
        let bytes = fs::read(&snapshot_path)
            .map_err(|error| io_error(format!("reading {}", snapshot_path.display()), error))?;
        let actual = sha256_hex(&bytes);
        if actual != manifest.world_checksum {
            return Err(ProjectError::ChecksumMismatch {
                expected: manifest.world_checksum.clone(),
                actual,
            });
        }
        let mut world = verify_round_trip(&bytes)?;
        let marker_present = dir.join(SESSION_MARKER).exists();
        let recovery = if marker_present {
            self.recovery_for(&dir, &manifest)?
        } else {
            None
        };
        if let Some(info) = &recovery {
            let autosave = dir.join(DATA_DIR).join(AUTOSAVE_FILE);
            let bytes = fs::read(&autosave)
                .map_err(|error| io_error(format!("reading {}", autosave.display()), error))?;
            world = verify_round_trip(&bytes)?;
            if world.revision() != info.recovered_revision {
                return Err(ProjectError::corrupt(
                    "autosave revision does not match its metadata",
                ));
            }
        }
        let detail = self.detail(folder)?;
        Ok((detail, world, recovery))
    }

    fn recovery_for(
        &self,
        dir: &Path,
        manifest: &ProjectManifest,
    ) -> Result<Option<RecoveryInfo>, ProjectError> {
        let meta_path = dir.join(DATA_DIR).join(AUTOSAVE_META);
        if !meta_path.exists() {
            return Ok(None);
        }
        let meta: AutosaveMeta = match read_json(&meta_path) {
            Ok(meta) => meta,
            Err(_) => {
                return Ok(Some(RecoveryInfo {
                    reason: "the crashed session left an unreadable autosave record".to_owned(),
                    recovered_revision: manifest.world_revision,
                    recovered_at_ms: now_ms(),
                    from_backup: false,
                }))
            }
        };
        if meta.revision <= manifest.world_revision {
            return Ok(None);
        }
        if !is_hex_sha256(&meta.checksum) {
            return Ok(Some(RecoveryInfo {
                reason: "autosave metadata has an invalid checksum".to_owned(),
                recovered_revision: meta.revision,
                recovered_at_ms: meta.saved_at_ms,
                from_backup: false,
            }));
        }
        let autosave = dir.join(DATA_DIR).join(AUTOSAVE_FILE);
        if !autosave.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&autosave)
            .map_err(|error| io_error(format!("reading {}", autosave.display()), error))?;
        let checksum = sha256_hex(&bytes);
        if checksum != meta.checksum {
            return Ok(Some(RecoveryInfo {
                reason: "autosave checksum does not match its metadata; save again from the editor"
                    .to_owned(),
                recovered_revision: manifest.world_revision,
                recovered_at_ms: meta.saved_at_ms,
                from_backup: false,
            }));
        }
        Ok(Some(RecoveryInfo {
            reason: "the previous session ended without closing the project".to_owned(),
            recovered_revision: meta.revision,
            recovered_at_ms: meta.saved_at_ms,
            from_backup: false,
        }))
    }

    /// Starts a session. The marker records enough context to explain a crash on the next
    /// open; nothing else changes on disk.
    pub fn begin_session(&self, folder: &str, revision: u64) -> Result<(), ProjectError> {
        let dir = self.project_dir(folder)?;
        if !dir.join(PROJECT_FILE).exists() {
            return Err(ProjectError::NotFound(folder.to_owned()));
        }
        let marker = SessionMarker {
            pid: std::process::id(),
            started_at_ms: now_ms(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            revision,
        };
        write_json(&dir.join(SESSION_MARKER), &marker)
    }

    /// Removes the marker after a graceful close and clears the (recreatable) cache.
    pub fn end_session(&self, folder: &str) -> Result<(), ProjectError> {
        let dir = self.project_dir(folder)?;
        let marker = dir.join(SESSION_MARKER);
        if marker.exists() {
            fs::remove_file(&marker)
                .map_err(|error| io_error(format!("removing {}", marker.display()), error))?;
        }
        let cache = dir.join(CACHE_DIR);
        if cache.exists() {
            fs::remove_dir_all(&cache)
                .map_err(|error| io_error(format!("clearing {}", cache.display()), error))?;
        }
        fs::create_dir_all(&cache)
            .map_err(|error| io_error(format!("creating {}", cache.display()), error))?;
        Ok(())
    }

    /// Atomic save: snapshot bytes are written to a temporary file, verified by a full
    /// decode + re-encode comparison, published by rename, and only then referenced from
    /// `project.json` (which is itself published atomically). A crash between the two
    /// renames leaves an unreferenced snapshot, never a manifest pointing at bad bytes.
    pub fn save(
        &self,
        folder: &str,
        world: &World,
        editor_state: EditorState,
        kind: SaveKind,
    ) -> Result<SaveOutcome, ProjectError> {
        let dir = self.project_dir(folder)?;
        let mut manifest = self.manifest(&dir)?;
        manifest.validate()?;
        editor_state.validate()?;

        let bytes = encode_world(world)?;
        let decoded = verify_round_trip(&bytes)?;
        if decoded.revision() != world.revision()
            || decoded.block_count() != world.block_count()
            || decoded.chunk_count() != world.chunk_count()
        {
            return Err(ProjectError::Corrupt(
                "snapshot semantic verification failed".to_owned(),
            ));
        }
        let checksum = sha256_hex(&bytes);
        let file = snapshot_file(world.revision());
        let relative = format!("{DATA_DIR}/{file}");
        let data_dir = dir.join(DATA_DIR);
        write_atomic(&data_dir.join(&file), &bytes)?;

        let now = now_ms();
        manifest.world_revision = world.revision();
        manifest.set_world_payload(relative.clone(), checksum.clone(), bytes.len() as u64);
        manifest.updated_at_ms = now;
        manifest.last_saved_at_ms = now;
        manifest.last_save_kind = kind;
        manifest.editor_state = editor_state;
        manifest.validate()?;

        // Keep the previous manifest as a migration/backup breadcrumb before publishing.
        let previous_manifest = dir.join(PROJECT_FILE);
        let mut backup_created = false;
        if previous_manifest.exists() {
            let backup = dir.join(format!(
                "project.json.bak-{}",
                manifest.world_revision.saturating_sub(1)
            ));
            fs::copy(&previous_manifest, &backup).map_err(|error| {
                io_error(format!("backing up {}", previous_manifest.display()), error)
            })?;
            backup_created = true;
        }
        write_json(&previous_manifest, &manifest)?;

        if kind == SaveKind::Manual {
            // A manual save is authoritative; a stale autosave must not resurface.
            let autosave = data_dir.join(AUTOSAVE_FILE);
            if autosave.exists() {
                let _ = fs::remove_file(&autosave);
            }
            let meta = data_dir.join(AUTOSAVE_META);
            if meta.exists() {
                let _ = fs::remove_file(&meta);
            }
        }

        let pruned_snapshots = self.prune_snapshots(&data_dir, &relative)?;
        let detail = self.detail(folder)?;
        Ok(SaveOutcome {
            project: detail,
            checksum,
            snapshot_file: relative,
            bytes_written: bytes.len() as u64,
            backup_created,
            pruned_snapshots,
        })
    }

    /// Writes a crash-recovery autosave. It never touches the manifest, so an autosave can
    /// only be adopted explicitly after a crash marker is found.
    pub fn autosave(
        &self,
        folder: &str,
        world: &World,
        editor_state: EditorState,
        reason: &str,
    ) -> Result<u64, ProjectError> {
        let dir = self.project_dir(folder)?;
        let mut manifest = self.manifest(&dir)?;
        manifest.validate()?;
        editor_state.validate()?;
        let bytes = encode_world(world)?;
        verify_round_trip(&bytes)?;
        let data_dir = dir.join(DATA_DIR);
        write_atomic(&data_dir.join(AUTOSAVE_FILE), &bytes)?;
        let meta = AutosaveMeta {
            revision: world.revision(),
            checksum: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            saved_at_ms: now_ms(),
        };
        write_json(&data_dir.join(AUTOSAVE_META), &meta)?;
        manifest.editor_state = editor_state;
        manifest.updated_at_ms = meta.saved_at_ms;
        manifest.last_save_kind = SaveKind::Autosave;
        write_json(&dir.join(PROJECT_FILE), &manifest)?;
        if !reason.is_empty() {
            self.checkpoint(folder, world, manifest.editor_state, reason)?;
        }
        Ok(world.revision())
    }

    /// Rolling checkpoints used for explicit recovery points. Bounded to `MAX_CHECKPOINTS`.
    pub fn checkpoint(
        &self,
        folder: &str,
        world: &World,
        editor_state: EditorState,
        reason: &str,
    ) -> Result<String, ProjectError> {
        let dir = self.project_dir(folder)?;
        let manifest = self.manifest(&dir)?;
        manifest.validate()?;
        editor_state.validate()?;
        let bytes = encode_world(world)?;
        verify_round_trip(&bytes)?;
        let checkpoint_dir = dir.join(CHECKPOINT_DIR);
        fs::create_dir_all(&checkpoint_dir)
            .map_err(|error| io_error(format!("creating {}", checkpoint_dir.display()), error))?;
        let name = format!("checkpoint-{:020}.fwl", world.revision());
        write_atomic(&checkpoint_dir.join(&name), &bytes)?;
        let meta = CheckpointMeta {
            revision: world.revision(),
            checksum: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            saved_at_ms: now_ms(),
            reason: reason.to_owned(),
        };
        write_json(
            &checkpoint_dir.join(format!("checkpoint-{:020}.json", world.revision())),
            &meta,
        )?;
        let mut entries: Vec<_> = fs::read_dir(&checkpoint_dir)
            .map_err(|error| io_error(format!("listing {}", checkpoint_dir.display()), error))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "fwl"))
            .collect();
        entries.sort();
        let excess = entries.len().saturating_sub(MAX_CHECKPOINTS);
        for path in entries.into_iter().take(excess) {
            let _ = fs::remove_file(&path);
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                let _ = fs::remove_file(checkpoint_dir.join(format!("{stem}.json")));
            }
        }
        Ok(name)
    }

    /// Lists verified checkpoints, newest first. Corrupt entries are skipped.
    pub fn checkpoints(&self, folder: &str) -> Result<Vec<CheckpointMeta>, ProjectError> {
        let dir = self.project_dir(folder)?;
        let checkpoint_dir = dir.join(CHECKPOINT_DIR);
        if !checkpoint_dir.exists() {
            return Ok(Vec::new());
        }
        let mut entries: Vec<_> = fs::read_dir(&checkpoint_dir)
            .map_err(|error| io_error(format!("listing {}", checkpoint_dir.display()), error))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries.sort();
        entries.reverse();
        Ok(entries
            .into_iter()
            .filter_map(|path| read_json::<CheckpointMeta>(&path).ok())
            .collect())
    }

    /// Opens a checkpoint snapshot by revision. The current project is unaffected.
    pub fn open_checkpoint(&self, folder: &str, revision: u64) -> Result<World, ProjectError> {
        let dir = self.project_dir(folder)?;
        let path = dir
            .join(CHECKPOINT_DIR)
            .join(format!("checkpoint-{revision:020}.fwl"));
        if !path.exists() {
            return Err(ProjectError::NotFound(format!("checkpoint {revision}")));
        }
        let bytes = fs::read(&path)
            .map_err(|error| io_error(format!("reading {}", path.display()), error))?;
        verify_round_trip(&bytes)
    }

    fn prune_snapshots(&self, data_dir: &Path, keep: &str) -> Result<usize, ProjectError> {
        let mut snapshots: Vec<(u64, PathBuf)> = fs::read_dir(data_dir)
            .map_err(|error| io_error(format!("listing {}", data_dir.display()), error))?
            .flatten()
            .map(|entry| entry.path())
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?;
                let revision = parse_snapshot_revision(name)?;
                Some((revision, path))
            })
            .collect();
        snapshots.sort_by_key(|(revision, _)| *revision);
        let keep_name = Path::new(keep)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let excess = snapshots.len().saturating_sub(MAX_SNAPSHOTS);
        let mut pruned = 0;
        for (_, path) in snapshots.into_iter().take(excess) {
            if path.file_name().and_then(|name| name.to_str()) == Some(keep_name.as_str()) {
                continue;
            }
            if fs::remove_file(&path).is_ok() {
                pruned += 1;
            }
        }
        Ok(pruned)
    }

    /// Copies immutable source files into the project. The overlay is the project snapshot;
    /// the source layer is never edited. Paths are validated to stay under `source/files/`.
    pub fn attach_source(
        &self,
        folder: &str,
        source_path: &Path,
        relative_name: &str,
    ) -> Result<ProjectDetail, ProjectError> {
        let bytes = fs::read(source_path)
            .map_err(|error| io_error(format!("reading {}", source_path.display()), error))?;
        self.attach_source_bytes(folder, relative_name, &bytes)
    }

    /// Same contract as `attach_source`, for import bytes that arrived through IPC rather
    /// than a filesystem path.
    pub fn attach_source_bytes(
        &self,
        folder: &str,
        relative_name: &str,
        bytes: &[u8],
    ) -> Result<ProjectDetail, ProjectError> {
        let dir = self.project_dir(folder)?;
        let mut manifest = self.manifest(&dir)?;
        manifest.validate()?;
        let relative = Path::new(relative_name);
        if relative.components().count() != 1
            || relative_name.is_empty()
            || relative_name.contains("..")
            || relative_name.contains('/')
            || relative_name.contains('\\')
        {
            return Err(ProjectError::UnsafeSource(relative_name.to_owned()));
        }
        let source_dir = dir.join(SOURCE_DIR).join("files");
        fs::create_dir_all(&source_dir)
            .map_err(|error| io_error(format!("creating {}", source_dir.display()), error))?;
        let checksum = sha256_hex(bytes);
        let destination = source_dir.join(relative_name);
        write_atomic(&destination, bytes)?;
        let mut source = read_source_manifest(&dir.join(SOURCE_DIR).join("source.json"))?;
        source.immutable = true;
        source
            .files
            .retain(|file| file.relative_path != relative_name);
        source.files.push(crate::source::SourceFile {
            relative_path: relative_name.to_owned(),
            bytes: bytes.len() as u64,
            sha256: checksum,
        });
        source
            .files
            .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        write_json(&dir.join(SOURCE_DIR).join("source.json"), &source)?;
        manifest.source = SourceInfo {
            kind: crate::manifest::SourceKind::Imported,
            immutable: true,
            reference: Some(relative_name.to_owned()),
        };
        manifest.updated_at_ms = now_ms();
        write_json(&dir.join(PROJECT_FILE), &manifest)?;
        self.detail(folder)
    }

    /// Publishes an interchange file under `exports/` with an atomic write and a byte-level
    /// read-back check. Existing names get a numeric suffix instead of being overwritten.
    pub fn write_export(
        &self,
        folder: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<ExportOutcome, ProjectError> {
        let dir = self.project_dir(folder)?;
        if !dir.join(PROJECT_FILE).exists() {
            return Err(ProjectError::NotFound(folder.to_owned()));
        }
        let path = Path::new(file_name);
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if file_name.is_empty()
            || file_name.len() > 120
            || path.components().count() != 1
            || file_name.contains("..")
            || file_name.contains('/')
            || file_name.contains('\\')
            || !matches!(extension.as_str(), "nbt" | "schem")
        {
            return Err(ProjectError::InvalidPath(file_name.to_owned()));
        }
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("structure")
            .to_owned();
        let exports = dir.join(EXPORTS_DIR);
        fs::create_dir_all(&exports)
            .map_err(|error| io_error(format!("creating {}", exports.display()), error))?;

        let mut unique_name = file_name.to_owned();
        let mut destination = exports.join(&unique_name);
        let mut counter = 1;
        while destination.exists() {
            counter += 1;
            if counter > 999 {
                return Err(ProjectError::AlreadyExists(file_name.to_owned()));
            }
            unique_name = format!("{stem}-{counter}.{extension}");
            destination = exports.join(&unique_name);
        }

        write_atomic(&destination, bytes)?;
        let read_back = fs::read(&destination)
            .map_err(|error| io_error(format!("reading {}", destination.display()), error))?;
        if read_back != bytes {
            let _ = fs::remove_file(&destination);
            return Err(ProjectError::corrupt(
                "export read-back did not match the written bytes",
            ));
        }
        Ok(ExportOutcome {
            relative_path: format!("{EXPORTS_DIR}/{unique_name}"),
            file_name: unique_name,
            bytes: bytes.len() as u64,
            checksum: sha256_hex(bytes),
            created_at_ms: now_ms(),
        })
    }

    pub fn source_manifest(&self, folder: &str) -> Result<SourceManifest, ProjectError> {
        let dir = self.project_dir(folder)?;
        read_source_manifest(&dir.join(SOURCE_DIR).join("source.json"))
    }
}

/// Migrates a stored manifest to the current schema. Unknown/newer versions are refused
/// with a backup path so a newer app can still open the folder; older draft versions gain
/// the fields introduced after them.
pub fn migrate_manifest(
    raw: serde_json::Value,
    backup_path: &Path,
) -> Result<ProjectManifest, ProjectError> {
    let version = raw
        .get("projectSchemaVersion")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    match version {
        v if v == PROJECT_SCHEMA_VERSION => {
            let manifest: ProjectManifest = serde_json::from_value(raw)?;
            manifest.validate()?;
            Ok(manifest)
        }
        v if v > PROJECT_SCHEMA_VERSION => Err(ProjectError::UnsupportedSchema {
            found: v,
            supported: PROJECT_SCHEMA_VERSION,
        }),
        v if v < PROJECT_SCHEMA_VERSION => {
            // Draft folders from before schema 1 used snake_case and lacked editorState.
            let mut value = raw;
            if let Some(object) = value.as_object_mut() {
                object.insert(
                    "projectSchemaVersion".to_owned(),
                    serde_json::Value::from(PROJECT_SCHEMA_VERSION),
                );
                object
                    .entry("editorState")
                    .or_insert_with(|| serde_json::to_value(EditorState::default()).unwrap());
                object
                    .entry("source")
                    .or_insert_with(|| serde_json::to_value(SourceInfo::generated()).unwrap());
                object
                    .entry("createdBy")
                    .or_insert_with(|| serde_json::Value::from("FuryBuilder"));
                object
                    .entry("appVersion")
                    .or_insert_with(|| serde_json::Value::from(env!("CARGO_PKG_VERSION")));
                object
                    .entry("lastSaveKind")
                    .or_insert_with(|| serde_json::Value::from("manual"));
            }
            let bytes = serde_json::to_vec_pretty(&value)?;
            write_atomic(backup_path, &bytes)?;
            let manifest: ProjectManifest = serde_json::from_value(value)?;
            manifest.validate()?;
            Ok(manifest)
        }
        _ => Err(ProjectError::UnsupportedSchema {
            found: version,
            supported: PROJECT_SCHEMA_VERSION,
        }),
    }
}

/// Stable checksum helper used by tests and the UI.
pub fn checksum(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

/// Serializes a world snapshot without touching disk (used by tests and future exports).
pub fn snapshot_bytes(world: &World) -> Result<Vec<u8>, ProjectError> {
    encode_world(world)
}

/// Reads a specific block for tests and inspection.
pub fn block_at(world: &World, dimension: &str, pos: BlockPos) -> Option<world_domain::BlockState> {
    world.get_block(dimension, pos).ok()
}

/// Chunk coordinates that contain data in a dimension.
pub fn chunk_positions(world: &World, dimension: &str) -> Vec<ChunkPos> {
    world
        .dimension(dimension)
        .map(|dimension| dimension.chunks().keys().copied().collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-store-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn create_list_open_round_trips_world_and_manifest() {
        let root = temp_root("roundtrip");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("My Castle", CreationMode::Flat, EditorState::default())
            .expect("create");
        assert_eq!(detail.summary.name, "My Castle");
        assert_eq!(detail.summary.world_revision, 1);

        let (_, world, recovery) = store.open(&detail.summary.folder).expect("open");
        assert!(recovery.is_none());
        assert_eq!(world.revision(), 1);
        assert!(world.block_count() > 3000);
        assert_eq!(
            block_at(&world, "minecraft:overworld", BlockPos::new(0, 64, 0))
                .unwrap()
                .name(),
            "minecraft:grass_block"
        );
        assert_eq!(
            block_at(&world, "minecraft:overworld", BlockPos::new(-16, 62, 16))
                .unwrap()
                .name(),
            "minecraft:stone"
        );
        assert!(
            block_at(&world, "minecraft:overworld", BlockPos::new(-17, 62, 0))
                .unwrap()
                .is_air()
        );

        let list = store.list();
        assert_eq!(list.projects.len(), 1);
        assert!(list.warnings.is_empty());
    }

    #[test]
    fn corrupt_snapshot_is_rejected_by_checksum() {
        let root = temp_root("checksum");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Checksum", CreationMode::Empty, EditorState::default())
            .expect("create");
        let dir = root.join(&detail.summary.folder);
        let manifest = store.manifest(&dir).unwrap();
        let path = dir.join(&manifest.world_file);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&path, bytes).unwrap();
        match store.open(&detail.summary.folder) {
            Err(ProjectError::ChecksumMismatch { .. }) => {}
            other => panic!("expected checksum mismatch, got {other:?}"),
        }
    }

    #[test]
    fn save_is_atomic_and_keeps_previous_snapshot() {
        let root = temp_root("save");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Atomic", CreationMode::Empty, EditorState::default())
            .expect("create");
        let (_, mut world, _) = store.open(&detail.summary.folder).expect("open");
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(-3, 70, 5),
                &"minecraft:oak_planks".parse().unwrap(),
            )
            .unwrap();
        world.bump_revision();
        let outcome = store
            .save(
                &detail.summary.folder,
                &world,
                EditorState::default(),
                SaveKind::Manual,
            )
            .expect("save");
        assert_eq!(outcome.project.summary.world_revision, 2);
        assert!(outcome.backup_created);
        assert_eq!(outcome.checksum.len(), 64);

        let (_, reopened, _) = store.open(&detail.summary.folder).expect("reopen");
        assert_eq!(reopened.revision(), 2);
        assert_eq!(
            block_at(&reopened, "minecraft:overworld", BlockPos::new(-3, 70, 5))
                .unwrap()
                .name(),
            "minecraft:oak_planks"
        );
        let snapshots = fs::read_dir(root.join(&detail.summary.folder).join(DATA_DIR))
            .unwrap()
            .flatten()
            .filter(|entry| parse_snapshot_revision(&entry.file_name().to_string_lossy()).is_some())
            .count();
        assert_eq!(snapshots, 2);
    }

    #[test]
    fn crash_recovery_uses_newer_verified_autosave() {
        let root = temp_root("recovery");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Crashy", CreationMode::Empty, EditorState::default())
            .expect("create");
        let folder = detail.summary.folder.clone();
        store.begin_session(&folder, 1).expect("begin");
        let (_, mut world, _) = store.open(&folder).expect("open");
        world
            .set_block(
                "minecraft:overworld",
                BlockPos::new(2, 80, -2),
                &"minecraft:glass".parse().unwrap(),
            )
            .unwrap();
        world.bump_revision();
        world.bump_revision();
        store
            .autosave(&folder, &world, EditorState::default(), "autosave")
            .expect("autosave");

        let (detail, recovered, recovery) = store.open(&folder).expect("recover");
        assert_eq!(recovered.revision(), 3);
        assert_eq!(
            block_at(&recovered, "minecraft:overworld", BlockPos::new(2, 80, -2))
                .unwrap()
                .name(),
            "minecraft:glass"
        );
        let info = recovery.expect("recovery info");
        assert_eq!(info.recovered_revision, 3);
        assert!(detail.summary.recoverable);
        let listed = store.list();
        assert!(listed.projects[0].recoverable);

        store.end_session(&folder).expect("end");
        let (_, plain, recovery) = store.open(&folder).expect("open after end");
        assert_eq!(plain.revision(), 1);
        assert!(recovery.is_none());
    }

    #[test]
    fn manual_save_clears_stale_autosave() {
        let root = temp_root("clear-autosave");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Clear", CreationMode::Empty, EditorState::default())
            .expect("create");
        let folder = detail.summary.folder.clone();
        store.begin_session(&folder, 1).unwrap();
        let (_, mut world, _) = store.open(&folder).unwrap();
        world.bump_revision();
        store
            .autosave(&folder, &world, EditorState::default(), "")
            .unwrap();
        store
            .save(&folder, &world, EditorState::default(), SaveKind::Manual)
            .unwrap();
        assert!(!root
            .join(&folder)
            .join(DATA_DIR)
            .join(AUTOSAVE_FILE)
            .exists());
        let (_, reopened, recovery) = store.open(&folder).unwrap();
        assert!(recovery.is_none());
        assert_eq!(reopened.revision(), 2);
    }

    #[test]
    fn checkpoints_are_bounded_and_readable() {
        let root = temp_root("checkpoints");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Checkpoints", CreationMode::Empty, EditorState::default())
            .expect("create");
        let folder = detail.summary.folder.clone();
        let (_, mut world, _) = store.open(&folder).unwrap();
        for _ in 0..(MAX_CHECKPOINTS + 3) {
            world.bump_revision();
            store
                .checkpoint(&folder, &world, EditorState::default(), "manual")
                .unwrap();
        }
        let checkpoints = store.checkpoints(&folder).unwrap();
        assert_eq!(checkpoints.len(), MAX_CHECKPOINTS);
        let newest = checkpoints[0].revision;
        let restored = store.open_checkpoint(&folder, newest).unwrap();
        assert_eq!(restored.revision(), newest);
        assert!(store.open_checkpoint(&folder, 1).is_err());
    }

    #[test]
    fn unsupported_newer_schema_is_refused() {
        let root = temp_root("schema");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Schema", CreationMode::Empty, EditorState::default())
            .expect("create");
        let dir = root.join(&detail.summary.folder);
        let path = dir.join(PROJECT_FILE);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["projectSchemaVersion"] = serde_json::Value::from(99);
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        match store.open(&detail.summary.folder) {
            Err(ProjectError::UnsupportedSchema { found: 99, .. }) => {}
            other => panic!("expected unsupported schema, got {other:?}"),
        }
        value["projectSchemaVersion"] = serde_json::Value::from(0);
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        let (_, world, _) = store
            .open(&detail.summary.folder)
            .expect("draft schema migrates");
        assert_eq!(world.revision(), 1);
        assert!(root
            .join(&detail.summary.folder)
            .join("project.json.v0.bak")
            .exists());
    }

    #[test]
    fn rename_updates_display_name_only_and_rejects_paths() {
        let root = temp_root("rename");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Old Name", CreationMode::Empty, EditorState::default())
            .expect("create");
        let folder = detail.summary.folder.clone();
        let renamed = store.rename(&folder, "New Name").expect("rename");
        assert_eq!(renamed.summary.name, "New Name");
        assert_eq!(renamed.summary.folder, folder);
        assert!(store.rename(&folder, "../escape").is_err());
        assert!(store.rename(&folder, "").is_err());
    }

    #[test]
    fn source_files_are_immutable_copies_with_hashes() {
        let root = temp_root("source");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Imported", CreationMode::Empty, EditorState::default())
            .expect("create");
        let folder = detail.summary.folder.clone();
        let source = root.join("outside.bin");
        fs::write(&source, b"structure-bytes").unwrap();
        let updated = store
            .attach_source(&folder, &source, "outside.bin")
            .expect("attach");
        assert_eq!(updated.source.kind, crate::manifest::SourceKind::Imported);
        assert!(updated.source.immutable);
        let manifest = store.source_manifest(&folder).unwrap();
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].sha256, checksum(b"structure-bytes"));
        assert!(store
            .attach_source(&folder, &source, "../escape.bin")
            .is_err());
    }

    #[test]
    fn end_session_clears_only_recreatable_cache() {
        let root = temp_root("cache");
        let store = ProjectStore::new(&root);
        let detail = store
            .create("Cache", CreationMode::Empty, EditorState::default())
            .expect("create");
        let folder = detail.summary.folder.clone();
        let dir = root.join(&folder);
        fs::write(dir.join(CACHE_DIR).join("thumbnail.bin"), b"cache").unwrap();
        let manifest = store.manifest(&dir).unwrap();
        store.begin_session(&folder, 1).unwrap();
        store.end_session(&folder).unwrap();
        assert!(!dir.join(SESSION_MARKER).exists());
        assert!(dir.join(CACHE_DIR).is_dir());
        assert_eq!(fs::read_dir(dir.join(CACHE_DIR)).unwrap().count(), 0);
        assert!(dir.join(&manifest.world_file).exists());
        assert!(dir.join(PROJECT_FILE).exists());
    }

    #[test]
    fn duplicate_names_get_unique_folders() {
        let root = temp_root("duplicate");
        let store = ProjectStore::new(&root);
        let first = store
            .create("Same", CreationMode::Empty, EditorState::default())
            .expect("first");
        let second = store
            .create("Same", CreationMode::Empty, EditorState::default())
            .expect("second");
        assert_ne!(first.summary.folder, second.summary.folder);
        assert_eq!(store.list().projects.len(), 2);
    }

    #[test]
    fn snapshot_round_trip_preserves_states_across_dimensions() {
        let mut world = World::new_empty();
        let stairs: world_domain::BlockState =
            "minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=false]"
                .parse()
                .unwrap();
        world
            .set_block("minecraft:the_nether", BlockPos::new(-30, 40, 19), &stairs)
            .unwrap();
        world
            .set_block("minecraft:overworld", BlockPos::new(0, -64, 0), &stairs)
            .unwrap();
        world.bump_revision();
        let bytes = encode_world(&world).unwrap();
        let decoded = decode_world(&bytes).unwrap();
        assert_eq!(decoded.revision(), 1);
        assert_eq!(
            decoded
                .get_block("minecraft:the_nether", BlockPos::new(-30, 40, 19))
                .unwrap(),
            stairs
        );
        assert_eq!(encode_world(&decoded).unwrap(), bytes);
        assert_eq!(chunk_positions(&decoded, "minecraft:overworld").len(), 1);
    }
}
