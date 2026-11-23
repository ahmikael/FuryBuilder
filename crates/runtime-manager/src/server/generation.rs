//! Isolated Minecraft server supervision for Vanilla World generation (M9.5).
//!
//! A run directory is created only after the user explicitly accepted the Minecraft EULA for
//! the exact selected `server.jar`. The official server is started with:
//!
//! - `server-ip=127.0.0.1` and an ephemeral loopback port (no external listeners);
//! - RCON, query and status disabled, offline mode on;
//! - a bounded JVM heap and processor count;
//! - a Windows Job Object with kill-on-close and a process-memory cap, so cancel, app crash
//!   and forced termination always remove the whole Java/bundler process tree;
//! - an explicit wall-clock limit, periodic saves, progress from Anvil region headers and a
//!   bounded console tail.
//!
//! This module never downloads or starts anything by itself; it receives an inspected JAR,
//! a probed Java executable and an accepted EULA record.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::datapack;
use super::jar::{is_sha1, ServerJarInfo};
use super::process_tree::ProcessTree;
use super::types::{
    GeneratedWorld, GenerationDatapack, GenerationPhase, GenerationReport, GenerationRequest,
    GenerationRunInfo, GenerationRunsReport, GenerationSnapshot, GenerationStopReason, RunManifest,
    SavedEula, WorldPreset, RUN_MANIFEST_SCHEMA_VERSION,
};
use crate::error::ServerError;

pub(crate) const RUN_MANIFEST_NAME: &str = ".furybuilder-run.json";
pub(crate) const RUN_REPORT_NAME: &str = "report.json";
const MAX_CONSOLE_TAIL: usize = 120;
const MAX_REGION_FILES: usize = 4096;
const MAX_SCANNED_ENTRIES: usize = 20_000;
const TICK: Duration = Duration::from_millis(200);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(240);
const SAVE_TIMEOUT: Duration = Duration::from_secs(90);
const STOP_TIMEOUT: Duration = Duration::from_secs(45);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_SAVE_INTERVAL: Duration = Duration::from_secs(10);
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MIN_GENERATION: Duration = Duration::from_secs(5);
const MIN_MEMORY_MB: u32 = 512;
const MAX_MEMORY_MB: u32 = 4096;
const MIN_TIME_LIMIT_SECONDS: u32 = 10;
const MAX_TIME_LIMIT_SECONDS: u32 = 3600;
const MAX_RADIUS_CHUNKS: u32 = 16;
const MAX_WORLD_COORD: i32 = 29_999_968;

#[derive(Debug, Clone, Copy)]
pub(crate) struct GenerationRuntime<'a> {
    pub java_exe: &'a Path,
    pub java_version: &'a str,
    pub server_jar: &'a Path,
    pub server: &'a ServerJarInfo,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RunApproval<'a> {
    pub eula: &'a SavedEula,
    pub consent_at: u64,
}

#[derive(Debug)]
pub(crate) struct GenerationPlan {
    pub run_id: String,
    pub run_dir: PathBuf,
    pub world_dir: PathBuf,
    pub log_path: PathBuf,
    pub java_exe: PathBuf,
    pub java_version: String,
    pub server_jar: PathBuf,
    pub server_sha1: String,
    pub seed: String,
    pub preset: WorldPreset,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    pub time_limit_seconds: u32,
    pub memory_mb: u32,
    pub view_distance: u8,
    pub cpu_threads: u8,
    pub generate_structures: bool,
    pub eula_accepted_at: u64,
    pub consent_at: u64,
    pub datapacks: Vec<GenerationDatapack>,
    pub port: u16,
    pub expected_chunks: u64,
    pub idle_timeout: Duration,
    pub min_generation: Duration,
    pub save_interval: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct RegionStats {
    chunks: u64,
    region_files: u64,
    world_bytes: u64,
}

#[derive(Debug)]
struct Outcome {
    phase: GenerationPhase,
    stop_reason: GenerationStopReason,
    world_complete: bool,
    exit_code: Option<i32>,
    server_pid: Option<u32>,
    process_tree_guarded: bool,
    bind_address: Option<String>,
    message: String,
    error: Option<String>,
    stats: RegionStats,
}

#[derive(Debug, Clone, Copy)]
enum Decision {
    Complete,
    Idle,
    TimeLimit,
    Cancel,
    Crash(Option<i32>),
    StartupTimeout,
    IsolationViolation,
}

struct Live {
    started: bool,
    saved: bool,
    bind_address: Option<String>,
    forceload_sent: bool,
    isolation_checked: bool,
    last_chunks: u64,
    last_progress: Instant,
    last_save: Instant,
    last_error: Option<String>,
    spawn_percent: Option<u32>,
}

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

impl Clone for Console {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct GuardedChild {
    child: Child,
    tree: ProcessTree,
    exited: bool,
    exit_code: Option<i32>,
}

impl GuardedChild {
    fn spawn(command: &mut Command, memory_mb: u32) -> Result<Self, ServerError> {
        let mut tree = ProcessTree::prepare(Some((memory_mb as u64 + 1024) * 1024 * 1024));
        let child = command.spawn().map_err(|error| {
            ServerError::Io(format!(
                "cannot start the isolated Minecraft server: {error}"
            ))
        })?;
        tree.attach(&child);
        Ok(Self {
            child,
            tree,
            exited: false,
            exit_code: None,
        })
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn guarded(&self) -> bool {
        self.tree.guarded()
    }

    fn send(&mut self, line: &str) -> bool {
        let Some(stdin) = self.child.stdin.as_mut() else {
            return false;
        };
        writeln!(stdin, "{line}")
            .and_then(|_| stdin.flush())
            .is_ok()
    }

    /// `Some(code)` means the immediate child has exited. `None` means it is still running.
    /// The returned code is `None` when the OS terminated the process without an exit code.
    fn poll(&mut self) -> Option<Option<i32>> {
        if self.exited {
            return Some(self.exit_code);
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.exited = true;
                self.exit_code = status.code();
                Some(self.exit_code)
            }
            _ => None,
        }
    }

    fn terminate(&mut self) {
        self.tree.terminate(&mut self.child);
        if !self.exited {
            let _ = self.child.wait();
            self.exited = true;
        }
    }
}

impl Drop for GuardedChild {
    fn drop(&mut self) {
        if !self.exited {
            self.terminate();
        } else {
            // The bundler child may have exited while its Java grandchild is still alive.
            self.tree.terminate(&mut self.child);
        }
    }
}

pub(crate) fn expected_chunks(request: &GenerationRequest) -> u64 {
    let side = request.radius_chunks.saturating_mul(2).saturating_add(1) as u64;
    side.saturating_mul(side)
}

pub(crate) fn validate_request(request: &GenerationRequest) -> Result<(), ServerError> {
    if !request.consent {
        return Err(ServerError::ConsentRequired(
            "confirm that the official server will be started locally for this run".to_owned(),
        ));
    }
    let seed = request.seed.trim();
    if seed.is_empty() || seed.len() > 64 {
        return Err(ServerError::Invalid(
            "the seed must contain between 1 and 64 characters".to_owned(),
        ));
    }
    if !seed
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err(ServerError::Invalid(
            "the seed may only contain ASCII letters, digits, '-', '_' and '.'".to_owned(),
        ));
    }
    if !(1..=MAX_RADIUS_CHUNKS).contains(&request.radius_chunks) {
        return Err(ServerError::Limit(format!(
            "radius must be between 1 and {MAX_RADIUS_CHUNKS} chunks"
        )));
    }
    if !(MIN_TIME_LIMIT_SECONDS..=MAX_TIME_LIMIT_SECONDS).contains(&request.time_limit_seconds) {
        return Err(ServerError::Limit(format!(
            "time limit must be between {MIN_TIME_LIMIT_SECONDS} and {MAX_TIME_LIMIT_SECONDS} seconds"
        )));
    }
    if !(MIN_MEMORY_MB..=MAX_MEMORY_MB).contains(&request.memory_mb) {
        return Err(ServerError::Limit(format!(
            "memory limit must be between {MIN_MEMORY_MB} and {MAX_MEMORY_MB} MiB"
        )));
    }
    if !(2..=8).contains(&request.view_distance) {
        return Err(ServerError::Limit(
            "view distance must be between 2 and 8 chunks".to_owned(),
        ));
    }
    if !(1..=8).contains(&request.cpu_threads) {
        return Err(ServerError::Limit(
            "CPU threads must be between 1 and 8".to_owned(),
        ));
    }
    if request.datapacks.len() > datapack::MAX_DATAPACKS {
        return Err(ServerError::Limit(format!(
            "at most {} data packs can be used in one run",
            datapack::MAX_DATAPACKS
        )));
    }
    let radius_blocks = request.radius_chunks as i64 * 16 + 16;
    for (axis, value) in [("x", request.center_x), ("z", request.center_z)] {
        let limit = MAX_WORLD_COORD as i64 - radius_blocks;
        if (value as i64).abs() > limit {
            return Err(ServerError::Limit(format!(
                "center {axis}={value} is outside the supported world coordinates"
            )));
        }
    }
    Ok(())
}

/// Checks the two independent approvals before any run directory or process exists:
/// a per-run consent and an EULA acceptance for exactly this JAR and Minecraft version.
pub(crate) fn validate_approval(
    request: &GenerationRequest,
    approval: &RunApproval<'_>,
    server: &ServerJarInfo,
) -> Result<(), ServerError> {
    if !request.consent || approval.consent_at == 0 {
        return Err(ServerError::ConsentRequired(
            "confirm that the official server will be started locally for this run".to_owned(),
        ));
    }
    if !approval.eula.matches(&server.version, &server.sha1) {
        return Err(ServerError::EulaRequired(format!(
            "the EULA acceptance does not cover the Minecraft {} server JAR with SHA-1 {}; accept it again for this exact file",
            server.version, server.sha1
        )));
    }
    Ok(())
}

fn manifest_is_valid(manifest: &RunManifest) -> bool {
    manifest.schema_version == RUN_MANIFEST_SCHEMA_VERSION
        && !manifest.minecraft_version.is_empty()
        && is_sha1(&manifest.server_jar_sha1)
        && manifest.eula_accepted_at > 0
        && manifest.consent_at > 0
}

pub(crate) fn prepare(
    runs_root: &Path,
    runtime: GenerationRuntime<'_>,
    request: &GenerationRequest,
    approval: RunApproval,
    datapacks: &[GenerationDatapack],
) -> Result<GenerationPlan, ServerError> {
    validate_request(request)?;
    validate_approval(request, &approval, runtime.server)?;
    fs::create_dir_all(runs_root).map_err(|error| {
        ServerError::Io(format!(
            "cannot create the generation runs directory {}: {error}",
            runs_root.display()
        ))
    })?;
    let created_at = now_millis();
    let run_id = format!("run-{created_at}-{}", std::process::id());
    let run_dir = runs_root.join(&run_id);
    if run_dir.exists() {
        return Err(ServerError::Invalid(format!(
            "{} already exists",
            run_dir.display()
        )));
    }
    fs::create_dir_all(run_dir.join("logs")).map_err(|error| {
        ServerError::Io(format!(
            "cannot create the isolated run directory {}: {error}",
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
    let seed = request.seed.trim().to_owned();
    let plan = GenerationPlan {
        run_id: run_id.clone(),
        world_dir: run_dir.join("world"),
        log_path: run_dir.join("logs").join("furybuilder-console.log"),
        run_dir: run_dir.clone(),
        java_exe: runtime.java_exe.to_path_buf(),
        java_version: runtime.java_version.to_owned(),
        server_jar: runtime.server_jar.to_path_buf(),
        server_sha1: runtime.server.sha1.clone(),
        seed,
        preset: request.preset,
        center_x: request.center_x,
        center_z: request.center_z,
        radius_chunks: request.radius_chunks,
        time_limit_seconds: request.time_limit_seconds,
        memory_mb: request.memory_mb,
        view_distance: request.view_distance,
        cpu_threads: request.cpu_threads,
        generate_structures: request.generate_structures,
        eula_accepted_at: approval.eula.accepted_at,
        consent_at: approval.consent_at,
        datapacks: datapacks.to_vec(),
        port,
        expected_chunks: expected_chunks(request),
        idle_timeout: DEFAULT_IDLE_TIMEOUT,
        min_generation: DEFAULT_MIN_GENERATION,
        save_interval: DEFAULT_SAVE_INTERVAL,
    };
    let manifest = RunManifest {
        schema_version: RUN_MANIFEST_SCHEMA_VERSION,
        run_id,
        created_at,
        seed: plan.seed.clone(),
        preset: plan.preset,
        center_x: plan.center_x,
        center_z: plan.center_z,
        radius_chunks: plan.radius_chunks,
        generate_structures: plan.generate_structures,
        datapacks: plan.datapacks.clone(),
        minecraft_version: runtime.server.version.clone(),
        server_jar_sha1: runtime.server.sha1.clone(),
        eula_accepted_at: approval.eula.accepted_at,
        consent_at: approval.consent_at,
    };
    let result = (|| -> Result<(), ServerError> {
        write_json(&run_dir.join(RUN_MANIFEST_NAME), &manifest)?;
        fs::write(
            run_dir.join("server.properties"),
            render_properties(
                &plan.seed,
                plan.preset,
                plan.generate_structures,
                plan.view_distance,
                plan.port,
            ),
        )
        .map_err(|error| ServerError::Io(format!("cannot write server.properties: {error}")))?;
        datapack::install(&plan.world_dir, datapacks)?;
        // Only written inside this run directory, only after an explicit acceptance for
        // this exact server JAR. The acceptance itself lives in server.json.
        fs::write(run_dir.join("eula.txt"), b"eula=true\n")
            .map_err(|error| ServerError::Io(format!("cannot write eula.txt: {error}")))?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&run_dir);
        return Err(error);
    }
    Ok(plan)
}

pub(crate) fn initial_snapshot(plan: &GenerationPlan) -> GenerationSnapshot {
    GenerationSnapshot {
        run_id: plan.run_id.clone(),
        running: true,
        phase: GenerationPhase::Preparing,
        elapsed_ms: 0,
        expected_chunks: plan.expected_chunks,
        generated_chunks: 0,
        region_files: 0,
        world_bytes: 0,
        message: "Prepared an isolated run directory".to_owned(),
        error: None,
    }
}

pub(crate) fn execute(
    plan: GenerationPlan,
    cancel: Arc<AtomicBool>,
    snapshot: Arc<Mutex<GenerationSnapshot>>,
) -> GenerationReport {
    let wall_started = Instant::now();
    let started_at = now_millis();
    let console = Console::new(&plan.log_path);
    let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        supervise(&plan, &cancel, &snapshot, console.clone())
    })) {
        Ok(outcome) => outcome,
        Err(_) => Outcome {
            phase: GenerationPhase::Failed,
            stop_reason: GenerationStopReason::Crashed,
            world_complete: false,
            exit_code: None,
            server_pid: None,
            process_tree_guarded: false,
            bind_address: None,
            message: "The generation supervisor stopped unexpectedly".to_owned(),
            error: Some(
                "internal generation supervisor panic; the process tree was terminated".to_owned(),
            ),
            stats: count_progress(&plan),
        },
    };
    let elapsed_ms = wall_started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let final_snapshot = GenerationSnapshot {
        run_id: plan.run_id.clone(),
        running: false,
        phase: outcome.phase,
        elapsed_ms,
        expected_chunks: plan.expected_chunks,
        generated_chunks: outcome.stats.chunks,
        region_files: outcome.stats.region_files,
        world_bytes: outcome.stats.world_bytes,
        message: outcome.message,
        error: outcome.error,
    };
    if let Ok(mut shared) = snapshot.lock() {
        *shared = final_snapshot.clone();
    }
    let report = GenerationReport {
        snapshot: final_snapshot,
        stop_reason: outcome.stop_reason,
        world_complete: outcome.world_complete,
        exit_code: outcome.exit_code,
        server_pid: outcome.server_pid,
        seed: plan.seed,
        preset: plan.preset,
        center_x: plan.center_x,
        center_z: plan.center_z,
        radius_chunks: plan.radius_chunks,
        time_limit_seconds: plan.time_limit_seconds,
        memory_mb: plan.memory_mb,
        view_distance: plan.view_distance,
        cpu_threads: plan.cpu_threads,
        generate_structures: plan.generate_structures,
        java_path: jvm_path(&plan.java_exe).display().to_string(),
        java_version: plan.java_version,
        server_jar: jvm_path(&plan.server_jar).display().to_string(),
        server_sha1: plan.server_sha1,
        eula_accepted_at: plan.eula_accepted_at,
        consent_at: plan.consent_at,
        datapacks: plan.datapacks,
        bind_address: outcome.bind_address,
        process_tree_guarded: outcome.process_tree_guarded,
        run_dir: plan.run_dir.display().to_string(),
        world_dir: plan.world_dir.display().to_string(),
        log_path: plan.log_path.display().to_string(),
        started_at,
        finished_at: now_millis(),
        log_tail: console.tail(),
    };
    let _ = write_json(&plan.run_dir.join(RUN_REPORT_NAME), &report);
    report
}

fn supervise(
    plan: &GenerationPlan,
    cancel: &AtomicBool,
    snapshot: &Mutex<GenerationSnapshot>,
    console: Console,
) -> Outcome {
    let start = Instant::now();
    let mut stats = count_progress(plan);
    publish(
        snapshot,
        Some(GenerationPhase::Preparing),
        Some("Starting the isolated official server"),
        start.elapsed(),
        stats,
    );
    let mut command = build_command(plan);
    let mut guard = match GuardedChild::spawn(&mut command, plan.memory_mb) {
        Ok(guard) => guard,
        Err(error) => {
            return failure_outcome(
                GenerationPhase::Failed,
                GenerationStopReason::StartupFailed,
                format!("The isolated server could not be started: {error}"),
                Some(error.to_string()),
                stats,
                None,
                None,
                false,
                None,
            );
        }
    };
    let pid = guard.pid();
    let guarded = guard.guarded();
    let stdout = guard.child.stdout.take();
    let stderr = guard.child.stderr.take();
    let (sender, receiver) = mpsc::channel::<String>();
    if let Some(stdout) = stdout {
        spawn_reader(stdout, sender.clone(), console.clone());
    }
    if let Some(stderr) = stderr {
        spawn_reader(stderr, sender.clone(), console);
    }
    drop(sender);

    let mut live = Live {
        started: false,
        saved: false,
        bind_address: None,
        forceload_sent: false,
        isolation_checked: false,
        last_chunks: 0,
        last_progress: start,
        last_save: start,
        last_error: None,
        spawn_percent: None,
    };
    let mut decision: Option<Decision> = None;
    let mut next_progress = start;
    let mut last_publish = start;

    while decision.is_none() {
        let mut received = false;
        for _ in 0..256 {
            match receiver.try_recv() {
                Ok(line) => {
                    absorb(&line, &mut live);
                    received = true;
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if !received {
            if let Ok(line) = receiver.recv_timeout(TICK) {
                absorb(&line, &mut live);
            }
        }
        let now = Instant::now();
        let elapsed = now.duration_since(start);

        if cancel.load(Ordering::SeqCst) {
            decision = Some(Decision::Cancel);
            break;
        }
        if elapsed >= Duration::from_secs(plan.time_limit_seconds as u64) {
            decision = Some(Decision::TimeLimit);
            break;
        }
        if let Some(code) = guard.poll() {
            decision = Some(Decision::Crash(code));
            break;
        }
        if !live.started && now.duration_since(start) >= STARTUP_TIMEOUT {
            decision = Some(Decision::StartupTimeout);
            break;
        }
        if live.started && !live.isolation_checked {
            live.isolation_checked = true;
            if let Some(address) = live.bind_address.as_deref() {
                if !is_loopback_bind(address) {
                    decision = Some(Decision::IsolationViolation);
                    break;
                }
            }
        }
        if live.started && !live.forceload_sent {
            for command in forceload_commands(plan.center_x, plan.center_z, plan.radius_chunks) {
                let _ = guard.send(&command);
            }
            live.forceload_sent = true;
            live.last_progress = now;
            live.last_save = now + Duration::from_secs(2);
            publish(
                snapshot,
                Some(GenerationPhase::Generating),
                Some("Generating the requested chunks with the official server"),
                elapsed,
                stats,
            );
        }
        if live.started {
            if now >= next_progress {
                stats = count_progress(plan);
                if stats.chunks > live.last_chunks {
                    live.last_chunks = stats.chunks;
                    live.last_progress = now;
                }
                let message = if stats.chunks >= plan.expected_chunks {
                    "The requested chunks are present; finishing the isolated server"
                } else {
                    "Generating chunks; periodic saves keep progress observable"
                };
                publish(snapshot, None, Some(message), elapsed, stats);
                next_progress = now + Duration::from_secs(1);
            } else if now.duration_since(last_publish) >= Duration::from_millis(500) {
                publish(snapshot, None, None, elapsed, stats);
                last_publish = now;
            }
            if now >= live.last_save {
                let _ = guard.send("save-all flush");
                live.last_save = now + plan.save_interval;
            }
            if stats.chunks >= plan.expected_chunks && elapsed >= plan.min_generation {
                decision = Some(Decision::Complete);
                break;
            }
            if elapsed >= plan.min_generation
                && now.duration_since(live.last_progress) >= plan.idle_timeout
            {
                decision = Some(Decision::Idle);
                break;
            }
        }
    }

    let decision = decision.unwrap_or(Decision::Crash(None));
    match decision {
        Decision::Crash(code) => {
            guard.terminate();
            stats = count_progress(plan);
            let reason = if live.started {
                GenerationStopReason::Crashed
            } else {
                GenerationStopReason::StartupFailed
            };
            let error = live.last_error.clone().unwrap_or_else(|| {
                format!(
                    "the official server exited unexpectedly with {}",
                    code.map(|code| format!("code {code}"))
                        .unwrap_or_else(|| "no exit code".to_owned())
                )
            });
            failure_outcome(
                GenerationPhase::Failed,
                reason,
                "The isolated server stopped unexpectedly".to_owned(),
                Some(error),
                stats,
                code,
                live.bind_address,
                guarded,
                Some(pid),
            )
        }
        Decision::StartupTimeout => {
            guard.terminate();
            stats = count_progress(plan);
            failure_outcome(
                GenerationPhase::Failed,
                GenerationStopReason::StartupFailed,
                "The isolated server did not become ready in time".to_owned(),
                Some(format!(
                    "the server did not print its ready line within {} seconds",
                    STARTUP_TIMEOUT.as_secs()
                )),
                stats,
                None,
                live.bind_address,
                guarded,
                Some(pid),
            )
        }
        Decision::IsolationViolation => {
            let _ = stop_server(
                &mut guard,
                &receiver,
                &mut live,
                plan,
                snapshot,
                start,
                &mut stats,
                STOP_TIMEOUT,
                false,
            );
            failure_outcome(
                GenerationPhase::Failed,
                GenerationStopReason::IsolationFailed,
                "The isolated server tried to listen outside loopback".to_owned(),
                Some(format!(
                    "refusing a non-loopback bind address: {}",
                    live.bind_address.as_deref().unwrap_or("unknown")
                )),
                stats,
                None,
                live.bind_address,
                guarded,
                Some(pid),
            )
        }
        Decision::Cancel => {
            let was_started = live.started;
            let exit = stop_server(
                &mut guard,
                &receiver,
                &mut live,
                plan,
                snapshot,
                start,
                &mut stats,
                CANCEL_TIMEOUT,
                was_started,
            );
            let world_complete = stats.chunks >= plan.expected_chunks;
            Outcome {
                phase: GenerationPhase::Cancelled,
                stop_reason: GenerationStopReason::Cancelled,
                world_complete,
                exit_code: exit,
                server_pid: Some(pid),
                process_tree_guarded: guarded,
                bind_address: live.bind_address,
                message: "Generation cancelled; the isolated process tree was terminated"
                    .to_owned(),
                error: None,
                stats,
            }
        }
        Decision::Complete | Decision::Idle | Decision::TimeLimit => {
            let was_started = live.started;
            let exit = stop_server(
                &mut guard,
                &receiver,
                &mut live,
                plan,
                snapshot,
                start,
                &mut stats,
                STOP_TIMEOUT,
                was_started,
            );
            let world_complete = stats.chunks >= plan.expected_chunks;
            let (stop_reason, message, error) = match decision {
                Decision::Complete => (
                    GenerationStopReason::Completed,
                    "Isolated generation completed".to_owned(),
                    None,
                ),
                Decision::Idle if world_complete => (
                    GenerationStopReason::Completed,
                    "Isolated generation completed".to_owned(),
                    None,
                ),
                Decision::Idle => (
                    GenerationStopReason::Incomplete,
                    "Generation stopped after no new chunks were written".to_owned(),
                    Some(
                        "the requested area is incomplete; no new chunks appeared before the idle limit"
                            .to_owned(),
                    ),
                ),
                Decision::TimeLimit => (
                    GenerationStopReason::TimeLimit,
                    if world_complete {
                        "Time limit reached; the requested area is present".to_owned()
                    } else {
                        "Time limit reached; the requested area is incomplete".to_owned()
                    },
                    if world_complete {
                        None
                    } else {
                        Some("the configured time limit was reached before the full area was generated".to_owned())
                    },
                ),
                _ => unreachable!(),
            };
            Outcome {
                phase: GenerationPhase::Completed,
                stop_reason,
                world_complete,
                exit_code: exit,
                server_pid: Some(pid),
                process_tree_guarded: guarded,
                bind_address: live.bind_address,
                message,
                error,
                stats,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stop_server(
    guard: &mut GuardedChild,
    receiver: &Receiver<String>,
    live: &mut Live,
    plan: &GenerationPlan,
    snapshot: &Mutex<GenerationSnapshot>,
    start: Instant,
    stats: &mut RegionStats,
    grace: Duration,
    save: bool,
) -> Option<i32> {
    if save {
        publish(
            snapshot,
            Some(GenerationPhase::Saving),
            Some("Saving generated chunks and releasing forceloads"),
            start.elapsed(),
            *stats,
        );
        let _ = guard.send("forceload remove all");
        let _ = guard.send("save-all flush");
        live.saved = false;
        let deadline = Instant::now() + SAVE_TIMEOUT.min(grace);
        while Instant::now() < deadline && !live.saved {
            if let Some(code) = guard.poll() {
                *stats = count_progress(plan);
                return code;
            }
            drain_lines(receiver, live, Duration::from_millis(150));
        }
        *stats = count_progress(plan);
    }
    publish(
        snapshot,
        Some(GenerationPhase::Stopping),
        Some("Stopping the isolated server"),
        start.elapsed(),
        *stats,
    );
    let _ = guard.send("stop");
    let deadline = Instant::now() + grace;
    let mut exit: Option<Option<i32>> = None;
    while Instant::now() < deadline {
        if let Some(code) = guard.poll() {
            exit = Some(code);
            break;
        }
        drain_lines(receiver, live, Duration::from_millis(100));
    }
    if exit.is_none() {
        guard.terminate();
        exit = Some(None);
    }
    *stats = count_progress(plan);
    exit.flatten()
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

#[allow(clippy::too_many_arguments)]
fn failure_outcome(
    phase: GenerationPhase,
    stop_reason: GenerationStopReason,
    message: String,
    error: Option<String>,
    stats: RegionStats,
    exit_code: Option<i32>,
    bind_address: Option<String>,
    process_tree_guarded: bool,
    server_pid: Option<u32>,
) -> Outcome {
    Outcome {
        phase,
        stop_reason,
        world_complete: false,
        exit_code,
        server_pid,
        process_tree_guarded,
        bind_address,
        message,
        error,
        stats,
    }
}

/// Java on Windows does not reliably open `\\?\` verbatim paths. furybuilder keeps them for
/// safe Rust filesystem access, but the JVM gets a normal drive/UNC path.
fn jvm_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.display().to_string();
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }
    path.to_path_buf()
}

fn build_command(plan: &GenerationPlan) -> Command {
    let mut command = Command::new(jvm_path(&plan.java_exe));
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

fn spawn_reader<R: Read + Send + 'static>(pipe: R, sender: mpsc::Sender<String>, console: Console) {
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
    });
}

fn absorb(line: &str, live: &mut Live) {
    let clean = strip_ansi(line);
    if let Some(rest) = clean.split("Starting Minecraft server on ").nth(1) {
        if let Some(address) = rest.split_whitespace().next() {
            live.bind_address = Some(address.trim_end_matches(',').to_owned());
        }
    }
    if clean.contains("Done (") && clean.contains("For help") {
        live.started = true;
    }
    if clean.contains("Saved the game") {
        live.saved = true;
    }
    if let Some(rest) = clean.split("Preparing spawn area: ").nth(1) {
        if let Some(percent) = rest.split('%').next() {
            live.spawn_percent = percent.trim().parse().ok();
        }
    }
    if live.last_error.is_none()
        && (clean.contains("ERROR]") || clean.contains("Exception") || clean.contains("Caused by"))
    {
        live.last_error = Some(clean);
    }
}

fn publish(
    snapshot: &Mutex<GenerationSnapshot>,
    phase: Option<GenerationPhase>,
    message: Option<&str>,
    elapsed: Duration,
    stats: RegionStats,
) {
    if let Ok(mut state) = snapshot.lock() {
        if let Some(phase) = phase {
            state.phase = phase;
        }
        if let Some(message) = message {
            state.message = message.to_owned();
        }
        state.elapsed_ms = elapsed.as_millis().min(u64::MAX as u128) as u64;
        state.generated_chunks = stats.chunks;
        state.region_files = stats.region_files;
        state.world_bytes = stats.world_bytes;
    }
}

fn is_loopback_bind(address: &str) -> bool {
    address.starts_with("127.") || address == "[::1]" || address == "0:0:0:0:0:0:0:1"
}

fn strip_ansi(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            if characters.next() == Some('[') {
                for next in characters.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        output.push(character);
    }
    output
}

fn render_properties(
    seed: &str,
    preset: WorldPreset,
    generate_structures: bool,
    view_distance: u8,
    port: u16,
) -> String {
    format!(
        "# Generated by FuryBuilder for one isolated Vanilla World generation run.\n\
         # External listeners are disabled: loopback only, no RCON/query/status.\n\
         server-ip=127.0.0.1\n\
         server-port={port}\n\
         online-mode=false\n\
         enable-rcon=false\n\
         enable-query=false\n\
         enable-status=false\n\
         max-players=1\n\
         motd=FuryBuilder isolated worldgen\n\
         level-name=world\n\
         level-seed={seed}\n\
         level-type={}\n\
         generator-settings=\n\
         generate-structures={generate_structures}\n\
         view-distance={view_distance}\n\
         simulation-distance={view_distance}\n\
         spawn-protection=0\n\
         sync-chunk-writes=true\n\
         max-tick-time=-1\n",
        preset.level_type()
    )
}

fn forceload_commands(center_x: i32, center_z: i32, radius: u32) -> Vec<String> {
    let center_chunk_x = center_x.div_euclid(16);
    let center_chunk_z = center_z.div_euclid(16);
    let radius = radius as i32;
    let min_x = center_chunk_x - radius;
    let max_x = center_chunk_x + radius;
    let min_z = center_chunk_z - radius;
    let max_z = center_chunk_z + radius;
    let mut commands = Vec::new();
    let mut z = min_z;
    while z <= max_z {
        let z_end = (z + 15).min(max_z);
        let mut x = min_x;
        while x <= max_x {
            let x_end = (x + 15).min(max_x);
            commands.push(format!(
                "forceload add {} {} {} {}",
                x * 16,
                z * 16,
                x_end * 16 + 15,
                z_end * 16 + 15
            ));
            x = x_end + 1;
        }
        z = z_end + 1;
    }
    commands
}

fn count_progress(plan: &GenerationPlan) -> RegionStats {
    count_region_chunks(
        &plan.world_dir,
        plan.center_x,
        plan.center_z,
        plan.radius_chunks,
    )
}

fn count_region_chunks(
    world_dir: &Path,
    center_x: i32,
    center_z: i32,
    radius_chunks: u32,
) -> RegionStats {
    let mut stats = RegionStats::default();
    let region_dir = world_dir.join("region");
    let Ok(entries) = fs::read_dir(&region_dir) else {
        return stats;
    };
    let center_chunk_x = center_x.div_euclid(16);
    let center_chunk_z = center_z.div_euclid(16);
    let radius = radius_chunks as i32;
    let min_chunk_x = center_chunk_x - radius;
    let max_chunk_x = center_chunk_x + radius;
    let min_chunk_z = center_chunk_z - radius;
    let max_chunk_z = center_chunk_z + radius;
    for (index, entry) in entries.flatten().enumerate() {
        if index >= MAX_REGION_FILES {
            break;
        }
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((region_x, region_z)) = parse_region_name(name) else {
            continue;
        };
        stats.region_files += 1;
        stats.world_bytes = stats.world_bytes.saturating_add(metadata.len());
        if let Ok(count) = count_region_header(
            &path,
            region_x,
            region_z,
            min_chunk_x,
            max_chunk_x,
            min_chunk_z,
            max_chunk_z,
        ) {
            stats.chunks = stats.chunks.saturating_add(count);
        }
    }
    stats
}

fn parse_region_name(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

fn count_region_header(
    path: &Path,
    region_x: i32,
    region_z: i32,
    min_chunk_x: i32,
    max_chunk_x: i32,
    min_chunk_z: i32,
    max_chunk_z: i32,
) -> std::io::Result<u64> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 4096];
    file.read_exact(&mut header)?;
    let mut count = 0u64;
    for index in 0..1024usize {
        let offset = index * 4;
        // The first three bytes are the sector offset; zero means the chunk is absent.
        if header[offset] == 0 && header[offset + 1] == 0 && header[offset + 2] == 0 {
            continue;
        }
        let local_x = (index % 32) as i32;
        let local_z = (index / 32) as i32;
        let chunk_x = region_x * 32 + local_x;
        let chunk_z = region_z * 32 + local_z;
        if (min_chunk_x..=max_chunk_x).contains(&chunk_x)
            && (min_chunk_z..=max_chunk_z).contains(&chunk_z)
        {
            count += 1;
        }
    }
    Ok(count)
}

fn pick_loopback_port() -> Result<u16, ServerError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        ServerError::Io(format!(
            "cannot reserve an ephemeral loopback port for the isolated server: {error}"
        ))
    })?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| {
            ServerError::Io(format!("cannot read the reserved loopback port: {error}"))
        })
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), ServerError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        ServerError::Io(format!("cannot serialize {}: {error}", path.display()))
    })?;
    fs::write(path, bytes)
        .map_err(|error| ServerError::Io(format!("cannot write {}: {error}", path.display())))
}

pub(crate) fn load_last_report(runs_root: &Path) -> Option<GenerationReport> {
    let mut best: Option<GenerationReport> = None;
    let entries = fs::read_dir(runs_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join(RUN_REPORT_NAME);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let Ok(report) = serde_json::from_slice::<GenerationReport>(&bytes) else {
            continue;
        };
        if report.snapshot.running {
            continue;
        }
        if best
            .as_ref()
            .map(|current| report.finished_at >= current.finished_at)
            .unwrap_or(true)
        {
            best = Some(report);
        }
    }
    best
}

fn valid_run_id(run_id: &str) -> bool {
    let mut parts = run_id.split('-');
    let (Some("run"), Some(millis), Some(pid), None) =
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

fn read_report_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ServerError> {
    let bytes = fs::read(path).map_err(|error| {
        ServerError::Invalid(format!(
            "the generation run is missing {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        ServerError::Invalid(format!(
            "{} is not a valid generation record: {error}",
            path.display()
        ))
    })
}

/// Opens a generation run only when it is manifest-owned, terminal, completed and its world
/// directory is still inside that exact run. The returned world is read-only input for the
/// project import pipeline; this function never writes to it.
pub(crate) fn load_completed_world(
    runs_root: &Path,
    run_id: &str,
) -> Result<GeneratedWorld, ServerError> {
    if !valid_run_id(run_id) {
        return Err(ServerError::Invalid(
            "the generation run id is not a valid FuryBuilder run id".to_owned(),
        ));
    }
    let root = fs::canonicalize(runs_root).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the generation runs root {}: {error}",
            runs_root.display()
        ))
    })?;
    let run_dir = root.join(run_id);
    let metadata = fs::symlink_metadata(&run_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "the generation run {run_id} is not available: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ServerError::Invalid(format!(
            "the generation run {run_id} is not a regular directory"
        )));
    }
    let run_dir = fs::canonicalize(&run_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the generation run {run_id}: {error}"
        ))
    })?;
    if run_dir.parent() != Some(root.as_path()) {
        return Err(ServerError::Invalid(format!(
            "the generation run {run_id} escapes the runs root"
        )));
    }
    let manifest: RunManifest = read_report_json(&run_dir.join(RUN_MANIFEST_NAME))?;
    if !manifest_is_valid(&manifest) || manifest.run_id != run_id {
        return Err(ServerError::Invalid(format!(
            "the generation manifest for {run_id} does not match this run"
        )));
    }
    let report: GenerationReport = read_report_json(&run_dir.join(RUN_REPORT_NAME))?;
    if report.snapshot.run_id != run_id {
        return Err(ServerError::Invalid(format!(
            "the generation report for {run_id} belongs to a different run"
        )));
    }
    if report.stop_reason != GenerationStopReason::Completed
        || !report.world_complete
        || report.snapshot.running
    {
        return Err(ServerError::Invalid(format!(
            "the generation run {run_id} did not complete; only a completed world can be imported"
        )));
    }
    if report.snapshot.expected_chunks == 0
        || report.snapshot.generated_chunks < report.snapshot.expected_chunks
    {
        return Err(ServerError::Invalid(format!(
            "the generation report for {run_id} does not prove the requested chunks were written"
        )));
    }
    if report.seed != manifest.seed
        || report.preset != manifest.preset
        || report.center_x != manifest.center_x
        || report.center_z != manifest.center_z
        || report.radius_chunks != manifest.radius_chunks
        || report.generate_structures != manifest.generate_structures
        || report.datapacks != manifest.datapacks
        || report.server_sha1 != manifest.server_jar_sha1
        || report.eula_accepted_at != manifest.eula_accepted_at
        || report.consent_at != manifest.consent_at
    {
        return Err(ServerError::Invalid(format!(
            "the generation report for {run_id} does not match its manifest"
        )));
    }
    let world_dir = run_dir.join("world");
    let metadata = fs::symlink_metadata(&world_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "the generated world for {run_id} is missing: {error}"
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ServerError::Invalid(format!(
            "the generated world for {run_id} is not a regular directory"
        )));
    }
    let world_dir = fs::canonicalize(&world_dir).map_err(|error| {
        ServerError::Invalid(format!(
            "cannot resolve the generated world for {run_id}: {error}"
        ))
    })?;
    if world_dir.parent() != Some(run_dir.as_path()) {
        return Err(ServerError::Invalid(format!(
            "the generated world for {run_id} is outside its run directory"
        )));
    }
    let reported_world = fs::canonicalize(Path::new(&report.world_dir)).map_err(|error| {
        ServerError::Invalid(format!(
            "the generation report points to an unavailable world: {error}"
        ))
    })?;
    if reported_world != world_dir {
        return Err(ServerError::Invalid(format!(
            "the generation report for {run_id} points to a different world directory"
        )));
    }
    Ok(GeneratedWorld {
        run_id: run_id.to_owned(),
        world_dir: world_dir.display().to_string(),
        seed: manifest.seed,
        preset: manifest.preset,
        center_x: manifest.center_x,
        center_z: manifest.center_z,
        radius_chunks: manifest.radius_chunks,
        generate_structures: manifest.generate_structures,
        datapacks: manifest.datapacks,
        java_version: report.java_version,
        server_sha1: report.server_sha1,
        chunks: report.snapshot.generated_chunks,
        region_files: report.snapshot.region_files,
        world_bytes: report.snapshot.world_bytes,
        finished_at: report.finished_at,
    })
}

pub(crate) fn scan_runs(runs_root: &Path) -> GenerationRunsReport {
    let mut runs = Vec::new();
    let mut total_bytes = 0u64;
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(runs_root) else {
        return GenerationRunsReport {
            root: runs_root.display().to_string(),
            runs,
            total_bytes,
            problems,
        };
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        let manifest_path = path.join(RUN_MANIFEST_NAME);
        if !manifest_path.is_file() {
            problems.push(format!(
                "{} is not a FuryBuilder generation run",
                path.display()
            ));
            continue;
        }
        let manifest = match fs::read(&manifest_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<RunManifest>(&bytes).ok())
        {
            Some(manifest) if manifest_is_valid(&manifest) => manifest,
            _ => {
                problems.push(format!("{} has an unreadable run manifest", path.display()));
                continue;
            }
        };
        let report = fs::read(path.join(RUN_REPORT_NAME))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<GenerationReport>(&bytes).ok());
        let bytes = directory_size(&path);
        total_bytes = total_bytes.saturating_add(bytes);
        runs.push(GenerationRunInfo {
            run_id: manifest.run_id,
            path: path.display().to_string(),
            status: report
                .as_ref()
                .map(|report| stop_reason_label(report.stop_reason).to_owned())
                .unwrap_or_else(|| "incomplete".to_owned()),
            created_at: manifest.created_at,
            finished_at: report.as_ref().map(|report| report.finished_at),
            chunks: report
                .as_ref()
                .map(|report| report.snapshot.generated_chunks)
                .unwrap_or(0),
            expected_chunks: manifest_chunk_count(manifest.radius_chunks),
            region_files: report
                .as_ref()
                .map(|report| report.snapshot.region_files)
                .unwrap_or(0),
            world_bytes: report
                .as_ref()
                .map(|report| report.snapshot.world_bytes)
                .unwrap_or(0),
            world_complete: report.as_ref().map(|report| report.world_complete),
            stop_reason: report.as_ref().map(|report| report.stop_reason),
            error: report
                .as_ref()
                .and_then(|report| report.snapshot.error.clone()),
        });
    }
    runs.sort_by_key(|run| std::cmp::Reverse(run.created_at));
    GenerationRunsReport {
        root: runs_root.display().to_string(),
        runs,
        total_bytes,
        problems,
    }
}

pub(crate) fn clear_runs(runs_root: &Path) -> Result<GenerationRunsReport, ServerError> {
    if runs_root.exists() {
        let entries = fs::read_dir(runs_root)
            .map_err(|error| ServerError::Io(format!("{}: {error}", runs_root.display())))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            if path.join(RUN_MANIFEST_NAME).is_file() {
                fs::remove_dir_all(&path).map_err(|error| {
                    ServerError::Io(format!("cannot remove {}: {error}", path.display()))
                })?;
            }
        }
    }
    Ok(scan_runs(runs_root))
}

fn manifest_chunk_count(radius_chunks: u32) -> u64 {
    let side = radius_chunks.saturating_mul(2).saturating_add(1) as u64;
    side.saturating_mul(side)
}

fn stop_reason_label(reason: GenerationStopReason) -> &'static str {
    match reason {
        GenerationStopReason::Completed => "completed",
        GenerationStopReason::Incomplete => "incomplete",
        GenerationStopReason::TimeLimit => "time_limit",
        GenerationStopReason::Cancelled => "cancelled",
        GenerationStopReason::Crashed => "crashed",
        GenerationStopReason::StartupFailed => "startup_failed",
        GenerationStopReason::IsolationFailed => "isolation_failed",
    }
}

fn directory_size(root: &Path) -> u64 {
    fn walk(path: &Path, bytes: &mut u64, entries: &mut usize) {
        if *entries >= MAX_SCANNED_ENTRIES {
            return;
        }
        let Ok(read_dir) = fs::read_dir(path) else {
            return;
        };
        for entry in read_dir.flatten() {
            if *entries >= MAX_SCANNED_ENTRIES {
                return;
            }
            *entries += 1;
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                walk(&path, bytes, entries);
            } else if metadata.is_file() {
                *bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    let mut bytes = 0u64;
    let mut entries = 0usize;
    walk(root, &mut bytes, &mut entries);
    bytes
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::types::EULA_URL;
    use std::sync::atomic::AtomicU64;

    static FAKE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn request(seed: &str) -> GenerationRequest {
        GenerationRequest {
            seed: seed.to_owned(),
            preset: WorldPreset::Normal,
            center_x: 0,
            center_z: 0,
            radius_chunks: 1,
            time_limit_seconds: 120,
            memory_mb: 1024,
            view_distance: 2,
            cpu_threads: 1,
            generate_structures: true,
            consent: true,
            datapacks: Vec::new(),
        }
    }

    #[test]
    fn validates_limits_and_rejects_seed_injection() {
        assert!(validate_request(&request("FuryBuilder")).is_ok());
        assert!(validate_request(&request("12345")).is_ok());
        assert!(validate_request(&request("bad\nseed")).is_err());
        assert!(validate_request(&request("with space")).is_err());
        assert!(validate_request(&request("")).is_err());
        assert!(validate_request(&request(&"a".repeat(65))).is_err());

        let mut no_consent = request("seed");
        no_consent.consent = false;
        assert!(matches!(
            validate_request(&no_consent),
            Err(ServerError::ConsentRequired(_))
        ));

        let mut too_many_packs = request("seed");
        too_many_packs.datapacks = (0..=datapack::MAX_DATAPACKS)
            .map(|index| super::super::types::DatapackSelection {
                path: format!("pack-{index}.zip"),
            })
            .collect();
        assert!(validate_request(&too_many_packs).is_err());

        let mut too_large = request("seed");
        too_large.radius_chunks = 17;
        assert!(validate_request(&too_large).is_err());

        let mut too_far = request("seed");
        too_far.center_x = 30_000_000;
        assert!(validate_request(&too_far).is_err());

        let mut no_memory = request("seed");
        no_memory.memory_mb = 128;
        assert!(validate_request(&no_memory).is_err());

        let mut bad_view = request("seed");
        bad_view.view_distance = 32;
        assert!(validate_request(&bad_view).is_err());
    }

    #[test]
    fn approval_requires_the_exact_jar_version_and_run_consent() {
        let server = ServerJarInfo {
            version: "1.21.4".to_owned(),
            data_version: 4189,
            java_major: Some(21),
            protocol_version: Some(769),
            resource_pack_format: Some(46),
            sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            size: 56_880_250,
        };
        let eula = SavedEula {
            accepted_at: 1_763_500_000,
            minecraft_version: "1.21.4".to_owned(),
            jar_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            eula_url: EULA_URL.to_owned(),
        };
        let approval = RunApproval {
            eula: &eula,
            consent_at: 2,
        };
        assert!(validate_approval(&request("seed"), &approval, &server).is_ok());

        let mut wrong_version = eula.clone();
        wrong_version.minecraft_version = "1.20.6".to_owned();
        assert!(matches!(
            validate_approval(
                &request("seed"),
                &RunApproval {
                    eula: &wrong_version,
                    consent_at: 2,
                },
                &server
            ),
            Err(ServerError::EulaRequired(_))
        ));

        let mut wrong_jar = eula.clone();
        wrong_jar.jar_sha1 = "0000000000000000000000000000000000000000".to_owned();
        assert!(matches!(
            validate_approval(
                &request("seed"),
                &RunApproval {
                    eula: &wrong_jar,
                    consent_at: 2,
                },
                &server
            ),
            Err(ServerError::EulaRequired(_))
        ));

        let mut wrong_url = eula.clone();
        wrong_url.eula_url = "https://example.com/not-the-eula".to_owned();
        assert!(matches!(
            validate_approval(
                &request("seed"),
                &RunApproval {
                    eula: &wrong_url,
                    consent_at: 2,
                },
                &server
            ),
            Err(ServerError::EulaRequired(_))
        ));

        let mut missing_timestamp = eula.clone();
        missing_timestamp.accepted_at = 0;
        assert!(matches!(
            validate_approval(
                &request("seed"),
                &RunApproval {
                    eula: &missing_timestamp,
                    consent_at: 2,
                },
                &server
            ),
            Err(ServerError::EulaRequired(_))
        ));

        let mut no_consent = request("seed");
        no_consent.consent = false;
        assert!(matches!(
            validate_approval(&no_consent, &approval, &server),
            Err(ServerError::ConsentRequired(_))
        ));
        assert!(matches!(
            validate_approval(
                &request("seed"),
                &RunApproval {
                    eula: &eula,
                    consent_at: 0,
                },
                &server
            ),
            Err(ServerError::ConsentRequired(_))
        ));
    }

    #[test]
    fn run_manifest_requires_the_recorded_jar_and_approvals() {
        let manifest = RunManifest {
            schema_version: RUN_MANIFEST_SCHEMA_VERSION,
            run_id: "run-1".to_owned(),
            created_at: 1,
            seed: "seed".to_owned(),
            preset: WorldPreset::Normal,
            center_x: 0,
            center_z: 0,
            radius_chunks: 1,
            generate_structures: true,
            datapacks: Vec::new(),
            minecraft_version: "1.21.4".to_owned(),
            server_jar_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            eula_accepted_at: 1,
            consent_at: 2,
        };
        assert!(manifest_is_valid(&manifest));

        let mut no_eula = manifest.clone();
        no_eula.eula_accepted_at = 0;
        assert!(!manifest_is_valid(&no_eula));

        let mut no_consent = manifest.clone();
        no_consent.consent_at = 0;
        assert!(!manifest_is_valid(&no_consent));

        let mut no_jar = manifest.clone();
        no_jar.server_jar_sha1.clear();
        assert!(!manifest_is_valid(&no_jar));

        let mut old_schema = manifest;
        old_schema.schema_version = RUN_MANIFEST_SCHEMA_VERSION - 1;
        assert!(!manifest_is_valid(&old_schema));
    }

    #[test]
    fn properties_are_loopback_only_and_use_the_requested_level_type() {
        let properties = render_properties("FuryBuilder", WorldPreset::LargeBiomes, true, 4, 51234);
        assert!(properties.contains("server-ip=127.0.0.1\n"));
        assert!(properties.contains("server-port=51234\n"));
        assert!(properties.contains("level-seed=FuryBuilder\n"));
        assert!(properties.contains("level-type=minecraft:large_biomes\n"));
        assert!(properties.contains("generate-structures=true\n"));
        assert!(properties.contains("enable-rcon=false\n"));
        assert!(properties.contains("enable-query=false\n"));
        assert!(properties.contains("enable-status=false\n"));
        assert!(properties.contains("online-mode=false\n"));
        assert!(!properties.contains("server-ip=0.0.0.0"));
        let without = render_properties("seed", WorldPreset::Flat, false, 2, 1);
        assert!(without.contains("generate-structures=false\n"));
        assert!(without.contains("level-type=minecraft:flat\n"));
    }

    #[test]
    fn forceload_commands_cover_the_whole_area_with_bounded_tiles() {
        assert_eq!(
            forceload_commands(0, 0, 1),
            vec!["forceload add -16 -16 31 31".to_owned()]
        );
        let radius16 = forceload_commands(0, 0, 16);
        assert_eq!(radius16.len(), 9);
        assert_eq!(radius16[0], "forceload add -256 -256 -1 -1");
        assert_eq!(radius16[8], "forceload add 256 256 271 271");
        assert!(radius16
            .iter()
            .all(|command| command.starts_with("forceload add ")));
    }

    #[test]
    fn region_header_counting_filters_the_requested_area() {
        let dir =
            std::env::temp_dir().join(format!("furybuilder-region-count-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let region = dir.join("world").join("region");
        fs::create_dir_all(&region).expect("region dir");
        let write_region = |path: &Path, chunks: &[(i32, i32)]| {
            let mut header = vec![0u8; 8192];
            for (chunk_x, chunk_z) in chunks {
                let local_x = chunk_x.rem_euclid(32) as usize;
                let local_z = chunk_z.rem_euclid(32) as usize;
                let index = local_z * 32 + local_x;
                header[index * 4 + 2] = 1;
            }
            fs::write(path, &header).expect("region file");
        };
        // The area -1..1 spans four region files; chunk -1 belongs to region -1, not 0.
        for region_x in -1i32..=0 {
            for region_z in -1i32..=0 {
                let mut chunks = Vec::new();
                for chunk_z in -1i32..=1 {
                    for chunk_x in -1i32..=1 {
                        if chunk_x.div_euclid(32) == region_x && chunk_z.div_euclid(32) == region_z
                        {
                            chunks.push((chunk_x, chunk_z));
                        }
                    }
                }
                if region_x == 0 && region_z == 0 {
                    chunks.push((5, 5)); // Saved, but outside the requested radius.
                }
                write_region(
                    &region.join(format!("r.{region_x}.{region_z}.mca")),
                    &chunks,
                );
            }
        }
        fs::write(region.join("notes.txt"), b"ignored").expect("ignored file");
        let stats = count_region_chunks(&dir.join("world"), 0, 0, 1);
        assert_eq!(stats.chunks, 9);
        assert_eq!(stats.region_files, 4);
        assert_eq!(stats.world_bytes, 4 * 8192);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    fn test_plan(
        root: &Path,
        script: &Path,
        expected_chunks: u64,
        idle_timeout: Duration,
        min_generation: Duration,
    ) -> GenerationPlan {
        let run_dir = root.join("run");
        fs::create_dir_all(run_dir.join("logs")).expect("run dir");
        GenerationPlan {
            run_id: "run-test".to_owned(),
            world_dir: run_dir.join("world"),
            log_path: run_dir.join("logs").join("furybuilder-console.log"),
            run_dir,
            java_exe: script.to_path_buf(),
            java_version: "21-test".to_owned(),
            server_jar: root.join("server.jar"),
            server_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            seed: "FuryBuilder".to_owned(),
            preset: WorldPreset::Normal,
            center_x: 0,
            center_z: 0,
            radius_chunks: 1,
            time_limit_seconds: 60,
            memory_mb: 512,
            view_distance: 2,
            cpu_threads: 1,
            generate_structures: true,
            eula_accepted_at: 1,
            consent_at: 2,
            datapacks: Vec::new(),
            port: 25565,
            expected_chunks,
            idle_timeout,
            min_generation,
            save_interval: Duration::from_millis(200),
        }
    }

    #[cfg(windows)]
    fn compile_fake(root: &Path, name: &str, source: &str) -> PathBuf {
        let cs = root.join(format!("{name}.cs"));
        let exe = root.join(format!("{name}.exe"));
        let build = root.join(format!("{name}.ps1"));
        fs::write(&cs, source).expect("fake source");
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
            .expect("powershell for the fake server");
        assert!(status.success(), "fake server compilation failed");
        assert!(exe.is_file(), "fake server executable is missing");
        exe
    }

    #[cfg(windows)]
    fn run_fake(
        source: &str,
        expected_chunks: u64,
        idle_timeout: Duration,
        min_generation: Duration,
        cancel_after: Option<Duration>,
    ) -> (GenerationReport, Option<Duration>) {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-fake-server-{}-{}-{}",
            std::process::id(),
            now_millis(),
            FAKE_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test root");
        let exe = compile_fake(&root, "fake-java", source);
        let plan = test_plan(&root, &exe, expected_chunks, idle_timeout, min_generation);
        let snapshot = Arc::new(Mutex::new(initial_snapshot(&plan)));
        let cancel = Arc::new(AtomicBool::new(false));
        let (report, cancel_latency) = if let Some(delay) = cancel_after {
            let cancel_thread = Arc::clone(&cancel);
            let handle = thread::spawn(move || execute(plan, cancel, snapshot));
            thread::sleep(delay);
            // PowerShell compiles the C# fixture slowly on CI; time only cancellation.
            let requested = Instant::now();
            cancel_thread.store(true, Ordering::SeqCst);
            let report = handle.join().expect("fake server thread");
            (report, Some(requested.elapsed()))
        } else {
            (execute(plan, cancel, snapshot), None)
        };
        let _ = fs::remove_dir_all(&root);
        (report, cancel_latency)
    }

    #[cfg(windows)]
    const FAKE_SERVER: &str = r#"
using System;
public class FakeServer {
    public static int Main(string[] args) {
        Console.WriteLine("[12:00:00] [Server thread/INFO]: Starting Minecraft server on 127.0.0.1:25565");
        Console.WriteLine("[12:00:00] [Server thread/INFO]: Done (0.1s)! For help, type \"help\"");
        Console.Out.Flush();
        string line;
        while ((line = Console.ReadLine()) != null) {
            if (line == "stop") {
                Console.WriteLine("[12:00:02] [Server thread/INFO]: Stopping the server");
                Console.Out.Flush();
                return 0;
            }
            if (line == "save-all flush") {
                Console.WriteLine("[12:00:01] [Server thread/INFO]: Saved the game");
                Console.Out.Flush();
            }
        }
        return 3;
    }
}
"#;

    #[cfg(windows)]
    #[test]
    fn fake_server_completes_and_exits_cleanly() {
        let (report, _) = run_fake(
            FAKE_SERVER,
            0,
            Duration::from_secs(30),
            Duration::ZERO,
            None,
        );
        assert_eq!(report.stop_reason, GenerationStopReason::Completed);
        assert!(report.world_complete);
        assert_eq!(report.exit_code, Some(0));
        assert!(!report.snapshot.running);
        assert!(report.log_tail.iter().any(|line| line.contains("Done (")));
        assert_eq!(report.bind_address.as_deref(), Some("127.0.0.1:25565"));
    }

    #[cfg(windows)]
    #[test]
    fn fake_server_cancel_terminates_quickly() {
        let (report, cancel_latency) = run_fake(
            FAKE_SERVER,
            100_000,
            Duration::from_secs(300),
            Duration::ZERO,
            Some(Duration::from_secs(2)),
        );
        assert_eq!(report.stop_reason, GenerationStopReason::Cancelled);
        assert_eq!(report.snapshot.phase, GenerationPhase::Cancelled);
        let cancel_latency = cancel_latency.expect("cancel latency");
        assert!(
            cancel_latency < Duration::from_secs(10),
            "cancellation took {cancel_latency:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn fake_server_crash_is_reported_with_the_exit_code() {
        let source = r#"
using System;
public class FakeCrash {
    public static int Main(string[] args) {
        Console.WriteLine("[12:00:00] [Server thread/INFO]: Starting Minecraft server on 127.0.0.1:25565");
        Console.WriteLine("[12:00:00] [Server thread/INFO]: Done (0.1s)! For help, type \"help\"");
        Console.Out.Flush();
        System.Threading.Thread.Sleep(800);
        return 7;
    }
}
"#;
        let (report, _) = run_fake(source, 9, Duration::from_secs(30), Duration::ZERO, None);
        assert_eq!(report.stop_reason, GenerationStopReason::Crashed);
        assert_eq!(report.snapshot.phase, GenerationPhase::Failed);
        assert_eq!(report.exit_code, Some(7));
    }

    #[test]
    fn run_inventory_ignores_unrelated_files_and_clears_manifest_directories() {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-run-inventory-{}-{}-{}",
            std::process::id(),
            now_millis(),
            FAKE_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("runs root");
        let run_dir = root.join("run-1");
        fs::create_dir_all(&run_dir).expect("run dir");
        write_json(
            &run_dir.join(RUN_MANIFEST_NAME),
            &RunManifest {
                schema_version: RUN_MANIFEST_SCHEMA_VERSION,
                run_id: "run-1".to_owned(),
                created_at: 1,
                seed: "seed".to_owned(),
                preset: WorldPreset::Normal,
                center_x: 0,
                center_z: 0,
                radius_chunks: 1,
                generate_structures: true,
                datapacks: Vec::new(),
                minecraft_version: "1.21.4".to_owned(),
                server_jar_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
                eula_accepted_at: 1,
                consent_at: 2,
            },
        )
        .expect("manifest");
        fs::write(run_dir.join("keep.txt"), b"run data").expect("run data");
        fs::write(root.join("keep.txt"), b"not ours").expect("unrelated file");
        let report = scan_runs(&root);
        assert_eq!(report.runs.len(), 1);
        assert_eq!(report.runs[0].run_id, "run-1");
        assert_eq!(report.runs[0].status, "incomplete");
        assert_eq!(report.runs[0].expected_chunks, 9);
        let cleared = clear_runs(&root).expect("clear runs");
        assert!(cleared.runs.is_empty());
        assert!(root.join("keep.txt").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn completed_generation_world_is_verified_before_import() {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-import-world-{}-{}-{}",
            std::process::id(),
            now_millis(),
            FAKE_COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("runs root");
        let run_id = "run-1700000000000-1234";
        let run_dir = root.join(run_id);
        let world_dir = run_dir.join("world");
        fs::create_dir_all(world_dir.join("region")).expect("world dir");
        fs::write(world_dir.join("level.dat"), b"level").expect("level.dat");
        let manifest = RunManifest {
            schema_version: RUN_MANIFEST_SCHEMA_VERSION,
            run_id: run_id.to_owned(),
            created_at: 1,
            seed: "FuryBuilder".to_owned(),
            preset: WorldPreset::Normal,
            center_x: 0,
            center_z: 0,
            radius_chunks: 1,
            generate_structures: true,
            datapacks: Vec::new(),
            minecraft_version: "1.21.4".to_owned(),
            server_jar_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            eula_accepted_at: 1,
            consent_at: 2,
        };
        let report = GenerationReport {
            snapshot: GenerationSnapshot {
                run_id: run_id.to_owned(),
                running: false,
                phase: GenerationPhase::Completed,
                elapsed_ms: 1000,
                expected_chunks: 9,
                generated_chunks: 9,
                region_files: 1,
                world_bytes: 8192,
                message: "completed".to_owned(),
                error: None,
            },
            stop_reason: GenerationStopReason::Completed,
            world_complete: true,
            exit_code: Some(0),
            server_pid: Some(10),
            seed: manifest.seed.clone(),
            preset: manifest.preset,
            center_x: manifest.center_x,
            center_z: manifest.center_z,
            radius_chunks: manifest.radius_chunks,
            time_limit_seconds: 120,
            memory_mb: 1024,
            view_distance: 2,
            cpu_threads: 1,
            generate_structures: manifest.generate_structures,
            java_path: "java.exe".to_owned(),
            java_version: "21.0.12".to_owned(),
            server_jar: "server.jar".to_owned(),
            server_sha1: "4707d00eb834b446575d89a61a11b5d548d8c001".to_owned(),
            eula_accepted_at: 1,
            consent_at: 2,
            datapacks: manifest.datapacks.clone(),
            bind_address: Some("127.0.0.1:12345".to_owned()),
            process_tree_guarded: true,
            run_dir: run_dir.display().to_string(),
            world_dir: world_dir.display().to_string(),
            log_path: run_dir.join("logs").display().to_string(),
            started_at: 1,
            finished_at: 2,
            log_tail: Vec::new(),
        };
        write_json(&run_dir.join(RUN_MANIFEST_NAME), &manifest).expect("manifest");
        write_json(&run_dir.join(RUN_REPORT_NAME), &report).expect("report");

        let world = load_completed_world(&root, run_id).expect("completed world");
        assert_eq!(world.run_id, run_id);
        assert_eq!(world.seed, "FuryBuilder");
        assert_eq!(world.chunks, 9);
        assert_eq!(world.region_files, 1);
        assert_eq!(world.server_sha1, report.server_sha1);
        let world_path = std::path::Path::new(&world.world_dir);
        assert_eq!(
            world_path
                .parent()
                .and_then(|parent| parent.file_name())
                .and_then(|name| name.to_str()),
            Some(run_id)
        );
        assert_eq!(
            world_path.file_name().and_then(|name| name.to_str()),
            Some("world")
        );

        assert!(load_completed_world(&root, "../outside").is_err());
        assert!(load_completed_world(&root, "run-1700000000000").is_err());

        let mut cancelled = report.clone();
        cancelled.stop_reason = GenerationStopReason::Cancelled;
        cancelled.world_complete = false;
        write_json(&run_dir.join(RUN_REPORT_NAME), &cancelled).expect("cancelled report");
        assert!(load_completed_world(&root, run_id).is_err());

        let mut wrong_world = report;
        wrong_world.world_dir = root.join("other-world").display().to_string();
        write_json(&run_dir.join(RUN_REPORT_NAME), &wrong_world).expect("wrong report");
        assert!(load_completed_world(&root, run_id).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn eula_url_is_the_official_link() {
        assert_eq!(EULA_URL, "https://aka.ms/MinecraftEULA");
    }
}
