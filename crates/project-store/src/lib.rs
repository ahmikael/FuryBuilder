//! Versioned FuryBuilder project folders.
//!
//! Layout:
//! ```text
//! MyProject.furybuilder/
//!   project.json        # versioned manifest, checksum of the active snapshot
//!   data/world-*.fwl    # deterministic binary world snapshots
//!   data/autosave.fwl   # crash-recovery snapshot (adopted only after a crash marker)
//!   changes/            # rolling checkpoints and their metadata
//!   source/             # immutable imported source files + manifest
//!   cache/              # recreatable caches, cleared on graceful close
//!   session.lock        # present while a session is open
//! ```
//!
//! Every publish is temp + verification + rename, and the world snapshot is verified by
//! decoding and re-encoding it before the manifest references it.

pub mod error;
pub mod manifest;
pub mod snapshot;
pub mod source;
pub mod store;

pub use error::ProjectError;
pub use manifest::{
    AssetsComponent, AssetsPack, ComponentMode, CreationMode, EditorState, JavaComponent,
    ProjectComponents, ProjectDetail, ProjectList, ProjectManifest, ProjectSummary, ProjectTarget,
    RecoveryInfo, SaveKind, SaveOutcome, ServerComponent, SourceInfo, SourceKind, VanillaDatapack,
    VanillaProvenance, PROJECT_EXTENSION, PROJECT_FILE, PROJECT_SCHEMA_VERSION,
};
pub use snapshot::{decode_world, encode_world, SNAPSHOT_FORMAT_VERSION};
pub use source::{SourceFile, SourceManifest};
pub use store::TEMPLATES_DIR;
pub use store::{
    build_world, checksum, create_flat_base, is_safe_file_name, snapshot_bytes, CheckpointMeta,
    ExportOutcome, OpenedCheckpoint, ProjectStore, AUTOSAVE_FILE, CACHE_DIR, CHECKPOINT_DIR,
    CHECKPOINT_SCHEMA_VERSION, DATA_DIR, EXPORTS_DIR, MAX_CHECKPOINTS, MAX_CHECKPOINT_TEXT_CHARS,
    MAX_MANIFEST_BACKUPS, MAX_SNAPSHOTS, MAX_SOURCE_FILES, SESSION_MARKER, SOURCE_DIR,
};
pub use workspace_model::WorkspacePolicy;
