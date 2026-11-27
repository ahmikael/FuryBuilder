//! Conversion of pre-1.8 worlds through the exact official server upgrade path.
//!
//! A pre-1.8 save is copied byte for byte into an isolated conversion run, the exact
//! official server JAR of a writable modern target is started with `--forceUpgrade`, and
//! the upgraded copy is reopened and decoded before it is reported as importable. The
//! source world is never started or written. The target profile is restricted to the
//! flattened/modern classic releases because `--forceUpgrade` is a 1.13+ server feature
//! and those are the releases FuryBuilder can import after the upgrade.
//!
//! This module owns process supervision and run ownership only; mapping counts and the
//! user-facing conversion report are added by the next legacy stage.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use minecraft_version::{profile_for_id, VersionFamily, VersionProfile, WorldLayout};
use serde::{Deserialize, Serialize};
use world_io::{
    analyze_java_world, load_java_world, passthrough_legacy_world, LegacyWorld, LevelDat,
};

use super::generation::{
    jvm_path, now_millis, pick_loopback_port, strip_ansi, write_json, FLATTENED_LOG_CONFIG,
    FLATTENED_LOG_CONFIG_NAME,
};
use super::process_tree::ProcessTree;
use super::types::SavedEula;
use crate::error::ServerError;

pub const CONVERSION_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const CONVERSION_MANIFEST_NAME: &str = ".furybuilder-conversion.json";
pub const CONVERSION_REPORT_NAME: &str = "report.json";
pub(crate) const CONVERSIONS_DIR_NAME: &str = "conversions";
const MAX_CONSOLE_TAIL: usize = 160;
const TICK: Duration = Duration::from_millis(200);
const SAVE_TIMEOUT: Duration = Duration::from_secs(90);
const STOP_TIMEOUT: Duration = Duration::from_secs(45);
const MIN_MEMORY_MB: u32 = 512;
const MAX_MEMORY_MB: u32 = 4096;
const MIN_TIME_LIMIT_SECONDS: u32 = 10;
const MAX_TIME_LIMIT_SECONDS: u32 = 3600;
const MIN_CPU_THREADS: u8 = 1;
const MAX_CPU_THREADS: u8 = 8;

/// Runtime knobs for one conversion run. The target and source profiles are selected by
/// id on the manager API; they are never inferred from a server JAR.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionRequest {
    /// Separate from the EULA: the user explicitly starts the local Java process.
    #[serde(default)]
    pub consent: bool,
    pub memory_mb: u32,
    pub cpu_threads: u8,
    pub time_limit_seconds: u32,
}

/// A completed conversion run. The world directory is verified and is input for the normal
/// project import path; the original source is not referenced by the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertedWorld {
    pub schema_version: u32,
    pub run_id: String,
    pub source_path: String,
    pub source_profile_id: String,
    pub source_minecraft_version: String,
    pub source_layout: String,
    pub source_files: usize,
    pub source_bytes: u64,
    pub source_chunks: usize,
    pub source_regions: usize,
    pub target_profile_id: String,
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub server_version: String,
    pub server_sha1: String,
    pub java_version: String,
    pub chunks: usize,
    pub regions: usize,
    pub dimensions: usize,
    pub world_bytes: u64,
    pub world_dir: String,
    pub log_path: String,
    pub source_unchanged: bool,
    pub created_at: u64,
    pub finished_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConversionManifest {
    schema_version: u32,
    run_id: String,
    created_at: u64,
    source_path: String,
    source_profile_id: String,
    source_minecraft_version: String,
    source_layout: String,
    source_files: usize,
    source_bytes: u64,
    source_chunks: usize,
    source_regions: usize,
    target_profile_id: String,
    target_minecraft_version: String,
    target_data_version: i32,
    server_version: String,
    server_sha1: String,
    eula_accepted_at: u64,
    consent_at: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConversionRuntime<'a> {
    pub java_exe: &'a Path,
    pub java_version: &'a str,
    pub server_jar: &'a Path,
    pub server_version: &'a str,
    pub server_sha1: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct ConversionPlan {
    run_id: String,
    run_dir: PathBuf,
    world_dir: PathBuf,
    log_path: PathBuf,
    source_path: PathBuf,
    source_profile: VersionProfile,
    target_profile: VersionProfile,
    source_files: usize,
    source_bytes: u64,
    source_chunks: usize,
    source_regions: usize,
    java_exe: PathBuf,
    java_version: String,
    server_jar: PathBuf,
    server_version: String,
    server_sha1: String,
    memory_mb: u32,
    cpu_threads: u8,
    time_limit_seconds: u32,
    created_at: u64,
}

impl ConversionPlan {
    pub(crate) fn run_dir(&self) -> &Path {
        &self.run_dir
    }
}

/// Removes the run directory unless the caller explicitly keeps the completed conversion.
pub(crate) struct ConversionRun {
    path: Option<PathBuf>,
}

impl ConversionRun {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub(crate) fn keep(mut self) {
        self.path = None;
    }
}

impl Drop for ConversionRun {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

/// Checks that the selected source and target can be connected by an official server
/// upgrade. Downgrades, same-version requests, read-only targets, split layouts and
/// targets without an official server JAR are refused before anything is copied.
pub(crate) fn validate_pair(
    source: &VersionProfile,
    target: &VersionProfile,
) -> Result<(), ServerError> {
    if source.family != VersionFamily::Legacy
        || !matches!(
            source.world_layout,
            WorldLayout::McRegion | WorldLayout::ClassicAnvil
        )
    {
        return Err(ServerError::UnsupportedConversion {
            profile_id: source.id.clone(),
            detail: "the source profile is not a pre-1.8 McRegion or classic Anvil release"
                .to_owned(),
        });
    }
    if target.family != VersionFamily::Flattened && target.family != VersionFamily::Modern {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target.id.clone(),
            detail: "the target must be a flattened or modern classic release from 1.13 onward"
                .to_owned(),
        });
    }
    if target.world_layout != WorldLayout::ClassicAnvil {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target.id.clone(),
            detail: "the target is not a classic Anvil world".to_owned(),
        });
    }
    if target.data_version <= 0 {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target.id.clone(),
            detail: "the target does not declare a positive DataVersion".to_owned(),
        });
    }
    if !target.capabilities.read || !target.capabilities.write || !target.capabilities.export {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target.id.clone(),
            detail: "the target must support import, writing and export".to_owned(),
        });
    }
    if !target.server.official_jar {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target.id.clone(),
            detail: "the target has no official server JAR".to_owned(),
        });
    }
    if target.minecraft_version <= source.minecraft_version {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target.id.clone(),
            detail: format!(
                "Minecraft {} cannot be converted to {}; downgrade and same-version conversion are refused",
                source.minecraft_version, target.minecraft_version
            ),
        });
    }
    Ok(())
}

pub(crate) fn validate_request(request: &ConversionRequest) -> Result<(), ServerError> {
    if !request.consent {
        return Err(ServerError::ConsentRequired(
            "confirm that the official server will be started locally for this conversion"
                .to_owned(),
        ));
    }
    if !(MIN_MEMORY_MB..=MAX_MEMORY_MB).contains(&request.memory_mb) {
        return Err(ServerError::Limit(format!(
            "conversion memory must be between {MIN_MEMORY_MB} and {MAX_MEMORY_MB} MiB"
        )));
    }
    if !(MIN_CPU_THREADS..=MAX_CPU_THREADS).contains(&request.cpu_threads) {
        return Err(ServerError::Limit(format!(
            "conversion CPU threads must be between {MIN_CPU_THREADS} and {MAX_CPU_THREADS}"
        )));
    }
    if !(MIN_TIME_LIMIT_SECONDS..=MAX_TIME_LIMIT_SECONDS).contains(&request.time_limit_seconds) {
        return Err(ServerError::Limit(format!(
            "conversion time limit must be between {MIN_TIME_LIMIT_SECONDS} and {MAX_TIME_LIMIT_SECONDS} seconds"
        )));
    }
    Ok(())
}

/// Copies the selected source into a new isolated conversion run. The source is validated
/// for the exact legacy profile before any byte is copied.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_run(
    conversions_root: &Path,
    source: &Path,
    source_profile: &VersionProfile,
    target_profile: &VersionProfile,
    runtime: ConversionRuntime<'_>,
    request: &ConversionRequest,
    eula: &SavedEula,
    consent_at: u64,
) -> Result<ConversionPlan, ServerError> {
    validate_pair(source_profile, target_profile)?;
    validate_request(request)?;
    if !eula.matches(
        target_profile.minecraft_version.as_str(),
        runtime.server_sha1,
    ) {
        return Err(ServerError::EulaRequired(format!(
            "the EULA acceptance does not cover the exact Minecraft {} server JAR",
            target_profile.minecraft_version
        )));
    }
    if profile_for_id(&source_profile.id).as_ref() != Some(source_profile)
        || profile_for_id(&target_profile.id).as_ref() != Some(target_profile)
    {
        return Err(ServerError::UnsupportedConversion {
            profile_id: target_profile.id.clone(),
            detail: "source and target must be exact built-in profiles".to_owned(),
        });
    }

    fs::create_dir_all(conversions_root).map_err(|error| {
        ServerError::Io(format!(
            "cannot create the conversion runs directory {}: {error}",
            conversions_root.display()
        ))
    })?;
    let created_at = now_millis();
    let run_id = format!("convert-{created_at}-{}", std::process::id());
    let run_dir = conversions_root.join(&run_id);
    if run_dir.exists() {
        return Err(ServerError::Invalid(format!(
            "{} already exists",
            run_dir.display()
        )));
    }
    fs::create_dir_all(run_dir.join("logs")).map_err(|error| {
        ServerError::Io(format!(
            "cannot create the isolated conversion directory {}: {error}",
            run_dir.display()
        ))
    })?;
    let port = match pick_loopback_port() {
        Ok(port) => port,
        Err(error) => {
            let _ = fs::remove_dir_all(&run_dir);
            return Err(error);
        }
    };
    let world_dir = run_dir.join("world");
    let source_path = fs::canonicalize(source).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the source world {}: {error}",
            source.display()
        ))
    })?;
    let copy = match passthrough_legacy_world(&source_path, &world_dir, source_profile) {
        Ok(copy) => copy,
        Err(error) => {
            let _ = fs::remove_dir_all(&run_dir);
            return Err(ServerError::Invalid(format!(
                "cannot copy and validate the pre-1.8 source: {error}"
            )));
        }
    };
    let manifest = ConversionManifest {
        schema_version: CONVERSION_MANIFEST_SCHEMA_VERSION,
        run_id: run_id.clone(),
        created_at,
        source_path: source_path.display().to_string(),
        source_profile_id: source_profile.id.clone(),
        source_minecraft_version: source_profile.minecraft_version.to_string(),
        source_layout: layout_name(source_profile.world_layout).to_owned(),
        source_files: copy.files.len(),
        source_bytes: copy.bytes(),
        source_chunks: copy.chunks,
        source_regions: copy.regions,
        target_profile_id: target_profile.id.clone(),
        target_minecraft_version: target_profile.minecraft_version.to_string(),
        target_data_version: target_profile.data_version,
        server_version: runtime.server_version.to_owned(),
        server_sha1: runtime.server_sha1.to_owned(),
        eula_accepted_at: eula.accepted_at,
        consent_at,
    };
    let result = (|| -> Result<(), ServerError> {
        write_json(&run_dir.join(CONVERSION_MANIFEST_NAME), &manifest)?;
        fs::write(run_dir.join("server.properties"), render_properties(port))
            .map_err(|error| ServerError::Io(format!("cannot write server.properties: {error}")))?;
        fs::write(run_dir.join("eula.txt"), b"eula=true\n")
            .map_err(|error| ServerError::Io(format!("cannot write eula.txt: {error}")))?;
        if target_profile.family == VersionFamily::Flattened {
            fs::write(
                run_dir.join(FLATTENED_LOG_CONFIG_NAME),
                FLATTENED_LOG_CONFIG,
            )
            .map_err(|error| ServerError::Io(format!("cannot write Log4j settings: {error}")))?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&run_dir);
        return Err(error);
    }
    Ok(ConversionPlan {
        run_id,
        log_path: run_dir.join("logs").join("furybuilder-conversion.log"),
        run_dir,
        world_dir,
        source_path,
        source_profile: source_profile.clone(),
        target_profile: target_profile.clone(),
        source_files: manifest.source_files,
        source_bytes: manifest.source_bytes,
        source_chunks: manifest.source_chunks,
        source_regions: manifest.source_regions,
        java_exe: runtime.java_exe.to_path_buf(),
        java_version: runtime.java_version.to_owned(),
        server_jar: runtime.server_jar.to_path_buf(),
        server_version: runtime.server_version.to_owned(),
        server_sha1: runtime.server_sha1.to_owned(),
        memory_mb: request.memory_mb,
        cpu_threads: request.cpu_threads,
        time_limit_seconds: request.time_limit_seconds,
        created_at,
    })
}

fn render_properties(port: u16) -> String {
    format!(
        "server-ip=127.0.0.1\n\
         server-port={port}\n\
         online-mode=false\n\
         enable-rcon=false\n\
         enable-query=false\n\
         enable-status=false\n\
         level-name=world\n\
         max-players=0\n\
         view-distance=2\n\
         spawn-protection=0\n\
         difficulty=peaceful\n\
         allow-flight=true\n\
         sync-chunk-writes=true\n\
         motd=FuryBuilder world conversion\n"
    )
}

/// Runs the already-prepared conversion, then reopens the upgraded world and decodes every
/// chunk. The caller owns cleanup of `plan.run_dir` when this returns an error.
pub(crate) fn execute(plan: &ConversionPlan) -> Result<ConvertedWorld, ServerError> {
    let console = Console::new(&plan.log_path);
    run_server(plan, console)?;
    let verified = verify_upgraded_world(plan)?;
    let source_unchanged = LegacyWorld::open(&plan.source_path, &plan.source_profile).is_ok();
    if !source_unchanged {
        return Err(ServerError::Verification(
            "the original pre-1.8 source changed during conversion; the result is not published"
                .to_owned(),
        ));
    }
    let finished_at = now_millis();
    let report = ConvertedWorld {
        schema_version: CONVERSION_MANIFEST_SCHEMA_VERSION,
        run_id: plan.run_id.clone(),
        source_path: plan.source_path.display().to_string(),
        source_profile_id: plan.source_profile.id.clone(),
        source_minecraft_version: plan.source_profile.minecraft_version.to_string(),
        source_layout: layout_name(plan.source_profile.world_layout).to_owned(),
        source_files: plan.source_files,
        source_bytes: plan.source_bytes,
        source_chunks: plan.source_chunks,
        source_regions: plan.source_regions,
        target_profile_id: plan.target_profile.id.clone(),
        target_minecraft_version: plan.target_profile.minecraft_version.to_string(),
        target_data_version: plan.target_profile.data_version,
        server_version: plan.server_version.clone(),
        server_sha1: plan.server_sha1.clone(),
        java_version: plan.java_version.clone(),
        chunks: verified.chunks,
        regions: verified.regions,
        dimensions: verified.dimensions,
        world_bytes: verified.world_bytes,
        world_dir: plan.world_dir.display().to_string(),
        log_path: plan.log_path.display().to_string(),
        source_unchanged,
        created_at: plan.created_at,
        finished_at,
    };
    write_json(&plan.run_dir.join(CONVERSION_REPORT_NAME), &report)?;
    Ok(report)
}

struct VerifiedWorld {
    chunks: usize,
    regions: usize,
    dimensions: usize,
    world_bytes: u64,
}

fn verify_upgraded_world(plan: &ConversionPlan) -> Result<VerifiedWorld, ServerError> {
    let level = LevelDat::read(plan.world_dir.join("level.dat")).map_err(|error| {
        ServerError::Verification(format!("the upgraded level.dat is invalid: {error}"))
    })?;
    if level.data_version != plan.target_profile.data_version {
        return Err(ServerError::Verification(format!(
            "the upgraded world declares DataVersion {}, expected {} for Minecraft {}",
            level.data_version,
            plan.target_profile.data_version,
            plan.target_profile.minecraft_version
        )));
    }
    if let Some(name) = level.version_name.as_deref() {
        if name != plan.target_profile.minecraft_version.as_str() {
            return Err(ServerError::Verification(format!(
                "the upgraded world declares Minecraft {name}, expected {}",
                plan.target_profile.minecraft_version
            )));
        }
    }
    let analysis = analyze_java_world(&plan.world_dir);
    if !analysis.supported {
        return Err(ServerError::Verification(format!(
            "the upgraded world is not importable: {}",
            if analysis.blockers.is_empty() {
                "the analysis did not admit it".to_owned()
            } else {
                analysis.blockers.join("; ")
            }
        )));
    }
    if analysis.data_version != Some(plan.target_profile.data_version) {
        return Err(ServerError::Verification(format!(
            "the upgraded analysis reports DataVersion {:?}, expected {}",
            analysis.data_version, plan.target_profile.data_version
        )));
    }
    let (_, load) = load_java_world(&plan.world_dir).map_err(|error| {
        ServerError::Verification(format!(
            "the upgraded world could not be decoded after conversion: {error}"
        ))
    })?;
    Ok(VerifiedWorld {
        chunks: load.chunks,
        regions: analysis.regions,
        dimensions: load.dimensions.len(),
        world_bytes: analysis.source_bytes,
    })
}

fn layout_name(layout: WorldLayout) -> &'static str {
    match layout {
        WorldLayout::McRegion => "mc_region",
        WorldLayout::ClassicAnvil => "classic_anvil",
        WorldLayout::SplitAnvil => "split_anvil",
        WorldLayout::Unknown => "unknown",
    }
}

#[derive(Clone)]
struct Console {
    inner: Arc<Mutex<ConsoleState>>,
}

struct ConsoleState {
    file: Option<File>,
    tail: VecDeque<String>,
}

impl Console {
    fn new(path: &Path) -> Self {
        let file = if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
            OpenOptions::new().create(true).append(true).open(path).ok()
        } else {
            None
        };
        Self {
            inner: Arc::new(Mutex::new(ConsoleState {
                file,
                tail: VecDeque::new(),
            })),
        }
    }

    fn push(&self, line: &str) {
        if let Ok(mut state) = self.inner.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = writeln!(file, "{line}");
            }
            if state.tail.len() >= MAX_CONSOLE_TAIL {
                state.tail.pop_front();
            }
            state.tail.push_back(line.to_owned());
        }
    }

    fn tail(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|state| state.tail.iter().cloned().collect())
            .unwrap_or_default()
    }
}

struct ServerProcess {
    child: Child,
    tree: ProcessTree,
}

impl ServerProcess {
    fn terminate(&mut self) {
        self.tree.terminate(&mut self.child);
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.tree.terminate(&mut self.child);
    }
}

struct Live {
    started: bool,
    saved: bool,
    last_error: Option<String>,
}

fn run_server(plan: &ConversionPlan, console: Console) -> Result<ExitStatus, ServerError> {
    let mut command = build_command(plan);
    let child = command.spawn().map_err(|error| {
        ServerError::Io(format!(
            "cannot start the official Minecraft {} server: {error}",
            plan.target_profile.minecraft_version
        ))
    })?;
    let mut tree = ProcessTree::prepare(Some(u64::from(plan.memory_mb) * 1024 * 1024));
    tree.attach(&child);
    let mut process = ServerProcess { child, tree };
    let mut stdin = process
        .child
        .stdin
        .take()
        .ok_or_else(|| ServerError::Io("the conversion server has no stdin".to_owned()))?;
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| ServerError::Io("the conversion server has no stdout".to_owned()))?;
    let stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| ServerError::Io("the conversion server has no stderr".to_owned()))?;
    let (sender, receiver) = mpsc::channel();
    let stdout_reader = spawn_reader(stdout, sender.clone(), console.clone());
    let stderr_reader = spawn_reader(stderr, sender, console.clone());
    let status = supervise(
        &mut process,
        &mut stdin,
        &receiver,
        plan.time_limit_seconds,
        &plan.log_path,
        &console,
    );
    process.terminate();
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    let status = status?;
    if !status.success() {
        return Err(ServerError::Invalid(format!(
            "the official server exited with {} before the conversion completed; log: {}{}",
            status
                .code()
                .map_or_else(|| "an unknown status".to_owned(), |code| code.to_string()),
            plan.log_path.display(),
            tail_suffix(&console)
        )));
    }
    Ok(status)
}

fn supervise(
    process: &mut ServerProcess,
    stdin: &mut impl Write,
    receiver: &Receiver<String>,
    time_limit_seconds: u32,
    log_path: &Path,
    console: &Console,
) -> Result<ExitStatus, ServerError> {
    let started_at = Instant::now();
    let limit = Duration::from_secs(u64::from(time_limit_seconds));
    let mut live = Live {
        started: false,
        saved: false,
        last_error: None,
    };
    let mut save_sent = false;
    let mut save_started = Instant::now();
    let mut stop_sent = false;
    let mut stop_started = Instant::now();
    loop {
        drain_lines(receiver, &mut live, TICK);
        if let Some(status) = process.child.try_wait().map_err(|error| {
            ServerError::Io(format!("cannot inspect the conversion server: {error}"))
        })? {
            return Ok(status);
        }
        if !live.started {
            if started_at.elapsed() > limit {
                process.terminate();
                return Err(ServerError::Invalid(format!(
                    "the official server did not finish startup within {} seconds; log: {}{}",
                    time_limit_seconds,
                    log_path.display(),
                    tail_suffix(console)
                )));
            }
        } else if !save_sent {
            send(stdin, "save-all flush\n", log_path, console)?;
            save_sent = true;
            save_started = Instant::now();
        } else if !live.saved {
            if save_started.elapsed() > SAVE_TIMEOUT {
                process.terminate();
                return Err(ServerError::Invalid(format!(
                    "the official server did not confirm a save within {} seconds; log: {}{}",
                    SAVE_TIMEOUT.as_secs(),
                    log_path.display(),
                    tail_suffix(console)
                )));
            }
        } else if !stop_sent {
            send(stdin, "stop\n", log_path, console)?;
            stop_sent = true;
            stop_started = Instant::now();
        } else if stop_started.elapsed() > STOP_TIMEOUT {
            process.terminate();
            return Err(ServerError::Invalid(format!(
                "the official server did not stop within {} seconds; log: {}{}",
                STOP_TIMEOUT.as_secs(),
                log_path.display(),
                tail_suffix(console)
            )));
        }
        if started_at.elapsed() > limit {
            process.terminate();
            return Err(ServerError::Invalid(format!(
                "the conversion exceeded the {} second limit; log: {}{}",
                time_limit_seconds,
                log_path.display(),
                tail_suffix(console)
            )));
        }
        thread::sleep(TICK);
    }
}

fn send(
    stdin: &mut impl Write,
    command: &str,
    log_path: &Path,
    console: &Console,
) -> Result<(), ServerError> {
    stdin
        .write_all(command.as_bytes())
        .and_then(|()| stdin.flush())
        .map_err(|error| {
            ServerError::Invalid(format!(
                "cannot send `{}` to the official server: {error}; log: {}{}",
                command.trim(),
                log_path.display(),
                tail_suffix(console)
            ))
        })
}

fn tail_suffix(console: &Console) -> String {
    let tail = console.tail();
    if tail.is_empty() {
        String::new()
    } else {
        format!("; console tail: {}", tail.join(" | "))
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    pipe: R,
    sender: mpsc::Sender<String>,
    console: Console,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines() {
            match line {
                Ok(line) => {
                    console.push(&line);
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn drain_lines(receiver: &Receiver<String>, live: &mut Live, timeout: Duration) {
    let mut received = false;
    for _ in 0..64 {
        match receiver.try_recv() {
            Ok(line) => {
                absorb(&line, live);
                received = true;
            }
            Err(_) => break,
        }
    }
    if !received {
        if let Ok(line) = receiver.recv_timeout(timeout) {
            absorb(&line, live);
        }
    }
}

fn absorb(line: &str, live: &mut Live) {
    let clean = strip_ansi(line);
    if clean.contains("Done (") && clean.contains("For help") {
        live.started = true;
    }
    if clean.contains("Saved the game") {
        live.saved = true;
    }
    if live.last_error.is_none()
        && (clean.contains("ERROR]") || clean.contains("Exception") || clean.contains("Caused by"))
    {
        live.last_error = Some(clean);
    }
}

fn build_command(plan: &ConversionPlan) -> Command {
    let mut command = Command::new(jvm_path(&plan.java_exe));
    if plan.target_profile.family == VersionFamily::Flattened {
        command.arg(format!(
            "-Dlog4j.configurationFile={FLATTENED_LOG_CONFIG_NAME}"
        ));
    }
    command
        .current_dir(jvm_path(&plan.run_dir))
        .arg(format!("-Xms{}M", (plan.memory_mb / 4).clamp(128, 512)))
        .arg(format!("-Xmx{}M", plan.memory_mb))
        .arg(format!("-XX:ActiveProcessorCount={}", plan.cpu_threads))
        .arg("-Dfile.encoding=UTF-8")
        .arg("-Duser.language=en")
        .arg("-Duser.country=US")
        .arg("-Djava.awt.headless=true")
        .arg("-jar")
        .arg(jvm_path(&plan.server_jar))
        .arg("--forceUpgrade")
        .arg("nogui")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// Loads a completed conversion run only when its manifest and report agree and the world
/// is still the exact directory owned by that run.
pub(crate) fn load_converted_world(
    conversions_root: &Path,
    run_id: &str,
) -> Result<ConvertedWorld, ServerError> {
    if !valid_run_id(run_id) {
        return Err(ServerError::Invalid(
            "the conversion run id is not a valid FuryBuilder run id".to_owned(),
        ));
    }
    let root = fs::canonicalize(conversions_root).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the conversion runs root {}: {error}",
            conversions_root.display()
        ))
    })?;
    let run_dir = root.join(run_id);
    let metadata = fs::symlink_metadata(&run_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "the conversion run {run_id} is not available: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ServerError::Invalid(format!(
            "the conversion run {run_id} is not a regular directory"
        )));
    }
    let run_dir = fs::canonicalize(&run_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the conversion run {run_id}: {error}"
        ))
    })?;
    if run_dir.parent() != Some(root.as_path()) {
        return Err(ServerError::Invalid(format!(
            "the conversion run {run_id} escapes the runs root"
        )));
    }
    let manifest: ConversionManifest = read_json(
        &run_dir.join(CONVERSION_MANIFEST_NAME),
        "conversion manifest",
    )?;
    if manifest.schema_version != CONVERSION_MANIFEST_SCHEMA_VERSION || manifest.run_id != run_id {
        return Err(ServerError::Invalid(format!(
            "the conversion manifest for {run_id} does not match this run"
        )));
    }
    let report: ConvertedWorld =
        read_json(&run_dir.join(CONVERSION_REPORT_NAME), "conversion report")?;
    if report.schema_version != CONVERSION_MANIFEST_SCHEMA_VERSION
        || report.run_id != run_id
        || report.source_profile_id != manifest.source_profile_id
        || report.target_profile_id != manifest.target_profile_id
        || report.target_data_version != manifest.target_data_version
        || report.server_sha1 != manifest.server_sha1
    {
        return Err(ServerError::Invalid(format!(
            "the conversion report for {run_id} does not match its manifest"
        )));
    }
    let world_dir = run_dir.join("world");
    let metadata = fs::symlink_metadata(&world_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "the conversion run {run_id} has no world directory: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ServerError::Invalid(format!(
            "the conversion world for {run_id} is not a regular directory"
        )));
    }
    let world_dir = fs::canonicalize(&world_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the conversion world for {run_id}: {error}"
        ))
    })?;
    if world_dir.parent() != Some(run_dir.as_path()) {
        return Err(ServerError::Invalid(format!(
            "the conversion world for {run_id} escapes its run directory"
        )));
    }
    let reported_world = fs::canonicalize(Path::new(&report.world_dir)).map_err(|error| {
        ServerError::Invalid(format!(
            "the reported conversion world for {run_id} is missing: {error}"
        ))
    })?;
    if reported_world != world_dir {
        return Err(ServerError::Invalid(format!(
            "the conversion report for {run_id} points at a different world"
        )));
    }
    let level = LevelDat::read(world_dir.join("level.dat")).map_err(|error| {
        ServerError::Invalid(format!(
            "the conversion world for {run_id} is not importable: {error}"
        ))
    })?;
    if level.data_version != manifest.target_data_version {
        return Err(ServerError::Invalid(format!(
            "the conversion world for {run_id} declares DataVersion {}, expected {}",
            level.data_version, manifest.target_data_version
        )));
    }
    Ok(report)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, label: &str) -> Result<T, ServerError> {
    let bytes = fs::read(path).map_err(|error| {
        ServerError::Invalid(format!(
            "the conversion run is missing its {label} ({}): {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        ServerError::Invalid(format!(
            "{} is not a valid {label}: {error}",
            path.display()
        ))
    })
}

fn valid_run_id(run_id: &str) -> bool {
    let mut parts = run_id.split('-');
    let (Some("convert"), Some(millis), Some(pid), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !millis.is_empty()
        && !pid.is_empty()
        && millis.len() <= 20
        && pid.len() <= 20
        && millis.bytes().all(|byte| byte.is_ascii_digit())
        && pid.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FAKE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "furybuilder-conversion-{label}-{}-{}-{}",
            std::process::id(),
            now_millis(),
            FAKE_COUNTER.fetch_add(1, Ordering::SeqCst)
        ))
    }

    fn eula_for(target: &VersionProfile, sha1: &str) -> SavedEula {
        SavedEula {
            accepted_at: 1,
            minecraft_version: target.minecraft_version.to_string(),
            jar_sha1: sha1.to_owned(),
            eula_url: super::super::types::EULA_URL.to_owned(),
        }
    }

    fn profile_of(version: &str) -> VersionProfile {
        profile_for_id(&format!("java-classic-{version}"))
            .or_else(|| profile_for_id(&format!("java-legacy-{version}")))
            .unwrap_or_else(|| panic!("{version} profile"))
    }

    fn level_dat(data_version: Option<i32>, format_version: i32, version_name: &str) -> Vec<u8> {
        use world_io::{nbt, NbtFile, NbtValue};
        let data = if let Some(data_version) = data_version {
            vec![
                ("DataVersion", NbtValue::Int(data_version)),
                (
                    "Version",
                    NbtValue::compound([
                        ("Id", NbtValue::Int(data_version)),
                        ("Name", NbtValue::String(version_name.to_owned())),
                    ]),
                ),
                ("LevelName", NbtValue::String(version_name.to_owned())),
            ]
        } else {
            vec![
                ("Version", NbtValue::Int(format_version)),
                ("LevelName", NbtValue::String(version_name.to_owned())),
            ]
        };
        nbt::write_gzip(&NbtFile::new(
            "",
            NbtValue::compound([("Data", NbtValue::compound(data))]),
        ))
        .expect("encode level.dat")
    }

    fn write_source_world(path: &Path) {
        fs::create_dir_all(path.join("region")).expect("source region");
        fs::write(path.join("level.dat"), level_dat(None, 19133, "source"))
            .expect("source level.dat");
    }

    fn write_target_world(path: &Path, target: &VersionProfile) {
        fs::create_dir_all(path.join("region")).expect("target region");
        fs::write(
            path.join("level.dat"),
            level_dat(
                Some(target.data_version),
                0,
                target.minecraft_version.as_str(),
            ),
        )
        .expect("target level.dat");
    }

    #[test]
    fn conversion_pair_refuses_downgrades_and_non_legacy_sources() {
        let source = profile_of("1.7.10");
        let target = profile_of("1.13.2");
        assert!(validate_pair(&source, &target).is_ok());

        let older = profile_of("1.2.1");
        assert!(matches!(
            validate_pair(&source, &older),
            Err(ServerError::UnsupportedConversion { .. })
        ));
        let modern = profile_of("1.21.4");
        assert!(matches!(
            validate_pair(&modern, &target),
            Err(ServerError::UnsupportedConversion { .. })
        ));
        let metadata = profile_of("1.8.8");
        assert!(matches!(
            validate_pair(&metadata, &target),
            Err(ServerError::UnsupportedConversion { .. })
        ));
    }

    #[test]
    fn conversion_properties_are_isolated_and_never_generate_a_new_level() {
        let properties = render_properties(51234);
        assert!(properties.contains("server-ip=127.0.0.1\n"));
        assert!(properties.contains("server-port=51234\n"));
        assert!(properties.contains("level-name=world\n"));
        assert!(properties.contains("max-players=0\n"));
        assert!(properties.contains("enable-rcon=false\n"));
        assert!(!properties.contains("level-seed="));
        assert!(!properties.contains("level-type="));
        assert!(!properties.contains("server-ip=0.0.0.0"));
    }

    #[test]
    fn conversion_request_requires_consent_and_bounds() {
        let mut request = ConversionRequest {
            consent: false,
            memory_mb: 1024,
            cpu_threads: 2,
            time_limit_seconds: 600,
        };
        assert!(matches!(
            validate_request(&request),
            Err(ServerError::ConsentRequired(_))
        ));
        request.consent = true;
        assert!(validate_request(&request).is_ok());
        request.memory_mb = 1;
        assert!(matches!(
            validate_request(&request),
            Err(ServerError::Limit(_))
        ));
    }

    #[test]
    fn conversion_rejects_missing_eula_and_unbuilt_profiles() {
        let source = profile_of("1.7.10");
        let target = profile_of("1.13.2");
        let root = temp_root("preflight");
        let source_dir = root.join("source");
        write_source_world(&source_dir);
        let wrong = SavedEula {
            accepted_at: 1,
            minecraft_version: target.minecraft_version.to_string(),
            jar_sha1: "0".repeat(40),
            eula_url: super::super::types::EULA_URL.to_owned(),
        };
        let request = ConversionRequest {
            consent: true,
            memory_mb: 1024,
            cpu_threads: 2,
            time_limit_seconds: 600,
        };
        let error = prepare_run(
            &root.join("runs"),
            &source_dir,
            &source,
            &target,
            ConversionRuntime {
                java_exe: Path::new("java"),
                java_version: "test",
                server_jar: Path::new("server.jar"),
                server_version: "1.13.2",
                server_sha1: &"1".repeat(40),
            },
            &request,
            &wrong,
            2,
        )
        .expect_err("mismatched EULA must be refused");
        assert!(matches!(error, ServerError::EulaRequired(_)));
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    const FAKE_SERVER: &str = r#"
using System;
using System.IO;
public class FakeConversionServer {
    static void Copy(string source, string target) {
        Directory.CreateDirectory(target);
        foreach (var dir in Directory.GetDirectories(source)) {
            Copy(dir, Path.Combine(target, Path.GetFileName(dir)));
        }
        foreach (var file in Directory.GetFiles(source)) {
            File.Copy(file, Path.Combine(target, Path.GetFileName(file)), true);
        }
    }
    public static int Main(string[] args) {
        if (Array.IndexOf(args, "--forceUpgrade") < 0) return 9;
        var upgraded = Path.Combine("..", "upgraded");
        if (Directory.Exists("world")) Directory.Delete("world", true);
        Copy(upgraded, "world");
        Console.WriteLine("[12:00:00] [Server thread/INFO]: Starting Minecraft server on 127.0.0.1:25565");
        Console.WriteLine("[12:00:00] [Server thread/INFO]: Done (0.1s)! For help, type \"help\"");
        Console.Out.Flush();
        string line;
        while ((line = Console.ReadLine()) != null) {
            if (line == "save-all flush") {
                Console.WriteLine("[12:00:01] [Server thread/INFO]: Saved the game");
                Console.Out.Flush();
            }
            if (line == "stop") {
                Console.WriteLine("[12:00:02] [Server thread/INFO]: Stopping the server");
                Console.Out.Flush();
                return 0;
            }
        }
        return 3;
    }
}
"#;

    #[cfg(windows)]
    fn compile_fake(root: &Path) -> PathBuf {
        let cs = root.join("fake.cs");
        let exe = root.join("fake-java.exe");
        let build = root.join("build.ps1");
        fs::write(&cs, FAKE_SERVER).expect("fake source");
        let cs_literal = cs.display().to_string().replace('\'', "''");
        let exe_literal = exe.display().to_string().replace('\'', "''");
        fs::write(
            &build,
            format!(
                "$ErrorActionPreference='Stop'\n$source = Get-Content -Raw -LiteralPath '{cs_literal}'\nAdd-Type -TypeDefinition $source -OutputAssembly '{exe_literal}' -OutputType ConsoleApplication\n"
            ),
        )
        .expect("build script");
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&build)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("powershell for the fake conversion server");
        assert!(
            status.success(),
            "fake conversion server compilation failed"
        );
        assert!(
            exe.is_file(),
            "fake conversion server executable is missing"
        );
        exe
    }

    #[cfg(windows)]
    #[test]
    fn fake_official_server_upgrades_a_copy_and_preserves_the_source() {
        let root = temp_root("fake");
        fs::create_dir_all(&root).expect("root");
        let source = profile_of("1.7.10");
        let target = profile_of("1.21.4");
        let source_dir = root.join("source");
        write_source_world(&source_dir);
        let target_dir = root.join("upgraded");
        write_target_world(&target_dir, &target);
        let java = compile_fake(&root);
        let request = ConversionRequest {
            consent: true,
            memory_mb: 1024,
            cpu_threads: 2,
            time_limit_seconds: 600,
        };
        let sha1 = "1".repeat(40);
        let eula = eula_for(&target, &sha1);
        let plan = prepare_run(
            &root.join("runs"),
            &source_dir,
            &source,
            &target,
            ConversionRuntime {
                java_exe: &java,
                java_version: "test",
                server_jar: &root.join("server.jar"),
                server_version: target.minecraft_version.as_str(),
                server_sha1: &sha1,
            },
            &request,
            &eula,
            2,
        )
        .expect("prepare conversion");
        // The fake server copies this fixture over the isolated world.
        fs::rename(&target_dir, plan.run_dir.parent().unwrap().join("upgraded"))
            .expect("move target fixture next to the run");
        let report = execute(&plan).expect("conversion report");
        assert_eq!(report.target_data_version, target.data_version);
        assert_eq!(report.source_minecraft_version, "1.7.10");
        assert_eq!(
            report.target_minecraft_version,
            target.minecraft_version.as_str()
        );
        assert!(report.source_unchanged);
        assert!(plan.world_dir.join("level.dat").is_file());
        assert_eq!(
            LevelDat::read(plan.world_dir.join("level.dat"))
                .expect("upgraded level.dat")
                .data_version,
            target.data_version
        );
        assert_eq!(
            LegacyWorld::open(&source_dir, &source)
                .expect("source still legacy")
                .level
                .format_version,
            19133
        );
        let loaded = load_converted_world(plan.run_dir.parent().unwrap(), &plan.run_id)
            .expect("completed conversion is reloadable");
        assert_eq!(loaded.run_id, plan.run_id);
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(windows)]
    #[test]
    fn a_failed_server_run_removes_the_isolated_copy() {
        let root = temp_root("failure");
        fs::create_dir_all(&root).expect("root");
        let source = profile_of("1.7.10");
        let target = profile_of("1.21.4");
        let source_dir = root.join("source");
        write_source_world(&source_dir);
        let java = compile_fake(&root);
        let sha1 = "1".repeat(40);
        let eula = eula_for(&target, &sha1);
        let plan = prepare_run(
            &root.join("runs"),
            &source_dir,
            &source,
            &target,
            ConversionRuntime {
                java_exe: &java,
                java_version: "test",
                server_jar: &root.join("server.jar"),
                server_version: target.minecraft_version.as_str(),
                server_sha1: &sha1,
            },
            &ConversionRequest {
                consent: true,
                memory_mb: 1024,
                cpu_threads: 2,
                time_limit_seconds: 10,
            },
            &eula,
            2,
        )
        .expect("prepare conversion");
        let run_dir = plan.run_dir.clone();
        let run = ConversionRun::new(run_dir.clone());
        // No upgraded fixture exists, so the fake server cannot replace the world and the
        // copied legacy world must not be published as a modern target.
        let error = execute(&plan).expect_err("missing target fixture must fail verification");
        assert!(matches!(
            error,
            ServerError::Verification(_) | ServerError::Invalid(_)
        ));
        assert!(source_dir.join("level.dat").is_file());
        drop(run);
        assert!(
            !run_dir.exists(),
            "the failed run is removed by ConversionRun"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
