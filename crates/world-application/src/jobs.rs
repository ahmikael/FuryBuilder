//! Bounded, cancellable job queue for long-running world operations.
//!
//! The queue owns the authoritative `WorldService` handle, executes one job at a time and
//! never invents progress. A queued job can be cancelled before it starts; a running job
//! receives a cancellation flag that the operation checks while planning and writing.
//! Cancellation during the write phase rolls back every touched section, so the world is
//! never left half-edited.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use world_operations::{Operation, OperationObserver, OperationPhase};

use crate::dto::{Actor, ActorSource, OperationRequest, OperationResult};
use crate::error::{AppError, ErrorCode};
use crate::service::WorldService;

pub const MAX_QUEUED_JOBS: usize = 16;
pub const MAX_RETAINED_JOBS: usize = 256;
pub const MAX_JOB_KIND: usize = 64;
pub const MAX_JOB_KEY: usize = 128;
pub const DEFAULT_JOB_LIST_LIMIT: usize = 50;
pub const MAX_JOB_LIST_LIMIT: usize = 200;

const JOB_APPLY: &str = "apply_operation";
const JOB_UNDO: &str = "undo_operation";
#[cfg(test)]
const JOB_TEST_PAUSE: &str = "test_pause";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl JobState {
    fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// Stable public view of a job. `result` is present only after success; `error` only after
/// failure or cancellation. The bridge maps these fields directly to its fallback tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSnapshot {
    pub job_id: String,
    pub project_id: String,
    pub kind: String,
    pub state: JobState,
    pub progress: u8,
    pub message: String,
    pub world_revision: Option<u64>,
    pub result: Option<OperationResult>,
    pub error: Option<AppError>,
    pub cancel_requested: bool,
    pub replayed: bool,
    pub created_at_ms: u64,
    pub started_at_ms: Option<u64>,
    pub finished_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobStartRequest {
    pub project_id: String,
    pub kind: String,
    #[serde(default)]
    pub parameters: Value,
    pub idempotency_key: String,
    #[serde(default)]
    pub expected_revision: Option<u64>,
    pub actor: Actor,
}

#[derive(Debug, Default)]
struct JobLive {
    progress: AtomicU8,
    message: Mutex<String>,
    cancel: AtomicBool,
}

impl JobLive {
    fn progress(&self) -> u8 {
        self.progress.load(Ordering::Acquire)
    }

    fn set_progress(&self, value: u8) {
        self.progress.store(value.min(100), Ordering::Release);
    }

    fn message(&self) -> String {
        self.message
            .lock()
            .map(|value| value.clone())
            .unwrap_or_else(|_| "job state unavailable".to_owned())
    }

    fn set_message(&self, value: impl Into<String>) {
        if let Ok(mut message) = self.message.lock() {
            *message = value.into();
        }
    }

    fn cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
}

struct JobControl {
    live: Arc<JobLive>,
}

impl JobControl {
    fn set_phase(&self, phase: OperationPhase) {
        if self.live.cancel_requested() {
            self.live.set_message("cancelling");
            return;
        }
        let label = match phase {
            OperationPhase::Validating => "validating",
            OperationPhase::Planning => "planning",
            OperationPhase::Applying => "applying",
        };
        self.live.set_message(label);
    }
}

fn scaled_percent(processed: u64, total: u64, start: u8, end: u8) -> u8 {
    let total = total.max(1);
    let done = processed.min(total);
    let span = u64::from(end.saturating_sub(start));
    let value = (u128::from(done) * u128::from(span) / u128::from(total)) as u64;
    u64::from(start).saturating_add(value).min(u64::from(end)) as u8
}

impl OperationObserver for JobControl {
    fn progress(&self, phase: OperationPhase, processed: u64, total: u64) {
        if self.live.cancel_requested() {
            self.live.set_message("cancelling");
            return;
        }
        self.set_phase(phase);
        let (start, end) = match phase {
            OperationPhase::Validating => (0, 5),
            OperationPhase::Planning => (5, 80),
            OperationPhase::Applying => (80, 99),
        };
        self.live
            .set_progress(scaled_percent(processed, total, start, end));
    }

    fn is_cancelled(&self) -> bool {
        self.live.cancel_requested()
    }
}

struct JobRecord {
    snapshot: JobSnapshot,
    request: Option<JobStartRequest>,
    live: Arc<JobLive>,
}

#[derive(Default)]
struct JobStore {
    jobs: BTreeMap<String, JobRecord>,
    order: VecDeque<String>,
    queue: VecDeque<String>,
    idempotency: BTreeMap<String, (String, String)>,
    worker_running: bool,
}

type JobListener = Arc<dyn Fn(&JobSnapshot) + Send + Sync>;

struct JobInner {
    service: Arc<Mutex<WorldService>>,
    state: Mutex<JobStore>,
    listener: Mutex<Option<JobListener>>,
}

/// Shared handle to the single-worker job queue.
#[derive(Clone)]
pub struct JobQueue {
    inner: Arc<JobInner>,
}

impl JobQueue {
    pub fn new(service: Arc<Mutex<WorldService>>) -> Self {
        Self {
            inner: Arc::new(JobInner {
                service,
                state: Mutex::new(JobStore::default()),
                listener: Mutex::new(None),
            }),
        }
    }

    /// Registers a completion callback for terminal job snapshots. The desktop uses this
    /// to refresh the viewport after an MCP job commits; the queue itself stays free of UI
    /// dependencies and runs the callback outside its state lock.
    pub fn set_listener<F>(&self, listener: F)
    where
        F: Fn(&JobSnapshot) + Send + Sync + 'static,
    {
        if let Ok(mut slot) = self.inner.listener.lock() {
            *slot = Some(Arc::new(listener));
        }
    }

    pub fn start(&self, request: JobStartRequest) -> Result<JobSnapshot, AppError> {
        validate_start(&request)?;
        let fingerprint = fingerprint(&request)?;
        let key = format!("{}:{}", request.project_id, request.idempotency_key);
        let now = now_ms();

        let (job_id, snapshot, should_spawn) = {
            let mut store = self
                .inner
                .state
                .lock()
                .map_err(|_| AppError::internal("job queue lock poisoned"))?;
            if let Some((existing_id, existing_fingerprint)) = store.idempotency.get(&key) {
                if existing_fingerprint != &fingerprint {
                    return Err(AppError::new(
                        ErrorCode::Conflict,
                        "job idempotency key was already used with a different request",
                    )
                    .with_details(json!({ "idempotencyKey": request.idempotency_key })));
                }
                let record = store.jobs.get(existing_id).ok_or_else(|| {
                    AppError::internal("job idempotency index points at a missing job")
                })?;
                let mut snapshot = live_snapshot(record);
                snapshot.replayed = true;
                return Ok(snapshot);
            }
            let active = store
                .jobs
                .values()
                .filter(|record| !record.snapshot.state.terminal())
                .count();
            if active >= MAX_QUEUED_JOBS {
                return Err(AppError::new(
                    ErrorCode::LimitExceeded,
                    "the job queue is full; cancel or wait for a running job",
                )
                .with_details(json!({
                    "limit": MAX_QUEUED_JOBS,
                    "active": active
                })));
            }

            let job_id = format!("job-{}", Uuid::new_v4().simple());
            let live = Arc::new(JobLive::default());
            live.set_message("queued");
            let snapshot = JobSnapshot {
                job_id: job_id.clone(),
                project_id: request.project_id.clone(),
                kind: request.kind.clone(),
                state: JobState::Queued,
                progress: 0,
                message: "queued".to_owned(),
                world_revision: None,
                result: None,
                error: None,
                cancel_requested: false,
                replayed: false,
                created_at_ms: now,
                started_at_ms: None,
                finished_at_ms: None,
            };
            store.jobs.insert(
                job_id.clone(),
                JobRecord {
                    snapshot: snapshot.clone(),
                    request: Some(request.clone()),
                    live,
                },
            );
            store.order.push_back(job_id.clone());
            store.queue.push_back(job_id.clone());
            store.idempotency.insert(key, (job_id.clone(), fingerprint));
            let should_spawn = !store.worker_running;
            if should_spawn {
                store.worker_running = true;
            }
            (job_id, snapshot, should_spawn)
        };

        if should_spawn {
            let inner = self.inner.clone();
            let spawn = thread::Builder::new()
                .name("furybuilder-jobs".to_owned())
                .spawn(move || run_worker(inner));
            if let Err(error) = spawn {
                if let Ok(mut store) = self.inner.state.lock() {
                    if let Some(record) = store.jobs.get_mut(&job_id) {
                        record.snapshot.state = JobState::Failed;
                        record.snapshot.message =
                            format!("could not start the job worker: {error}");
                        record.snapshot.error =
                            Some(AppError::internal(record.snapshot.message.clone()));
                        record.snapshot.finished_at_ms = Some(now_ms());
                        record.live.set_message(record.snapshot.message.clone());
                    }
                    store.queue.retain(|queued| queued != &job_id);
                    store.worker_running = false;
                }
                return Err(AppError::internal(format!(
                    "could not start the job worker: {error}"
                )));
            }
        }

        Ok(snapshot)
    }

    /// Returns the existing job for an idempotent retry without queueing anything. The
    /// fingerprint is checked exactly like `start`, so reusing a key for different work is
    /// still a conflict. Callers use this before asking the user to approve a write again.
    pub fn replay(&self, request: &JobStartRequest) -> Result<Option<JobSnapshot>, AppError> {
        validate_start(request)?;
        let fingerprint = fingerprint(request)?;
        let key = format!("{}:{}", request.project_id, request.idempotency_key);
        let store = self
            .inner
            .state
            .lock()
            .map_err(|_| AppError::internal("job queue lock poisoned"))?;
        let Some((job_id, expected)) = store.idempotency.get(&key) else {
            return Ok(None);
        };
        if expected != &fingerprint {
            return Err(AppError::new(
                ErrorCode::Conflict,
                "job idempotency key was already used with a different request",
            )
            .with_details(json!({ "idempotencyKey": request.idempotency_key })));
        }
        let record = store
            .jobs
            .get(job_id)
            .ok_or_else(|| AppError::internal("job idempotency index points at a missing job"))?;
        let mut snapshot = live_snapshot(record);
        snapshot.replayed = true;
        Ok(Some(snapshot))
    }

    /// Finds the most recent job in this project that committed an operation. Retained jobs
    /// are bounded, so an older operation may legitimately have no job record.
    pub fn find_by_operation(&self, project_id: &str, operation_id: &str) -> Option<JobSnapshot> {
        self.inner.state.lock().ok().and_then(|store| {
            store
                .order
                .iter()
                .rev()
                .filter_map(|job_id| store.jobs.get(job_id))
                .find(|record| {
                    record.snapshot.project_id == project_id
                        && record
                            .snapshot
                            .result
                            .as_ref()
                            .is_some_and(|result| result.operation_id == operation_id)
                })
                .map(live_snapshot)
        })
    }

    pub fn status(&self, project_id: &str, job_id: &str) -> Result<JobSnapshot, AppError> {
        let store = self
            .inner
            .state
            .lock()
            .map_err(|_| AppError::internal("job queue lock poisoned"))?;
        let record = store.jobs.get(job_id).ok_or_else(|| unknown_job(job_id))?;
        if record.snapshot.project_id != project_id {
            return Err(unknown_job(job_id));
        }
        Ok(live_snapshot(record))
    }

    pub fn cancel(&self, project_id: &str, job_id: &str) -> Result<JobSnapshot, AppError> {
        let mut store = self
            .inner
            .state
            .lock()
            .map_err(|_| AppError::internal("job queue lock poisoned"))?;
        let now = now_ms();
        let mut remove_from_queue = false;
        let snapshot = {
            let record = store
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| unknown_job(job_id))?;
            if record.snapshot.project_id != project_id {
                return Err(unknown_job(job_id));
            }
            match record.snapshot.state {
                JobState::Queued => {
                    record.snapshot.state = JobState::Cancelled;
                    record.snapshot.message = "cancelled before start".to_owned();
                    record.snapshot.finished_at_ms = Some(now);
                    record.live.set_message("cancelled before start");
                    record.live.cancel.store(true, Ordering::Release);
                    record.request = None;
                    remove_from_queue = true;
                    live_snapshot(record)
                }
                JobState::Running => {
                    record.live.cancel.store(true, Ordering::Release);
                    record.live.set_message("cancellation requested");
                    let mut snapshot = live_snapshot(record);
                    snapshot.message = "cancellation requested".to_owned();
                    snapshot
                }
                _ => live_snapshot(record),
            }
        };
        if remove_from_queue {
            store.queue.retain(|queued| queued != job_id);
        }
        Ok(snapshot)
    }

    /// Cancels every queued or running job created by one actor source. The desktop calls
    /// this when the MCP access policy changes so an earlier approval cannot outlive it.
    pub fn cancel_source(&self, source: ActorSource, message: &str) -> usize {
        let targets: Vec<(String, String)> = self
            .inner
            .state
            .lock()
            .map(|store| {
                store
                    .jobs
                    .values()
                    .filter(|record| {
                        !record.snapshot.state.terminal()
                            && record
                                .request
                                .as_ref()
                                .is_some_and(|request| request.actor.source == source)
                    })
                    .map(|record| {
                        (
                            record.snapshot.project_id.clone(),
                            record.snapshot.job_id.clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.cancel_targets(targets, message)
    }

    /// Cancels every active job. Called before the authoritative world is replaced or
    /// closed: a job authorized against the previous world must not commit into the next
    /// one.
    pub fn cancel_all(&self, message: &str) -> usize {
        let targets: Vec<(String, String)> = self
            .inner
            .state
            .lock()
            .map(|store| {
                store
                    .jobs
                    .values()
                    .filter(|record| !record.snapshot.state.terminal())
                    .map(|record| {
                        (
                            record.snapshot.project_id.clone(),
                            record.snapshot.job_id.clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.cancel_targets(targets, message)
    }

    fn cancel_targets(&self, targets: Vec<(String, String)>, message: &str) -> usize {
        let mut cancelled = 0;
        for (project_id, job_id) in targets {
            if self.cancel(&project_id, &job_id).is_ok() {
                cancelled += 1;
            }
        }
        if cancelled > 0 {
            // Keep the reason visible without changing a terminal snapshot.
            if let Ok(mut store) = self.inner.state.lock() {
                for record in store.jobs.values_mut() {
                    if record.snapshot.state == JobState::Running && record.live.cancel_requested()
                    {
                        record.live.set_message(message);
                    }
                }
            }
        }
        cancelled
    }

    pub fn list(&self, project_id: &str, limit: usize) -> Vec<JobSnapshot> {
        let limit = limit.clamp(1, MAX_JOB_LIST_LIMIT);
        self.inner
            .state
            .lock()
            .map(|store| {
                store
                    .order
                    .iter()
                    .rev()
                    .filter_map(|job_id| store.jobs.get(job_id))
                    .filter(|record| record.snapshot.project_id == project_id)
                    .take(limit)
                    .map(live_snapshot)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn summary(&self) -> Value {
        self.inner
            .state
            .lock()
            .map(|store| {
                let count = |state: JobState| {
                    store
                        .jobs
                        .values()
                        .filter(|record| record.snapshot.state == state)
                        .count()
                };
                json!({
                    "available": true,
                    "queued": count(JobState::Queued),
                    "running": count(JobState::Running),
                    "succeeded": count(JobState::Succeeded),
                    "failed": count(JobState::Failed),
                    "cancelled": count(JobState::Cancelled),
                    "retained": store.jobs.len(),
                    "maxQueued": MAX_QUEUED_JOBS,
                })
            })
            .unwrap_or_else(|_| json!({ "available": false }))
    }
}

fn validate_start(request: &JobStartRequest) -> Result<(), AppError> {
    if request.project_id.trim().is_empty() {
        return Err(AppError::invalid_argument("project_id must not be empty"));
    }
    if request.project_id.len() > 128 {
        return Err(AppError::invalid_argument(
            "project_id must be at most 128 characters",
        ));
    }
    if request.idempotency_key.trim().is_empty() || request.idempotency_key.len() > MAX_JOB_KEY {
        return Err(AppError::invalid_argument(format!(
            "idempotency_key must be 1..={MAX_JOB_KEY} characters"
        )));
    }
    if request.kind.trim().is_empty() || request.kind.len() > MAX_JOB_KIND {
        return Err(AppError::invalid_argument(format!(
            "job kind must be 1..={MAX_JOB_KIND} characters"
        )));
    }
    match request.kind.as_str() {
        JOB_APPLY => {
            parse_apply(&request.parameters)?;
        }
        JOB_UNDO => {
            parse_undo(&request.parameters)?;
        }
        #[cfg(test)]
        JOB_TEST_PAUSE => {}
        other => {
            return Err(
                AppError::invalid_argument(format!("unsupported job kind `{other}`"))
                    .with_details(json!({ "supportedKinds": [JOB_APPLY, JOB_UNDO] })),
            );
        }
    }
    Ok(())
}

fn fingerprint(request: &JobStartRequest) -> Result<String, AppError> {
    serde_json::to_string(&json!({
        "projectId": request.project_id,
        "kind": request.kind,
        "parameters": request.parameters,
        "expectedRevision": request.expected_revision,
    }))
    .map_err(|error| AppError::internal(format!("could not fingerprint the job: {error}")))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyJobParameters {
    operation: Operation,
    #[serde(default)]
    linked_object_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UndoJobParameters {
    #[serde(default)]
    operation_id: Option<String>,
}

fn parse_apply(parameters: &Value) -> Result<ApplyJobParameters, AppError> {
    serde_json::from_value(parameters.clone()).map_err(|error| {
        AppError::invalid_argument(format!(
            "apply_operation job parameters need an operation object: {error}"
        ))
    })
}

fn parse_undo(parameters: &Value) -> Result<UndoJobParameters, AppError> {
    let parsed: UndoJobParameters =
        serde_json::from_value(parameters.clone()).map_err(|error| {
            AppError::invalid_argument(format!("invalid undo_operation job parameters: {error}"))
        })?;
    if let Some(id) = &parsed.operation_id {
        if id.trim().is_empty() || id.len() > MAX_JOB_KEY {
            return Err(AppError::invalid_argument(format!(
                "operation_id must be 1..={MAX_JOB_KEY} characters"
            )));
        }
    }
    Ok(parsed)
}

/// Parses the operation of an `apply_operation` job before it is queued so the caller can
/// apply the same access policy and confirmation checks as a synchronous write. The job
/// worker uses the same parser again, so the checked operation is the executed one.
pub fn apply_job_operation(parameters: &Value) -> Result<Operation, AppError> {
    Ok(parse_apply(parameters)?.operation)
}

/// Parses the optional target operation of an `undo_operation` job.
pub fn undo_job_operation_id(parameters: &Value) -> Result<Option<String>, AppError> {
    Ok(parse_undo(parameters)?.operation_id)
}

fn execute_job(
    service: &Arc<Mutex<WorldService>>,
    request: &JobStartRequest,
    control: &JobControl,
) -> Result<OperationResult, AppError> {
    match request.kind.as_str() {
        JOB_APPLY => {
            let parameters = parse_apply(&request.parameters)?;
            let mut service = service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            service.execute_with_observer(
                OperationRequest {
                    project_id: request.project_id.clone(),
                    operation: parameters.operation,
                    expected_revision: request.expected_revision,
                    idempotency_key: Some(request.idempotency_key.clone()),
                    actor: request.actor.clone(),
                    linked_object_id: parameters.linked_object_id,
                },
                control,
            )
        }
        JOB_UNDO => {
            let parameters = parse_undo(&request.parameters)?;
            let mut service = service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            if let Some(expected) = request.expected_revision {
                let current = service.world().revision();
                if expected != current {
                    return Err(AppError::new(
                        ErrorCode::RevisionMismatch,
                        format!("expected revision {expected}, current revision is {current}"),
                    )
                    .with_details(json!({ "expected": expected, "current": current })));
                }
            }
            service.undo(
                &request.project_id,
                request.actor.clone(),
                parameters.operation_id.as_deref(),
            )
        }
        #[cfg(test)]
        JOB_TEST_PAUSE => {
            for _ in 0..100 {
                if control.is_cancelled() {
                    return Err(AppError::new(
                        ErrorCode::Cancelled,
                        "job was cancelled during the test pause",
                    ));
                }
                thread::sleep(std::time::Duration::from_millis(5));
            }
            let revision = service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?
                .world()
                .revision();
            Ok(OperationResult {
                operation_id: "op-test-pause".to_owned(),
                world_revision: revision,
                changed_blocks: 0,
                dirty_chunks: Vec::new(),
                warnings: Vec::new(),
                replayed: false,
            })
        }
        other => Err(AppError::invalid_argument(format!(
            "unsupported job kind `{other}`"
        ))),
    }
}

fn run_worker(inner: Arc<JobInner>) {
    loop {
        let next = {
            let mut store = match inner.state.lock() {
                Ok(store) => store,
                Err(_) => return,
            };
            match store.queue.pop_front() {
                Some(job_id) => {
                    let Some(record) = store.jobs.get_mut(&job_id) else {
                        continue;
                    };
                    record.snapshot.state = JobState::Running;
                    record.snapshot.started_at_ms = Some(now_ms());
                    record.snapshot.message = "starting".to_owned();
                    record.live.set_message("starting");
                    Some((
                        job_id,
                        record.request.clone().expect("queued job has a request"),
                        record.live.clone(),
                    ))
                }
                None => {
                    store.worker_running = false;
                    None
                }
            }
        };

        let Some((job_id, request, live)) = next else {
            break;
        };
        let control = JobControl { live: live.clone() };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute_job(&inner.service, &request, &control)
        }))
        .unwrap_or_else(|_| {
            Err(AppError::internal(
                "the job worker panicked; the world transaction was not committed",
            ))
        });
        let finished_at_ms = now_ms();

        let mut store = match inner.state.lock() {
            Ok(store) => store,
            Err(_) => return,
        };
        if let Some(record) = store.jobs.get_mut(&job_id) {
            record.request = None;
            record.snapshot.finished_at_ms = Some(finished_at_ms);
            match result {
                Ok(result) => {
                    record.snapshot.state = JobState::Succeeded;
                    record.snapshot.progress = 100;
                    record.snapshot.message = "succeeded".to_owned();
                    record.snapshot.world_revision = Some(result.world_revision);
                    record.snapshot.result = Some(result);
                    record.live.set_progress(100);
                    record.live.set_message("succeeded");
                }
                Err(error) => {
                    let cancelled = error.code == ErrorCode::Cancelled;
                    record.snapshot.state = if cancelled {
                        JobState::Cancelled
                    } else {
                        JobState::Failed
                    };
                    record.snapshot.message = if cancelled {
                        "cancelled".to_owned()
                    } else {
                        error.message.clone()
                    };
                    record.snapshot.progress = record.live.progress().min(99);
                    record.snapshot.error = Some(error);
                    record.live.set_message(record.snapshot.message.clone());
                }
            }
        }
        trim_retained(&mut store);
        let finished = store.jobs.get(&job_id).map(live_snapshot);
        drop(store);
        if let Some(snapshot) = finished {
            let listener = inner.listener.lock().ok().and_then(|slot| slot.clone());
            if let Some(listener) = listener {
                listener(&snapshot);
            }
        }
    }
}

fn live_snapshot(record: &JobRecord) -> JobSnapshot {
    let mut snapshot = record.snapshot.clone();
    snapshot.progress = record.live.progress();
    snapshot.message = record.live.message();
    snapshot.cancel_requested = record.live.cancel_requested();
    snapshot
}

fn trim_retained(store: &mut JobStore) {
    while store.jobs.len() > MAX_RETAINED_JOBS {
        let Some(index) = store.order.iter().position(|job_id| {
            store
                .jobs
                .get(job_id)
                .is_some_and(|record| record.snapshot.state.terminal())
        }) else {
            break;
        };
        let Some(job_id) = store.order.remove(index) else {
            break;
        };
        store.jobs.remove(&job_id);
        store.idempotency.retain(|_, (id, _)| id != &job_id);
    }
}

fn unknown_job(job_id: &str) -> AppError {
    AppError::new(ErrorCode::NotFound, format!("unknown job `{job_id}`"))
        .with_details(json!({ "jobId": job_id }))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use world_domain::BlockPos;

    fn queue() -> (JobQueue, Arc<Mutex<WorldService>>) {
        let service = Arc::new(Mutex::new(WorldService::new()));
        (JobQueue::new(service.clone()), service)
    }

    fn apply_request(key: &str, pos: BlockPos) -> JobStartRequest {
        JobStartRequest {
            project_id: "project-1".to_owned(),
            kind: JOB_APPLY.to_owned(),
            parameters: json!({
                "operation": {
                    "type": "set_block",
                    "dimension": "minecraft:overworld",
                    "pos": pos,
                    "state": { "name": "minecraft:stone" }
                }
            }),
            idempotency_key: key.to_owned(),
            expected_revision: None,
            actor: Actor::mcp("mcp-bridge"),
        }
    }

    fn wait_terminal(queue: &JobQueue, job_id: &str) -> JobSnapshot {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = queue.status("project-1", job_id).expect("job status");
            if snapshot.state.terminal() {
                return snapshot;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "job did not finish: {snapshot:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn applies_a_job_and_reports_completion() {
        let (queue, service) = queue();
        let snapshot = queue
            .start(apply_request("job-1", BlockPos::new(1, 64, 1)))
            .unwrap();
        assert_eq!(snapshot.state, JobState::Queued);
        let finished = wait_terminal(&queue, &snapshot.job_id);
        assert_eq!(finished.state, JobState::Succeeded, "{finished:?}");
        assert_eq!(finished.progress, 100);
        assert_eq!(finished.world_revision, Some(1));
        assert_eq!(finished.result.as_ref().expect("result").changed_blocks, 1);
        let revision = service.lock().expect("service").world().revision();
        assert_eq!(revision, 1);
    }

    #[test]
    fn duplicate_start_is_idempotent_and_conflicts_are_rejected() {
        let (queue, _) = queue();
        let first = queue
            .start(apply_request("same-key", BlockPos::new(2, 64, 2)))
            .unwrap();
        let second = queue
            .start(apply_request("same-key", BlockPos::new(2, 64, 2)))
            .unwrap();
        assert_eq!(first.job_id, second.job_id);
        assert!(second.replayed);
        wait_terminal(&queue, &first.job_id);

        let conflict = queue
            .start(apply_request("same-key", BlockPos::new(3, 64, 3)))
            .unwrap_err();
        assert_eq!(conflict.code, ErrorCode::Conflict);
    }

    #[test]
    fn running_job_can_be_cancelled_without_committing() {
        let (queue, service) = queue();
        let request = JobStartRequest {
            project_id: "project-1".to_owned(),
            kind: JOB_TEST_PAUSE.to_owned(),
            parameters: json!({}),
            idempotency_key: "cancel-me".to_owned(),
            expected_revision: None,
            actor: Actor::mcp("mcp-bridge"),
        };
        let started = queue.start(request).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let snapshot = queue.status("project-1", &started.job_id).unwrap();
            if snapshot.state == JobState::Running {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "job never started");
            thread::sleep(Duration::from_millis(5));
        }
        let requested = queue.cancel("project-1", &started.job_id).unwrap();
        assert!(requested.cancel_requested);
        let finished = wait_terminal(&queue, &started.job_id);
        assert_eq!(finished.state, JobState::Cancelled);
        assert_eq!(service.lock().expect("service").world().revision(), 0);
    }

    #[test]
    fn replay_returns_the_same_job_without_duplicate_work() {
        let (queue, service) = queue();
        let request = apply_request("replay-key", BlockPos::new(6, 64, 6));
        let first = queue.start(request.clone()).unwrap();
        let replay = queue
            .replay(&request)
            .expect("replay lookup")
            .expect("existing job");
        assert_eq!(replay.job_id, first.job_id);
        assert!(replay.replayed);

        let finished = wait_terminal(&queue, &first.job_id);
        assert_eq!(finished.state, JobState::Succeeded);
        assert_eq!(service.lock().expect("service").world().revision(), 1);
        let operation_id = finished
            .result
            .as_ref()
            .expect("completed result")
            .operation_id
            .clone();
        let linked = queue
            .find_by_operation("project-1", &operation_id)
            .expect("job linked to its operation");
        assert_eq!(linked.job_id, first.job_id);
        assert!(
            queue
                .find_by_operation("project-2", &operation_id)
                .is_none(),
            "job lookup must stay inside the requested project"
        );

        let conflict = queue
            .replay(&apply_request("replay-key", BlockPos::new(7, 64, 7)))
            .unwrap_err();
        assert_eq!(conflict.code, ErrorCode::Conflict);
        assert_eq!(service.lock().expect("service").world().revision(), 1);
    }

    #[test]
    fn status_and_cancel_are_scoped_to_the_requesting_project() {
        let (queue, _) = queue();
        let started = queue
            .start(apply_request("scope", BlockPos::new(4, 64, 4)))
            .unwrap();
        wait_terminal(&queue, &started.job_id);
        assert_eq!(
            queue.status("project-2", &started.job_id).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(
            queue.cancel("project-2", &started.job_id).unwrap_err().code,
            ErrorCode::NotFound
        );
        assert_eq!(queue.list("project-1", 10).len(), 1);
        assert!(queue.list("project-2", 10).is_empty());
    }

    #[test]
    fn completion_listener_sees_a_committed_job() {
        let (queue, service) = queue();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = seen.clone();
        queue.set_listener(move |snapshot| {
            captured.lock().expect("listener").push(snapshot.clone());
        });
        let started = queue
            .start(apply_request("listener-key", BlockPos::new(3, 64, 3)))
            .unwrap();
        let finished = wait_terminal(&queue, &started.job_id);
        assert_eq!(finished.state, JobState::Succeeded);

        let deadline = Instant::now() + Duration::from_secs(2);
        while seen.lock().expect("listener").is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let seen = seen.lock().expect("listener");
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].state, JobState::Succeeded);
        assert_eq!(seen[0].result.as_ref().expect("result").changed_blocks, 1);
        assert_eq!(service.lock().expect("service").world().revision(), 1);
    }

    #[test]
    fn cancel_all_stops_jobs_before_the_world_is_replaced() {
        let (queue, service) = queue();
        let paused = queue
            .start(JobStartRequest {
                project_id: "project-1".to_owned(),
                kind: JOB_TEST_PAUSE.to_owned(),
                parameters: json!({}),
                idempotency_key: "world-replaced".to_owned(),
                expected_revision: None,
                actor: Actor::mcp("mcp-bridge"),
            })
            .unwrap();
        let cancelled = queue.cancel_all("the open project changed");
        assert_eq!(cancelled, 1);
        let finished = wait_terminal(&queue, &paused.job_id);
        assert_eq!(finished.state, JobState::Cancelled);
        assert_eq!(service.lock().expect("service").world().revision(), 0);
    }

    #[test]
    fn unsupported_kinds_and_bad_parameters_are_rejected_before_queueing() {
        let (queue, _) = queue();
        let mut request = apply_request("bad", BlockPos::new(0, 64, 0));
        request.kind = "generate_everything".to_owned();
        assert_eq!(
            queue.start(request).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        let mut request = apply_request("bad-params", BlockPos::new(0, 64, 0));
        request.parameters = json!({ "operation": { "type": "set_block" } });
        assert_eq!(
            queue.start(request).unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn cancel_source_cancels_queued_mcp_jobs() {
        let (queue, _) = queue();
        let started = queue
            .start(apply_request("source-cancel", BlockPos::new(5, 64, 5)))
            .unwrap();
        wait_terminal(&queue, &started.job_id);
        let paused = queue
            .start(JobStartRequest {
                project_id: "project-1".to_owned(),
                kind: JOB_TEST_PAUSE.to_owned(),
                parameters: json!({}),
                idempotency_key: "source-pause".to_owned(),
                expected_revision: None,
                actor: Actor::mcp("mcp-bridge"),
            })
            .unwrap();
        let cancelled = queue.cancel_source(ActorSource::Mcp, "policy changed");
        assert!(cancelled >= 1);
        let finished = wait_terminal(&queue, &paused.job_id);
        assert_eq!(finished.state, JobState::Cancelled);
    }
}
