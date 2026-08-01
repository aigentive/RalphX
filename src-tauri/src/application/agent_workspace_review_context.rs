use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Notify;
use tracing::{info, warn};
use uuid::Uuid;

use crate::application::agent_workspace_review::{
    build_context, load_agent_workspace_review_context, load_current_workspace_review_eligible,
    AgentWorkspaceReviewContext, AgentWorkspaceReviewGoalContext, AgentWorkspaceReviewPacket,
    AgentWorkspaceReviewTarget,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewTargetScope,
};
use crate::{AppError, AppResult};

const PRESENTATION_CACHE_TTL: Duration = Duration::from_secs(2);
const COORDINATOR_ENTRY_TTL: Duration = Duration::from_secs(30);
const MAX_COORDINATOR_ENTRIES: usize = 128;
const WORKSPACE_REVIEW_CONTEXT_LOG_TARGET: &str =
    "ralphx_lib::application::agent_workspace_review_context";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspaceReviewContextReadMode {
    StatusSnapshot,
    FullTarget,
    FullPacket,
}

impl AgentWorkspaceReviewContextReadMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::StatusSnapshot => "status_snapshot",
            Self::FullTarget => "full_target",
            Self::FullPacket => "full_packet",
        }
    }

    fn allows_completed_cache(self) -> bool {
        self == Self::FullTarget
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct WorkspaceReviewContextKey {
    project_id: String,
    conversation_id: String,
    worktree_path: String,
}

impl From<&AgentConversationWorkspace> for WorkspaceReviewContextKey {
    fn from(workspace: &AgentConversationWorkspace) -> Self {
        Self {
            project_id: workspace.project_id.to_string(),
            conversation_id: workspace.conversation_id.to_string(),
            worktree_path: workspace.worktree_path.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MonitorGeneration {
    Missing,
    Present(Box<AgentWorkspaceReviewMonitor>),
}

impl From<Option<AgentWorkspaceReviewMonitor>> for MonitorGeneration {
    fn from(monitor: Option<AgentWorkspaceReviewMonitor>) -> Self {
        match monitor {
            Some(monitor) => Self::Present(Box::new(monitor)),
            None => Self::Missing,
        }
    }
}

#[derive(Clone)]
struct CompletedCalculation {
    generation: MonitorGeneration,
    context: AgentWorkspaceReviewContext,
    completed_at: Instant,
    sequence: u64,
}

struct CoordinatorEntryState {
    in_flight: bool,
    completed: Option<CompletedCalculation>,
    sequence: u64,
    waiters: usize,
    last_touched: Instant,
}

impl Default for CoordinatorEntryState {
    fn default() -> Self {
        Self {
            in_flight: false,
            completed: None,
            sequence: 0,
            waiters: 0,
            last_touched: Instant::now(),
        }
    }
}

#[derive(Default)]
struct CoordinatorEntry {
    state: Mutex<CoordinatorEntryState>,
    notify: Notify,
}

static CONTEXT_COORDINATOR: OnceLock<
    Mutex<HashMap<WorkspaceReviewContextKey, Arc<CoordinatorEntry>>>,
> = OnceLock::new();

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn coordinator_entry(key: &WorkspaceReviewContextKey) -> AppResult<Arc<CoordinatorEntry>> {
    let coordinator = CONTEXT_COORDINATOR.get_or_init(|| Mutex::new(HashMap::new()));
    let mut entries = lock_unpoisoned(coordinator);
    if let Some(entry) = entries.get(key) {
        return Ok(entry.clone());
    }
    let now = Instant::now();
    entries.retain(|_, entry| {
        let state = lock_unpoisoned(&entry.state);
        state.in_flight || now.duration_since(state.last_touched) <= COORDINATOR_ENTRY_TTL
    });
    if entries.len() >= MAX_COORDINATOR_ENTRIES {
        let removable = entries
            .iter()
            .filter_map(|(candidate_key, entry)| {
                let state = lock_unpoisoned(&entry.state);
                (!state.in_flight).then_some((candidate_key.clone(), state.last_touched))
            })
            .min_by_key(|(_, touched)| *touched)
            .map(|(candidate_key, _)| candidate_key);
        if let Some(removable) = removable {
            entries.remove(&removable);
        } else {
            return Err(AppError::Conflict(
                "Workspace Review context coordinator is at capacity; retry".to_string(),
            ));
        }
    }
    Ok(entries
        .entry(key.clone())
        .or_insert_with(|| Arc::new(CoordinatorEntry::default()))
        .clone())
}

pub(crate) fn invalidate_workspace_review_presentation_context(
    conversation_id: &crate::domain::entities::ChatConversationId,
) {
    let Some(coordinator) = CONTEXT_COORDINATOR.get() else {
        return;
    };
    lock_unpoisoned(coordinator).retain(|key, _| key.conversation_id != conversation_id.as_str());
}

struct CalculationOwnerGuard {
    entry: Arc<CoordinatorEntry>,
    request_id: Uuid,
    completed: bool,
}

struct RegisteredWaiterGuard {
    entry: Arc<CoordinatorEntry>,
}

impl RegisteredWaiterGuard {
    fn new(entry: Arc<CoordinatorEntry>) -> Self {
        Self { entry }
    }
}

impl Drop for RegisteredWaiterGuard {
    fn drop(&mut self) {
        let mut state = lock_unpoisoned(&self.entry.state);
        state.waiters = state.waiters.saturating_sub(1);
        state.last_touched = Instant::now();
    }
}

impl CalculationOwnerGuard {
    fn new(entry: Arc<CoordinatorEntry>, request_id: Uuid) -> Self {
        Self {
            entry,
            request_id,
            completed: false,
        }
    }

    fn finish(
        mut self,
        generation: MonitorGeneration,
        result: &AppResult<AgentWorkspaceReviewContext>,
    ) {
        self.completed = true;
        let mut state = lock_unpoisoned(&self.entry.state);
        state.in_flight = false;
        state.last_touched = Instant::now();
        state.sequence = state.sequence.saturating_add(1);
        if let Ok(context) = result {
            let sequence = state.sequence;
            state.completed = Some(CompletedCalculation {
                generation,
                context: context.clone(),
                completed_at: Instant::now(),
                sequence,
            });
        }
        drop(state);
        self.entry.notify.notify_waiters();
    }
}

impl Drop for CalculationOwnerGuard {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let mut state = lock_unpoisoned(&self.entry.state);
        state.in_flight = false;
        state.last_touched = Instant::now();
        drop(state);
        self.entry.notify.notify_waiters();
        warn!(
            target: WORKSPACE_REVIEW_CONTEXT_LOG_TARGET,
            request_id = %self.request_id,
            outcome = "owner_dropped",
            "Workspace Review context calculation owner dropped"
        );
    }
}

async fn load_monitor_generation(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<MonitorGeneration> {
    state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .map(MonitorGeneration::from)
}

fn target_from_monitor_snapshot(
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> Option<AgentWorkspaceReviewTarget> {
    let scope = monitor.current_target_scope?;
    let diff_fingerprint = monitor.current_diff_fingerprint.clone()?;
    let (base_ref, base_sha, head_ref, head_sha, source_pull_request_number) = match scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => (
            monitor.selected_source_base_ref.clone()?,
            monitor.selected_source_base_sha.clone(),
            monitor.selected_source_head_ref.clone()?,
            monitor.selected_source_head_sha.clone(),
            monitor.selected_source_pull_request_number,
        ),
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => (
            monitor.workspace_base_ref.clone()?,
            monitor.workspace_base_sha.clone(),
            monitor.workspace_head_ref.clone()?,
            monitor.workspace_head_sha.clone(),
            None,
        ),
    };
    Some(AgentWorkspaceReviewTarget {
        scope,
        base_ref,
        base_sha,
        head_ref,
        head_sha,
        diff_fingerprint,
        working_directory: PathBuf::from(&workspace.worktree_path),
        source_pull_request_number,
        review_packet: AgentWorkspaceReviewPacket::default(),
    })
}

fn status_snapshot(
    workspace: &AgentConversationWorkspace,
    generation: &MonitorGeneration,
) -> AppResult<Option<AgentWorkspaceReviewContext>> {
    let MonitorGeneration::Present(monitor) = generation else {
        return Ok(None);
    };
    if let Some(target) = target_from_monitor_snapshot(workspace, monitor) {
        return Ok(Some(build_context(
            workspace,
            monitor.as_ref().clone(),
            Some(target),
            AgentWorkspaceReviewGoalContext::default(),
        )));
    }
    if monitor.status == AgentWorkspaceReviewMonitorStatus::Reviewing {
        return Err(AppError::Conflict(
            "Workspace Review status is incomplete; retry the context request".to_string(),
        ));
    }
    Ok(None)
}

async fn calculate_generation_fenced_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    initial_generation: MonitorGeneration,
) -> AppResult<(MonitorGeneration, AgentWorkspaceReviewContext)> {
    let mut expected_generation = initial_generation;
    for attempt in 0..2 {
        let context = load_agent_workspace_review_context(state, workspace).await?;
        let current_generation = load_monitor_generation(state, workspace).await?;
        if current_generation == expected_generation {
            return Ok((current_generation, context));
        }
        if attempt == 1 {
            return Err(AppError::Conflict(
                "Workspace Review changed during context calculation; retry".to_string(),
            ));
        }
        expected_generation = current_generation;
    }
    Err(AppError::Conflict(
        "Workspace Review context calculation could not settle".to_string(),
    ))
}

async fn coordinated_full_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    mode: AgentWorkspaceReviewContextReadMode,
    request_id: Uuid,
    initial_generation: MonitorGeneration,
) -> AppResult<AgentWorkspaceReviewContext> {
    enum CoordinatorDecision {
        Cached {
            context: Box<AgentWorkspaceReviewContext>,
            waiters: usize,
        },
        Join {
            observed_sequence: u64,
            waiters: usize,
        },
        Own {
            waiters: usize,
        },
    }

    let key = WorkspaceReviewContextKey::from(workspace);
    let entry = coordinator_entry(&key)?;
    let mut generation = initial_generation;
    loop {
        let notified = entry.notify.notified();
        let decision = {
            let mut entry_state = lock_unpoisoned(&entry.state);
            entry_state.last_touched = Instant::now();
            let cached = mode.allows_completed_cache().then(|| {
                entry_state.completed.as_ref().filter(|completed| {
                    completed.generation == generation
                        && completed.completed_at.elapsed() <= PRESENTATION_CACHE_TTL
                })
            });
            if let Some(Some(completed)) = cached {
                CoordinatorDecision::Cached {
                    context: Box::new(completed.context.clone()),
                    waiters: entry_state.waiters,
                }
            } else if entry_state.in_flight {
                let observed_sequence = entry_state.sequence;
                entry_state.waiters = entry_state.waiters.saturating_add(1);
                CoordinatorDecision::Join {
                    observed_sequence,
                    waiters: entry_state.waiters,
                }
            } else {
                entry_state.in_flight = true;
                CoordinatorDecision::Own {
                    waiters: entry_state.waiters,
                }
            }
        };

        match decision {
            CoordinatorDecision::Cached { context, waiters } => {
                info!(
                    target: WORKSPACE_REVIEW_CONTEXT_LOG_TARGET,
                    request_id = %request_id,
                    mode = mode.as_str(),
                    outcome = "cache_hit",
                    waiters,
                    "Reused workspace Review presentation context"
                );
                return Ok(*context);
            }
            CoordinatorDecision::Join {
                observed_sequence,
                waiters,
            } => {
                let _waiter = RegisteredWaiterGuard::new(entry.clone());
                info!(
                    target: WORKSPACE_REVIEW_CONTEXT_LOG_TARGET,
                    request_id = %request_id,
                    mode = mode.as_str(),
                    outcome = "join",
                    waiters,
                    "Joined workspace Review context calculation"
                );
                notified.await;
                let joined_context = {
                    let entry_state = lock_unpoisoned(&entry.state);
                    entry_state
                        .completed
                        .as_ref()
                        .filter(|completed| {
                            completed.sequence > observed_sequence
                                && completed.generation == generation
                        })
                        .map(|completed| completed.context.clone())
                };
                if let Some(context) = joined_context {
                    return Ok(context);
                }
                generation = load_monitor_generation(state, workspace).await?;
            }
            CoordinatorDecision::Own { waiters } => {
                let owner = CalculationOwnerGuard::new(entry.clone(), request_id);
                info!(
                    target: WORKSPACE_REVIEW_CONTEXT_LOG_TARGET,
                    request_id = %request_id,
                    mode = mode.as_str(),
                    outcome = "owner",
                    waiters,
                    "Started workspace Review context calculation"
                );
                let result =
                    calculate_generation_fenced_context(state, workspace, generation.clone()).await;
                let (completed_generation, context_result) = match result {
                    Ok((completed_generation, context)) => (completed_generation, Ok(context)),
                    Err(error) => (generation.clone(), Err(error)),
                };
                owner.finish(completed_generation, &context_result);
                return context_result;
            }
        }
    }
}

/// The PERSISTED-ONLY workspace Review context — never recomputes, never spawns.
///
/// [`load_agent_workspace_review_presentation_context`] falls through to
/// `calculate_generation_fenced_context`, which resolves the review target with
/// `resolve_review_target` — a `git` command lane. That is the correct behaviour for the local
/// UI, and it is exactly what the remote fetch remount must never reach: the detector-(c)
/// process floor is absolute, and the 29 `diff_commands` sit denied for precisely this "may
/// spawn git" reason.
///
/// This variant is the read-only subset: it serves the monitor row the host already persisted
/// (which carries `review_artifact_id` / `review_requested_changes_artifact_id`, the ids the
/// client needs to open a review), and when no target snapshot exists it says so instead of
/// computing one. Every branch is a repository read.
///
/// Fail-closed carry-over: `status_snapshot` still refuses (`AppError::Conflict`) when the
/// monitor claims `Reviewing` without a persisted target, so an in-flight review is never
/// rendered as an idle one.
pub async fn load_persisted_workspace_review_snapshot_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<AgentWorkspaceReviewContext> {
    let workspace = load_current_workspace_review_eligible(state, workspace).await?;
    let workspace = &workspace;
    let generation = load_monitor_generation(state, workspace).await?;
    if let Some(context) = status_snapshot(workspace, &generation)? {
        return Ok(context);
    }
    let monitor = match generation {
        MonitorGeneration::Present(monitor) => *monitor,
        MonitorGeneration::Missing => AgentWorkspaceReviewMonitor::new(
            workspace.conversation_id.clone(),
            workspace.project_id.clone(),
        ),
    };
    Ok(build_context(
        workspace,
        monitor,
        None,
        AgentWorkspaceReviewGoalContext::default(),
    ))
}

pub async fn load_agent_workspace_review_presentation_context(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    mode: AgentWorkspaceReviewContextReadMode,
) -> AppResult<AgentWorkspaceReviewContext> {
    let workspace = load_current_workspace_review_eligible(state, workspace).await?;
    let workspace = &workspace;
    let request_id = Uuid::new_v4();
    let started = Instant::now();
    let generation = load_monitor_generation(state, workspace).await?;
    if mode == AgentWorkspaceReviewContextReadMode::StatusSnapshot {
        if let Some(context) = status_snapshot(workspace, &generation)? {
            info!(
                target: WORKSPACE_REVIEW_CONTEXT_LOG_TARGET,
                request_id = %request_id,
                mode = mode.as_str(),
                outcome = "snapshot",
                monitor_status = %context.monitor.status,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Loaded workspace Review status snapshot"
            );
            return Ok(context);
        }
    }
    let context = coordinated_full_context(state, workspace, mode, request_id, generation).await?;
    info!(
        target: WORKSPACE_REVIEW_CONTEXT_LOG_TARGET,
        request_id = %request_id,
        mode = mode.as_str(),
        outcome = "calculated",
        monitor_status = %context.monitor.status,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Loaded workspace Review presentation context"
    );
    Ok(context)
}
