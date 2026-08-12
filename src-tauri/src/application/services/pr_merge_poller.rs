// PR merge poller registry (AD1, AD9, AD11, AD18)
//
// Manages background polling tasks that watch GitHub PRs until they are merged,
// then trigger the existing post_merge_cleanup pipeline.
//
// Phase 3: Full poll_loop implementation with adaptive polling, rate limiting,
// crash recovery, and cancellation.

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

use crate::application::agent_conversation_workspace::{
    agent_name_for_workspace_mode, resolve_valid_agent_conversation_workspace_path,
};
use crate::application::agent_runtime_context::branch_status::BranchStatusCache;
use crate::application::agent_workspace_base_staleness::{
    classify_health_hold_disposition, BaseStalenessObservation, HealthHoldDisposition,
};
use crate::application::agent_workspace_ci_rerun::ci_rerun_hold_still_pending;
use crate::application::agent_workspace_pr_autofix_attempt::{
    load_pr_autofix_attempt_decision, pr_autofix_action_metadata,
};
use crate::application::agent_workspace_publish_recovery::{
    recover_stale_publish_repair_for_workspace_in_state_result, StalePublishRepairRecoveryOutcome,
    CONTINUATION_OPEN_EFFECT_ATTENTION_REASON, CONTINUATION_OPEN_EFFECT_EVIDENCE_REASON_PREFIX,
    CONTINUATION_OPEN_EFFECT_REARMED_STEP, CONTINUATION_OPEN_EFFECT_RECOVERY_REASON_PREFIX,
};
use crate::application::agent_workspace_publish_repair_state::{
    agent_workspace_repair_is_base_stale_held, agent_workspace_repair_is_ci_held,
    agent_workspace_repair_is_health_held, classify_agent_workspace_repair_delivery,
    held_repair_has_unpublished_head, mark_agent_workspace_base_update_target,
    release_agent_workspace_base_stale_hold, release_and_clear_agent_workspace_repair_target_lease,
    reserve_agent_workspace_base_stale_hold, reserve_agent_workspace_base_update,
    reserve_agent_workspace_repair_dispatch, settle_agent_workspace_repair_dispatch_outcome,
    start_or_join_agent_workspace_repair, start_or_join_agent_workspace_repair_without_projection,
    validate_agent_workspace_repair_target_lease, AgentWorkspaceRepairDispatchOutcome,
    AgentWorkspaceRepairDispatchSettlement, AgentWorkspaceRepairStartOutcome,
    AgentWorkspaceRepairStartRequest, AgentWorkspaceRepairTransitionOutcome,
};
#[cfg(test)]
use crate::application::agent_workspace_publish_repair_state::{
    claim_agent_workspace_repair, repair_run_event_classification,
    settle_agent_workspace_repair_failure, AgentWorkspaceRepairClaim,
};
use crate::application::agent_workspace_terminal_cleanup::{
    settle_review_pr_terminal_observation, terminalize_agent_workspace_after_pr,
    TerminalAgentWorkspaceCause,
};
use crate::application::chat_service::{ChatService, SendMessageOptions, SendQueuePolicy};
use crate::application::interactive_notification_producer::pr_review_notification_key;
use crate::application::publish_resilience::{
    ensure_publish_branch_fresh, verify_agent_workspace_settled_current_head,
    AgentWorkspaceSettledHeadCheck, PublishBranchFreshnessOutcome,
};
use crate::application::services::pr_auto_merge_status::{
    auto_merge_disable_failure_summary, auto_merge_enable_failure_summary,
    AUTO_MERGE_SUPERVISION_STATUS_WAITING,
};
use crate::application::services::pr_snapshot_hub::PrSnapshotHub;
use crate::application::task_transition_service::PrBranchFreshnessOutcome;
use crate::application::{AppState, GitService, NotificationService, TaskTransitionService};
use crate::domain::entities::plan_branch::PrStatus as DbPrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, AgentRunId,
    AgentWorkspacePrCommentEvidenceUpsert, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairContinuation,
    AgentWorkspaceRepairOperationStatus, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
    ChatContextType, ChatConversationId, IdeationSessionId, ProjectId,
};
use crate::domain::entities::{InternalStatus, PlanBranch, PlanBranchId, Project, TaskId};
use crate::domain::entities::{
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget,
    NotificationTargetKind,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository,
    AgentWorkspaceRepairAttemptTransition, AgentWorkspaceRepairAttemptTransitionOutcome,
    AgentWorkspaceRepairRepository, BranchUpdateRepository, PlanBranchRepository,
    SettleAgentWorkspaceRepairAttempt, SettleAgentWorkspaceRepairAttemptOutcome,
};
use crate::domain::services::github_service::{
    PrHealth, PrHealthCheck, PrMergeStateStatus, PrMergeableState, PrReviewCommentFeedback,
    PrReviewFeedback,
};
use crate::domain::services::{GithubServiceTrait, PrStatus};
use crate::error::AppError;
use crate::infrastructure::agents::claude::agent_names::{
    AGENT_WORKSPACE_PR_FIXER, AGENT_WORKSPACE_REPAIR,
};
use crate::infrastructure::agents::claude::git_runtime_config;

#[cfg(test)]
const AGENT_WORKSPACE_REPAIR_REQUESTED_STEP: &str = "repair_requested";
#[cfg(test)]
const AGENT_WORKSPACE_REPAIR_SENT_STEP: &str = "repair_sent";
#[cfg(test)]
const AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY_CLASSIFICATION: &str = "agent_fixable:update_only";
const AGENT_WORKSPACE_AUTO_MERGE_DISARM_STEP: &str = "auto_merge_disabled_for_repair";
const AGENT_WORKSPACE_AUTO_MERGE_DISARM_SUMMARY: &str =
    "Temporarily disabled GitHub auto-merge before starting PR repair.";

static HELD_PR_HEALTH_RECHECKS: OnceLock<
    DashMap<ChatConversationId, Arc<tokio::sync::OnceCell<Result<bool, String>>>>,
> = OnceLock::new();

#[derive(Clone)]
struct HeldPrHealthRecheckAuthority {
    attempt_id: AgentWorkspaceRepairAttemptId,
    generation: u64,
    updated_at: chrono::DateTime<chrono::Utc>,
}

async fn repair_owns_agent_workspace_pr_supervision_status(
    repair_repo: Option<&dyn AgentWorkspaceRepairRepository>,
    conversation_id: &ChatConversationId,
) -> crate::AppResult<bool> {
    match repair_repo {
        Some(repair_repo) => Ok(repair_repo
            .get_current_repair_attempt(conversation_id)
            .await?
            .is_some_and(|attempt| attempt.is_unsettled())),
        None => Ok(false),
    }
}

pub(crate) async fn update_agent_workspace_pr_supervision_preferences(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    conversation_id: &ChatConversationId,
    autofix_enabled: bool,
    auto_merge_desired: bool,
    auto_merge_method: &str,
) -> crate::AppResult<()> {
    if repair_owns_agent_workspace_pr_supervision_status(Some(repair_repo), conversation_id).await?
    {
        workspace_repo
            .update_pr_supervision_preferences_preserving_status(
                conversation_id,
                autofix_enabled,
                auto_merge_desired,
                auto_merge_method,
            )
            .await
    } else {
        workspace_repo
            .update_pr_supervision_preferences(
                conversation_id,
                autofix_enabled,
                auto_merge_desired,
                auto_merge_method,
            )
            .await
    }
}

pub(crate) async fn update_agent_workspace_pr_supervision_state(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    repair_repo: Option<&dyn AgentWorkspaceRepairRepository>,
    conversation_id: &ChatConversationId,
    auto_merge_current: Option<bool>,
    status: Option<&str>,
    summary: Option<&str>,
) -> crate::AppResult<()> {
    let repair_owns_status =
        repair_owns_agent_workspace_pr_supervision_status(repair_repo, conversation_id).await?;

    workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            auto_merge_current,
            (!repair_owns_status).then_some(status).flatten(),
            summary,
        )
        .await
}

// ────────────────────────────────────────────────────────────────────
// Rate limit state shared across all pollers in the registry
// ────────────────────────────────────────────────────────────────────

/// Tracks GitHub API rate limit state parsed from `gh api --include` headers.
/// Shared across all pollers via `Arc<Mutex<RateLimitState>>`.
#[derive(Debug)]
pub struct RateLimitState {
    /// Remaining calls in the current window
    pub remaining: u32,
    /// When the rate limit resets (used when remaining < 100 to sleep until reset)
    pub reset_at: Instant,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            remaining: 5000, // conservative default — no throttling until we get real data
            reset_at: Instant::now() + Duration::from_secs(3600),
        }
    }
}

/// How far ahead to assume the window resets when GitHub rejected a call but no probe has yet
/// supplied a real reset instant. Deliberately short: an over-long guess would stall every poller
/// for longer than the outage, and the next probe replaces it with the truth anyway.
const RATE_LIMITED_FALLBACK_RESET: Duration = Duration::from_secs(15 * 60);

/// Refreshes the shared rate-limit state from GitHub's quota-free `rate_limit` endpoint.
///
/// Single-flight and interval-gated: whichever poll iteration arrives first past the interval
/// performs the probe and every other poller reads the result. A failed or unsupported probe
/// leaves the previous state alone — a read RalphX could not perform is not evidence about the
/// budget. Runs inside an existing async poll iteration, so no thread is spawned.
async fn maybe_refresh_rate_limit(
    github: &Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    rate_limit: &Arc<std::sync::Mutex<RateLimitState>>,
    last_probe: &Arc<std::sync::Mutex<Option<Instant>>>,
) {
    let interval = Duration::from_secs(git_runtime_config().github_rate_limit_probe_interval_secs);
    {
        // Claim the probe slot before awaiting so concurrent iterations cannot pile on.
        let Ok(mut last) = last_probe.lock() else {
            return;
        };
        if last.is_some_and(|at| at.elapsed() < interval) {
            return;
        }
        *last = Some(Instant::now());
    }

    let snapshot = match github.fetch_rate_limit(working_dir).await {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return,
        Err(error) => {
            tracing::debug!(error = %error, "GitHub rate-limit probe failed; keeping prior state");
            return;
        }
    };

    let Some(reset_at) = instant_from_epoch_secs(snapshot.reset_epoch_secs) else {
        return;
    };
    if let Ok(mut state) = rate_limit.lock() {
        state.remaining = snapshot.remaining;
        state.reset_at = reset_at;
    }
}

/// Converts GitHub's absolute reset epoch into a monotonic `Instant`.
///
/// Returns `None` when the system clock cannot produce a Unix timestamp; a reset already in the
/// past collapses to "now", which correctly reads as "no longer limited".
fn instant_from_epoch_secs(epoch_secs: u64) -> Option<Instant> {
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(Instant::now() + Duration::from_secs(epoch_secs.saturating_sub(now_epoch)))
}

/// Records an observed rate-limit rejection into the shared state.
///
/// Zeroes `remaining` so every poll loop and the durable recovery sweep back off together.
/// `reset_at` is only pushed out when the currently known reset has already passed: a real reset
/// instant from the `gh api rate_limit` probe must always win over this conservative fallback.
fn note_rate_limited(rate_limit: &Arc<std::sync::Mutex<RateLimitState>>) {
    if let Ok(mut state) = rate_limit.lock() {
        state.remaining = 0;
        let now = Instant::now();
        if state.reset_at <= now {
            state.reset_at = now + RATE_LIMITED_FALLBACK_RESET;
        }
    }
}

/// Keeps one PR registered with the snapshot hub for exactly as long as its poller runs.
///
/// The workspace poll loop returns from many places — terminalization, stop requests, routed
/// repairs, semaphore shutdown — and an aborted `JoinHandle` drops mid-iteration with no return at
/// all. A `Drop` guard is the only way to withdraw the PR on all of those paths; a manual
/// `unregister` before each `return` would silently miss the abort case and leave a dead PR
/// inflating every future batch for that repository.
struct PrSnapshotRegistration {
    hub: Arc<PrSnapshotHub>,
    repo_key: String,
    pr_number: i64,
}

impl PrSnapshotRegistration {
    fn new(hub: Arc<PrSnapshotHub>, repo_key: String, pr_number: i64) -> Self {
        hub.register(&repo_key, pr_number);
        Self {
            hub,
            repo_key,
            pr_number,
        }
    }
}

impl Drop for PrSnapshotRegistration {
    fn drop(&mut self) {
        self.hub.unregister(&self.repo_key, self.pr_number);
    }
}

/// Applies shared rate-limit pressure to a poll interval.
///
/// Mirrors the task loop's ladder: below 500 remaining, back off; below 100, stop calling GitHub
/// entirely until the window resets. Returns the adjusted interval plus how long the caller must
/// sleep before its next GitHub read.
fn apply_rate_limit_pressure(
    rate_limit: &Arc<std::sync::Mutex<RateLimitState>>,
    interval: Duration,
    max_interval: Duration,
) -> (Duration, Duration) {
    let Ok(state) = rate_limit.lock() else {
        return (interval, Duration::ZERO);
    };
    if state.remaining < 100 {
        return (
            max_interval,
            state.reset_at.saturating_duration_since(Instant::now()),
        );
    }
    if state.remaining < 500 {
        return ((interval * 2).min(max_interval), Duration::ZERO);
    }
    (interval, Duration::ZERO)
}

// ────────────────────────────────────────────────────────────────────
// Registry
// ────────────────────────────────────────────────────────────────────

/// Registry of active PR polling tasks.
///
/// Each entry tracks one GitHub PR (keyed by TaskId) that is being polled
/// until it reaches the MERGED state, at which point the transition pipeline fires.
///
/// - `active` — JoinHandle per task. Allows liveness check (`is_finished()`) + cancellation (`abort()`).
/// - `stopping` — race guard set by `stop_polling` BEFORE abort. Prevents post-cleanup transitions. (AD11)
/// - `semaphore` — limits concurrent `gh` calls to avoid thundering herd.
/// - `rate_limit` — shared rate limit state updated from API headers.
pub struct PrPollerRegistry {
    /// Active poller handles keyed by TaskId. JoinHandle supports is_finished() + abort().
    active: Arc<DashMap<TaskId, JoinHandle<()>>>,

    /// Active direct agent-workspace PR poller handles keyed by conversation id.
    workspace_active: Arc<DashMap<ChatConversationId, JoinHandle<()>>>,

    /// Race guard: inserted BEFORE abort in stop_polling. poll_loop checks before calling transition.
    pub(crate) stopping: Arc<DashMap<TaskId, ()>>,

    /// Race guard for direct agent-workspace PR pollers.
    workspace_stopping: Arc<DashMap<ChatConversationId, ()>>,

    /// Guards PR creation — prevents duplicate draft PR creation per plan branch. (AD10)
    /// Shared with TaskServices so the merge entry action can lock before creating.
    pub pr_creation_guard: Arc<DashMap<PlanBranchId, ()>>,

    /// Limits the number of concurrent gh poll calls at once. (AD9)
    semaphore: Arc<tokio::sync::Semaphore>,

    /// Shared rate limit state parsed from gh API headers. (AD9)
    rate_limit: Arc<std::sync::Mutex<RateLimitState>>,

    /// When any poller last probed `gh api rate_limit`. Gates the shared probe to one call per
    /// configured interval across every poll loop in the registry.
    rate_limit_last_probe: Arc<std::sync::Mutex<Option<Instant>>>,

    /// Batched per-repository PR reads shared by every workspace poller on that repository.
    pr_snapshot_hub: Arc<PrSnapshotHub>,

    /// GitHub service for PR status checks. None when GitHub integration is disabled.
    github_service: Option<Arc<dyn GithubServiceTrait>>,

    /// Plan branch repository for reading/updating branch metadata.
    plan_branch_repo: Arc<dyn PlanBranchRepository>,

    /// Shared best-effort notification settlement for direct workspace pollers.
    notification_service: Arc<std::sync::RwLock<Option<Arc<NotificationService>>>>,

    /// Canonical Git target authority for durable repair dispatch. It is installed by the
    /// production AppState once and copied into each direct workspace poller.
    branch_update_repo: Arc<std::sync::RwLock<Option<Arc<dyn BranchUpdateRepository>>>>,

    /// Last branch observations consumed synchronously by agent runtime-context composition.
    branch_status_cache: BranchStatusCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspacePrPollerStart {
    Started,
    AlreadyRunning,
    Unavailable,
}

pub async fn start_review_pr_lifecycle_polling(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &crate::domain::entities::AgentWorkspacePrReviewMonitor,
) -> crate::AppResult<AgentWorkspacePrPollerStart> {
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr
        || workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.has_terminal_publication_pr_status()
        || monitor.status == AgentWorkspacePrReviewMonitorStatus::Terminal
    {
        return Err(AppError::Conflict(
            "Review PR lifecycle polling requires an active nonterminal workspace".to_string(),
        ));
    }
    let pr_number = workspace
        .source_pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
        .or(workspace.publication_pr_number)
        .ok_or_else(|| {
            AppError::Conflict("Review PR lifecycle polling requires a linked PR".to_string())
        })?;
    if monitor.pr_number != pr_number || monitor.conversation_id != workspace.conversation_id {
        return Err(AppError::Conflict(
            "Review PR lifecycle monitor does not match its workspace".to_string(),
        ));
    }
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.to_string()))?;
    let worktree_path =
        resolve_valid_agent_conversation_workspace_path(&project, workspace).await?;
    let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
    Ok(state
        .pr_poller_registry
        .start_agent_workspace_polling_with_repair_repo_and_recovery_state(
            workspace.conversation_id.clone(),
            pr_number,
            project,
            worktree_path,
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.agent_workspace_repair_repo),
            chat_service,
            Some(Arc::new(state.clone())),
        ))
}

impl PrPollerRegistry {
    /// Maximum number of concurrent PR poll tasks. (AD9: default 10)
    const MAX_CONCURRENT_POLLS: usize = 10;

    /// Create a new registry. In production, `github_service` is `Some(GhCliGithubService)`.
    /// In tests, `github_service` is `None` (no real `gh` calls).
    pub fn new(
        github_service: Option<Arc<dyn GithubServiceTrait>>,
        plan_branch_repo: Arc<dyn PlanBranchRepository>,
    ) -> Self {
        Self {
            active: Arc::new(DashMap::new()),
            workspace_active: Arc::new(DashMap::new()),
            stopping: Arc::new(DashMap::new()),
            workspace_stopping: Arc::new(DashMap::new()),
            pr_creation_guard: Arc::new(DashMap::new()),
            semaphore: Arc::new(tokio::sync::Semaphore::new(Self::MAX_CONCURRENT_POLLS)),
            rate_limit: Arc::new(std::sync::Mutex::new(RateLimitState::default())),
            rate_limit_last_probe: Arc::new(std::sync::Mutex::new(None)),
            pr_snapshot_hub: Arc::new(PrSnapshotHub::new()),
            github_service,
            plan_branch_repo,
            notification_service: Arc::new(std::sync::RwLock::new(None)),
            branch_update_repo: Arc::new(std::sync::RwLock::new(None)),
            branch_status_cache: BranchStatusCache::default(),
        }
    }

    pub(crate) fn branch_status_cache(&self) -> BranchStatusCache {
        self.branch_status_cache.clone()
    }

    /// Read-only view of the shared GitHub rate-limit state.
    ///
    /// Lets recovery paths outside the pollers defer work while GitHub's window is exhausted
    /// without holding the poller's lock for longer than a read. Returns `None` when the lock is
    /// poisoned, so a lock failure can never manufacture a "rate limited" verdict.
    pub fn rate_limit_snapshot(&self) -> Option<(u32, Instant)> {
        let state = self.rate_limit.lock().ok()?;
        Some((state.remaining, state.reset_at))
    }

    pub fn set_branch_update_repo(&self, repo: Arc<dyn BranchUpdateRepository>) {
        if let Ok(mut current) = self.branch_update_repo.write() {
            *current = Some(repo);
        }
    }

    pub fn set_notification_service(&self, service: Arc<NotificationService>) {
        if let Ok(mut current) = self.notification_service.write() {
            *current = Some(service);
        }
    }

    /// Legacy fixture seam. Production startup and live callers must use
    /// `start_agent_workspace_polling_with_repair_repo` so repair dispatch is attempt-owned.
    #[cfg(test)]
    pub fn start_agent_workspace_polling(
        &self,
        conversation_id: ChatConversationId,
        pr_number: i64,
        project: Project,
        working_dir: PathBuf,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        chat_service: Arc<dyn ChatService>,
    ) -> AgentWorkspacePrPollerStart {
        self.start_agent_workspace_polling_with_optional_repair_repo(
            conversation_id,
            pr_number,
            project,
            working_dir,
            workspace_repo,
            agent_run_repo,
            None,
            chat_service,
            None,
        )
    }

    pub fn start_agent_workspace_polling_with_repair_repo(
        &self,
        conversation_id: ChatConversationId,
        pr_number: i64,
        project: Project,
        working_dir: PathBuf,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
        chat_service: Arc<dyn ChatService>,
    ) -> AgentWorkspacePrPollerStart {
        self.start_agent_workspace_polling_with_repair_repo_and_recovery_state(
            conversation_id,
            pr_number,
            project,
            working_dir,
            workspace_repo,
            agent_run_repo,
            repair_repo,
            chat_service,
            None,
        )
    }

    pub(crate) fn start_agent_workspace_polling_with_repair_repo_and_recovery_state(
        &self,
        conversation_id: ChatConversationId,
        pr_number: i64,
        project: Project,
        working_dir: PathBuf,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
        chat_service: Arc<dyn ChatService>,
        recovery_state: Option<Arc<AppState>>,
    ) -> AgentWorkspacePrPollerStart {
        self.start_agent_workspace_polling_with_optional_repair_repo(
            conversation_id,
            pr_number,
            project,
            working_dir,
            workspace_repo,
            agent_run_repo,
            Some(repair_repo),
            chat_service,
            recovery_state,
        )
    }

    fn start_agent_workspace_polling_with_optional_repair_repo(
        &self,
        conversation_id: ChatConversationId,
        pr_number: i64,
        project: Project,
        working_dir: PathBuf,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
        chat_service: Arc<dyn ChatService>,
        recovery_state: Option<Arc<AppState>>,
    ) -> AgentWorkspacePrPollerStart {
        use dashmap::mapref::entry::Entry;

        if let Some(handle) = self.workspace_active.get(&conversation_id) {
            if !handle.is_finished() {
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    "start_agent_workspace_polling: already polling, skipping"
                );
                return AgentWorkspacePrPollerStart::AlreadyRunning;
            }
        }
        self.workspace_active.remove(&conversation_id);

        let Some(github) = self.github_service.as_ref().map(Arc::clone) else {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                "start_agent_workspace_polling: github_service is None — skipping"
            );
            return AgentWorkspacePrPollerStart::Unavailable;
        };

        self.workspace_stopping.remove(&conversation_id);

        let active = Arc::clone(&self.workspace_active);
        let stopping = Arc::clone(&self.workspace_stopping);
        let semaphore = Arc::clone(&self.semaphore);
        let notification_service = self
            .notification_service
            .read()
            .ok()
            .and_then(|service| service.clone());
        let plan_branch_repo = Arc::clone(&self.plan_branch_repo);
        let branch_update_repo = self
            .branch_update_repo
            .read()
            .ok()
            .and_then(|repo| repo.clone());
        let branch_status_cache = self.branch_status_cache.clone();
        let rate_limit = Arc::clone(&self.rate_limit);
        let rate_limit_last_probe = Arc::clone(&self.rate_limit_last_probe);
        let pr_snapshot_hub = Arc::clone(&self.pr_snapshot_hub);
        let conversation_id_for_spawn = conversation_id.clone();

        let handle = tokio::spawn(async move {
            agent_workspace_poll_loop(
                conversation_id_for_spawn,
                pr_number,
                project,
                working_dir,
                github,
                active,
                stopping,
                semaphore,
                rate_limit,
                rate_limit_last_probe,
                pr_snapshot_hub,
                workspace_repo,
                agent_run_repo,
                repair_repo,
                branch_update_repo,
                plan_branch_repo,
                chat_service,
                notification_service,
                recovery_state,
                branch_status_cache,
            )
            .await;
        });

        match self.workspace_active.entry(conversation_id) {
            Entry::Vacant(vacant) => {
                vacant.insert(handle);
                AgentWorkspacePrPollerStart::Started
            }
            Entry::Occupied(_) => {
                handle.abort();
                AgentWorkspacePrPollerStart::AlreadyRunning
            }
        }
    }

    pub fn stop_agent_workspace_polling(&self, conversation_id: &ChatConversationId) {
        self.workspace_stopping.insert(conversation_id.clone(), ());

        if let Some((_, handle)) = self.workspace_active.remove(conversation_id) {
            handle.abort();
        }
    }

    pub fn is_agent_workspace_polling(&self, conversation_id: &ChatConversationId) -> bool {
        self.workspace_active
            .get(conversation_id)
            .map(|handle| !handle.is_finished())
            .unwrap_or(false)
    }

    // ────────────────────────────────────────────────────────────────
    // Public API
    // ────────────────────────────────────────────────────────────────

    /// Begin polling the GitHub PR for a task.
    ///
    /// Idempotent — no-op if already polling. Atomically checks and inserts
    /// via `DashMap::entry()` to prevent duplicate pollers from concurrent callers
    /// (reconciler restart + PendingMerge re-entry race).
    ///
    /// Staggered start: adds `rand(1..=30s)` jitter so pollers don't thunderherd
    /// on startup batch. (AD9)
    pub fn start_polling(
        &self,
        task_id: TaskId,
        plan_branch_id: PlanBranchId,
        pr_number: i64,
        working_dir: PathBuf,
        base_branch: String,
        transition_service: Arc<TaskTransitionService>,
    ) {
        use dashmap::mapref::entry::Entry;

        // Check for existing live poller — idempotent if already running
        if let Some(h) = self.active.get(&task_id) {
            if !h.is_finished() {
                tracing::debug!(
                    task_id = task_id.as_str(),
                    "start_polling: already polling, skipping"
                );
                return;
            }
        }
        // Remove stale finished handle (if any) before inserting new one
        self.active.remove(&task_id);

        let Some(github) = self.github_service.as_ref().map(Arc::clone) else {
            tracing::warn!(
                task_id = task_id.as_str(),
                "start_polling: github_service is None — skipping"
            );
            return;
        };

        // Clone Arcs needed by the background task
        let active = Arc::clone(&self.active);
        let stopping = Arc::clone(&self.stopping);
        let semaphore = Arc::clone(&self.semaphore);
        let rate_limit = Arc::clone(&self.rate_limit);
        let rate_limit_last_probe = Arc::clone(&self.rate_limit_last_probe);
        let plan_branch_repo = Arc::clone(&self.plan_branch_repo);

        // Staggered start jitter (AD9): rand(1..=30s)
        let jitter_secs: u64 = {
            use rand::Rng;
            rand::thread_rng().gen_range(1..=30)
        };

        // Clone task_id for the spawned closure (original used for DashMap entry insert)
        let task_id_for_spawn = task_id.clone();

        let handle = tokio::spawn(async move {
            if jitter_secs > 0 {
                tokio::time::sleep(Duration::from_secs(jitter_secs)).await;
            }
            poll_loop(
                task_id_for_spawn,
                plan_branch_id,
                pr_number,
                working_dir,
                base_branch,
                github,
                active,
                stopping,
                semaphore,
                rate_limit,
                rate_limit_last_probe,
                plan_branch_repo,
                transition_service,
            )
            .await;
        });

        // Insert via entry — if another caller won the race, abort our duplicate
        match self.active.entry(task_id) {
            Entry::Vacant(vacant) => {
                vacant.insert(handle);
            }
            Entry::Occupied(_) => {
                // Another caller won — abort our duplicate poller
                handle.abort();
            }
        }
    }

    /// Cancel polling for a task.
    ///
    /// Called on task stop/cancel/re-execution/cascade_stop. (AD11)
    /// Inserts into `stopping` BEFORE abort so poll_loop skips transition on exit.
    pub fn stop_polling(&self, task_id: &TaskId) {
        // Set stopping flag BEFORE abort to prevent post-cleanup transitions (AD11)
        self.stopping.insert(task_id.clone(), ());

        if let Some((_, handle)) = self.active.remove(task_id) {
            handle.abort();
        }

        // NOTE: Do NOT remove from `stopping` here. abort() is non-blocking —
        // the tokio task may still be executing between awaits. The poll_loop's
        // own cleanup path removes from `stopping` on ALL exit branches.
        // Orphaned `stopping` entries (for pollers killed mid-flight) are cleaned
        // up by the reconciler periodic scan.

        // Fire-and-forget DB cleanup: clear pr_polling_active to prevent reconciler
        // from restarting a stopped poller.
        let repo = Arc::clone(&self.plan_branch_repo);
        let tid = task_id.clone();
        tokio::spawn(async move {
            if let Err(e) = repo.clear_polling_active_by_task(&tid).await {
                tracing::warn!(
                    task_id = tid.as_str(),
                    error = %e,
                    "stop_polling: failed to clear pr_polling_active"
                );
            }
        });
    }

    /// Returns true if there is a live (not finished) poll task for this task.
    ///
    /// `is_finished()` returns false even when blocked on semaphore, so semaphore-
    /// blocked pollers correctly appear as "polling". (AD9: reconciler safety)
    pub fn is_polling(&self, task_id: &TaskId) -> bool {
        self.active
            .get(task_id)
            .map(|h| !h.is_finished())
            .unwrap_or(false)
    }

    /// Poll GitHub once for requested-changes review feedback and route it into
    /// normal RalphX plan correction work.
    pub async fn process_review_feedback_once(
        &self,
        task_id: &TaskId,
        pr_number: i64,
        working_dir: &Path,
        transition_service: Arc<TaskTransitionService>,
        history_actor: &str,
    ) -> crate::AppResult<bool> {
        let Some(github) = self.github_service.as_ref() else {
            return Ok(false);
        };

        route_review_feedback_if_present(
            Arc::clone(github),
            working_dir,
            pr_number,
            task_id,
            transition_service,
            history_actor,
        )
        .await
    }

    #[cfg(test)]
    pub async fn process_agent_workspace_review_feedback_once(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        working_dir: &Path,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        chat_service: Arc<dyn ChatService>,
    ) -> crate::AppResult<bool> {
        let Some(github) = self.github_service.as_ref() else {
            return Ok(false);
        };

        route_agent_workspace_review_feedback_if_present(
            Arc::clone(github),
            working_dir,
            pr_number,
            conversation_id,
            workspace_repo,
            Some(agent_run_repo),
            chat_service,
        )
        .await
    }

    pub async fn process_agent_workspace_review_feedback_once_with_repair_repo(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        working_dir: &Path,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
        chat_service: Arc<dyn ChatService>,
    ) -> crate::AppResult<bool> {
        let Some(github) = self.github_service.as_ref() else {
            return Ok(false);
        };

        route_agent_workspace_review_feedback_if_present_with_repair_repo(
            Arc::clone(github),
            working_dir,
            pr_number,
            conversation_id,
            workspace_repo,
            Some(agent_run_repo),
            Some(repair_repo),
            self.branch_update_repo
                .read()
                .ok()
                .and_then(|repo| repo.clone()),
            chat_service,
            None,
        )
        .await
    }

    pub async fn check_agent_workspace_pr_status_once(
        &self,
        working_dir: &Path,
        pr_number: i64,
    ) -> crate::AppResult<Option<PrStatus>> {
        let Some(github) = self.github_service.as_ref() else {
            return Ok(None);
        };
        github
            .check_pr_status(working_dir, pr_number)
            .await
            .map(Some)
    }
}

// ────────────────────────────────────────────────────────────────────
// Poll loop (free async fn — all args owned, 'static safe for spawn)
// ────────────────────────────────────────────────────────────────────

/// Long-running poll loop for a single PR. Runs until the PR is Merged/Closed
/// or a terminal error threshold is reached. Implements:
///
/// - Adaptive intervals: age-based floor (60s/120s/300s) + error backoff cap at 600s (AD9)
/// - Semaphore concurrency: acquire before gh call, release after (AD9)
/// - Rate limit awareness: double interval at <500 remaining, sleep at <100 (AD9)
/// - Stopping guard: checks `stopping` set before any transition (AD11)
/// - 7-day stale guard: MergeIncomplete if no status change for 7 days (AD8)
/// - 10-error threshold: MergeIncomplete after 10 consecutive errors
async fn poll_loop(
    task_id: TaskId,
    plan_branch_id: PlanBranchId,
    pr_number: i64,
    working_dir: PathBuf,
    base_branch: String,
    github: Arc<dyn GithubServiceTrait>,
    active: Arc<DashMap<TaskId, JoinHandle<()>>>,
    stopping: Arc<DashMap<TaskId, ()>>,
    semaphore: Arc<tokio::sync::Semaphore>,
    rate_limit: Arc<std::sync::Mutex<RateLimitState>>,
    rate_limit_last_probe: Arc<std::sync::Mutex<Option<Instant>>>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    transition_service: Arc<TaskTransitionService>,
) {
    let start_time = Instant::now();
    let max_backoff = Duration::from_secs(600); // 10 min cap
    let stale_threshold = Duration::from_secs(7 * 24 * 3600); // 7 days

    let mut consecutive_errors = 0u32;
    let mut last_status_change_at = Instant::now();
    let mut first_poll = true;

    // age-based interval floor (AD9)
    let age_floor = |elapsed: Duration| -> Duration {
        if elapsed < Duration::from_secs(3600) {
            Duration::from_secs(60)
        } else if elapsed < Duration::from_secs(86400) {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(300)
        }
    };

    let mut interval = age_floor(start_time.elapsed());

    loop {
        if first_poll {
            first_poll = false;
        } else {
            tokio::time::sleep(interval).await;
        }

        // 7-day stale guard (AD8)
        if last_status_change_at.elapsed() >= stale_threshold {
            tracing::warn!(
                task_id = task_id.as_str(),
                "PR poller: no status change in 7 days — transitioning to MergeIncomplete"
            );
            if !stopping.contains_key(&task_id) {
                let _ = transition_service
                    .transition_task(&task_id, InternalStatus::MergeIncomplete)
                    .await;
            }
            active.remove(&task_id);
            stopping.remove(&task_id);
            return;
        }

        // Check stopping guard before poll (AD11 race prevention)
        if stopping.contains_key(&task_id) {
            active.remove(&task_id);
            stopping.remove(&task_id);
            return;
        }

        maybe_refresh_rate_limit(&github, &working_dir, &rate_limit, &rate_limit_last_probe).await;

        // Apply rate limit pressure — extract values before any await (no guard across await)
        let (should_sleep_until_reset, sleep_duration, is_low_remaining) = {
            let rl = rate_limit.lock().unwrap_or_else(|e| e.into_inner());
            let should_sleep = rl.remaining < 100;
            let sleep_dur = if should_sleep {
                rl.reset_at.saturating_duration_since(Instant::now())
            } else {
                Duration::ZERO
            };
            let low = rl.remaining < 500;
            (should_sleep, sleep_dur, low)
            // MutexGuard dropped here — safe to await after this block
        };

        if should_sleep_until_reset && !sleep_duration.is_zero() {
            tracing::warn!(
                task_id = task_id.as_str(),
                sleep_secs = sleep_duration.as_secs(),
                "Rate limit critically low (<100) — sleeping until reset"
            );
            tokio::time::sleep(sleep_duration).await;
        }

        // Acquire semaphore slot before making gh API call (AD9: concurrency control)
        let _permit = match semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => {
                // Semaphore closed — registry is shutting down
                active.remove(&task_id);
                stopping.remove(&task_id);
                return;
            }
        };

        // Check stopping guard again after potentially long semaphore wait
        if stopping.contains_key(&task_id) {
            active.remove(&task_id);
            stopping.remove(&task_id);
            return;
        }

        match github.check_pr_status(&working_dir, pr_number).await {
            Ok(PrStatus::Merged {
                merge_commit_sha, ..
            }) => {
                // Release semaphore before potentially-long fetch operation
                drop(_permit);
                clear_task_auto_merge_correction_marker_for_terminal_pr(
                    Arc::clone(&transition_service),
                    &task_id,
                    "merged",
                )
                .await;

                // Check stopping guard BEFORE transition (AD11 critical section)
                if stopping.contains_key(&task_id) {
                    active.remove(&task_id);
                    stopping.remove(&task_id);
                    return;
                }

                // AD17: Fetch remote + verify ancestry before transitioning
                match github.fetch_remote(&working_dir, &base_branch).await {
                    Ok(()) => {
                        // Store merge_commit_sha on PlanBranch for complete_merge_internal
                        if let Some(sha) = merge_commit_sha {
                            if let Err(e) = plan_branch_repo
                                .set_merge_commit_sha(&plan_branch_id, sha)
                                .await
                            {
                                tracing::warn!(
                                    task_id = task_id.as_str(),
                                    error = %e,
                                    "Failed to store merge_commit_sha"
                                );
                            }
                        }

                        let now = chrono::Utc::now();
                        let _ = plan_branch_repo
                            .update_last_polled_at(&plan_branch_id, now)
                            .await;
                        let _ = plan_branch_repo
                            .update_pr_status(&plan_branch_id, DbPrStatus::Merged)
                            .await;
                        let _ = plan_branch_repo
                            .clear_polling_active_by_task(&task_id)
                            .await;

                        // Final stopping check before transition
                        if stopping.contains_key(&task_id) {
                            active.remove(&task_id);
                            stopping.remove(&task_id);
                            return;
                        }

                        // Merging → Merged: on_enter(Merged) runs post_merge_cleanup (AD20)
                        if let Err(e) = transition_service
                            .transition_task(&task_id, InternalStatus::Merged)
                            .await
                        {
                            tracing::error!(
                                task_id = task_id.as_str(),
                                error = %e,
                                "Failed to transition task to Merged"
                            );
                        }
                        active.remove(&task_id);
                        stopping.remove(&task_id);
                        return;
                    }
                    Err(e) => {
                        // Don't transition yet — PR is still merged, retry next poll
                        consecutive_errors += 1;
                        let backoff = Duration::from_secs(60 * 2u64.pow(consecutive_errors.min(4)))
                            .min(max_backoff);
                        interval = backoff.max(age_floor(start_time.elapsed()));
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            error = %e,
                            consecutive_errors,
                            retry_secs = interval.as_secs(),
                            "git fetch failed for merged PR (will retry)"
                        );

                        if consecutive_errors >= 10 {
                            tracing::error!(
                                task_id = task_id.as_str(),
                                "10 consecutive fetch failures — transitioning to MergeIncomplete"
                            );
                            if !stopping.contains_key(&task_id) {
                                let _ = transition_service
                                    .transition_task(&task_id, InternalStatus::MergeIncomplete)
                                    .await;
                            }
                            active.remove(&task_id);
                            stopping.remove(&task_id);
                            return;
                        }
                    }
                }
            }

            Ok(PrStatus::Closed) => {
                drop(_permit);
                tracing::info!(
                    task_id = task_id.as_str(),
                    "PR closed without merging — transitioning to MergeIncomplete"
                );
                clear_task_auto_merge_correction_marker_for_terminal_pr(
                    Arc::clone(&transition_service),
                    &task_id,
                    "closed",
                )
                .await;
                let _ = plan_branch_repo
                    .update_pr_status(&plan_branch_id, DbPrStatus::Closed)
                    .await;
                let _ = plan_branch_repo
                    .clear_polling_active_by_task(&task_id)
                    .await;
                if !stopping.contains_key(&task_id) {
                    let _ = transition_service
                        .transition_task(&task_id, InternalStatus::MergeIncomplete)
                        .await;
                }
                active.remove(&task_id);
                stopping.remove(&task_id);
                return;
            }

            Ok(PrStatus::Open) => {
                drop(_permit);

                // Detect status change for stale guard reset — read from DB
                let prev_db_status = plan_branch_repo
                    .get_by_merge_task_id(&task_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|pb| pb.pr_status);

                if prev_db_status != Some(DbPrStatus::Open) {
                    last_status_change_at = Instant::now();
                    tracing::info!(task_id = task_id.as_str(), "PR status changed to Open");
                }

                // Update pr_status in DB for UI and update last_polled_at
                let _ = plan_branch_repo
                    .update_pr_status(&plan_branch_id, DbPrStatus::Open)
                    .await;

                let now = chrono::Utc::now();
                let _ = plan_branch_repo
                    .update_last_polled_at(&plan_branch_id, now)
                    .await;

                match route_review_feedback_if_present(
                    Arc::clone(&github),
                    &working_dir,
                    pr_number,
                    &task_id,
                    Arc::clone(&transition_service),
                    "github_pr_review",
                )
                .await
                {
                    Ok(true) => {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            pr_number,
                            "PR poller: GitHub requested changes routed to plan correction task"
                        );
                        active.remove(&task_id);
                        stopping.remove(&task_id);
                        return;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            pr_number,
                            error = %error,
                            "PR poller: failed to inspect GitHub review feedback"
                        );
                    }
                }

                match transition_service
                    .route_plan_pr_autofix_if_needed(&plan_branch_id, pr_number)
                    .await
                {
                    Ok(true) => {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            pr_number,
                            "PR poller: supervised plan PR issue routed to fixer agent"
                        );
                        active.remove(&task_id);
                        stopping.remove(&task_id);
                        return;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            pr_number,
                            error = %error,
                            "PR poller: failed to inspect supervised plan PR health"
                        );
                    }
                }

                match transition_service
                    .reconcile_pr_branch_freshness(
                        &task_id,
                        &plan_branch_id,
                        pr_number,
                        "pr_poller",
                    )
                    .await
                {
                    Ok(PrBranchFreshnessOutcome::ConflictRouted) => {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            pr_number,
                            "PR poller: routed stale PR branch conflict to merger agent"
                        );
                        active.remove(&task_id);
                        stopping.remove(&task_id);
                        return;
                    }
                    Ok(PrBranchFreshnessOutcome::Updated) => {
                        tracing::info!(
                            task_id = task_id.as_str(),
                            pr_number,
                            "PR poller: updated stale PR branch from base branch"
                        );
                    }
                    Ok(
                        PrBranchFreshnessOutcome::NotApplicable
                        | PrBranchFreshnessOutcome::UpToDate,
                    ) => {}
                    Err(error) => {
                        tracing::warn!(
                            task_id = task_id.as_str(),
                            pr_number,
                            error = %error,
                            "PR poller: failed to reconcile PR branch freshness"
                        );
                    }
                }

                // Reset error count and return to age-based floor (AD9)
                consecutive_errors = 0;
                interval = age_floor(start_time.elapsed());

                // Apply rate limit pressure on the interval
                let is_low = {
                    let rl = rate_limit.lock().unwrap_or_else(|e| e.into_inner());
                    rl.remaining < 500
                };
                if is_low {
                    interval = (interval * 2).min(max_backoff);
                }
            }

            Err(e) => {
                drop(_permit);
                consecutive_errors += 1;

                // Exponential backoff: 60s → 120s → 240s → 480s → cap at 600s
                // Floor: age-based interval (error backoff only increases above floor, AD9)
                let backoff =
                    Duration::from_secs(60 * 2u64.pow(consecutive_errors.min(4))).min(max_backoff);
                interval = backoff.max(age_floor(start_time.elapsed()));

                // Apply rate limit pressure on error backoff too (AD9)
                if is_low_remaining {
                    interval = (interval * 2).min(max_backoff);
                }

                tracing::warn!(
                    task_id = task_id.as_str(),
                    error = %e,
                    consecutive_errors,
                    retry_secs = interval.as_secs(),
                    "PR poll error (exponential backoff)"
                );

                if consecutive_errors >= 10 {
                    tracing::error!(
                        task_id = task_id.as_str(),
                        "10 consecutive PR poll errors — transitioning to MergeIncomplete"
                    );
                    if !stopping.contains_key(&task_id) {
                        let _ = transition_service
                            .transition_task(&task_id, InternalStatus::MergeIncomplete)
                            .await;
                    }
                    active.remove(&task_id);
                    stopping.remove(&task_id);
                    return;
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn agent_workspace_poll_loop(
    conversation_id: ChatConversationId,
    pr_number: i64,
    project: Project,
    working_dir: PathBuf,
    github: Arc<dyn GithubServiceTrait>,
    active: Arc<DashMap<ChatConversationId, JoinHandle<()>>>,
    stopping: Arc<DashMap<ChatConversationId, ()>>,
    semaphore: Arc<tokio::sync::Semaphore>,
    rate_limit: Arc<std::sync::Mutex<RateLimitState>>,
    rate_limit_last_probe: Arc<std::sync::Mutex<Option<Instant>>>,
    pr_snapshot_hub: Arc<PrSnapshotHub>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    chat_service: Arc<dyn ChatService>,
    notification_service: Option<Arc<NotificationService>>,
    recovery_state: Option<Arc<AppState>>,
    branch_status_cache: BranchStatusCache,
) {
    let config = git_runtime_config();
    let base_interval = Duration::from_secs(config.workspace_pr_poll_base_secs.max(1));
    let max_interval = Duration::from_secs(
        config
            .workspace_pr_poll_max_secs
            .max(config.workspace_pr_poll_base_secs.max(1)),
    );
    // Adaptive cadence: an idle PR doubles its interval up to the cap, and any observable change
    // snaps it straight back to base. Terminalization is unaffected beyond detection latency,
    // which is bounded by `max_interval`.
    let mut interval = base_interval;
    let mut previous_health: Option<PrHealth> = None;
    let mut first_poll = true;

    // The hub batches every registered PR for this repository into one GitHub read per TTL
    // window. `_hub_registration` withdraws this PR on every exit path — including the several
    // early `return`s below and an aborted task — so a stopped poller never keeps inflating
    // other pollers' batches.
    let repo_key = project.working_directory.clone();
    let _hub_registration =
        PrSnapshotRegistration::new(Arc::clone(&pr_snapshot_hub), repo_key.clone(), pr_number);

    loop {
        if first_poll {
            first_poll = false;
        } else {
            tokio::time::sleep(interval).await;
        }

        if stopping.contains_key(&conversation_id) {
            active.remove(&conversation_id);
            stopping.remove(&conversation_id);
            return;
        }

        maybe_refresh_rate_limit(&github, &working_dir, &rate_limit, &rate_limit_last_probe).await;

        // Pressure is applied before the iteration's GitHub reads, mirroring the task loop: when
        // fewer than 100 calls remain there is no point spending one to discover we are limited.
        let (pressured_interval, sleep_until_reset) =
            apply_rate_limit_pressure(&rate_limit, interval, max_interval);
        interval = pressured_interval;
        if !sleep_until_reset.is_zero() {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                sleep_secs = sleep_until_reset.as_secs(),
                "Agent workspace PR poller: GitHub rate limit critically low — sleeping until reset"
            );
            tokio::time::sleep(sleep_until_reset).await;
        }

        if stopping.contains_key(&conversation_id) {
            active.remove(&conversation_id);
            stopping.remove(&conversation_id);
            return;
        }

        #[cfg(not(test))]
        if repair_repo.is_none() {
            tracing::error!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                "Agent workspace PR poller: durable repair repository is unavailable"
            );
            active.remove(&conversation_id);
            stopping.remove(&conversation_id);
            return;
        }

        if !agent_workspace_pr_polling_should_continue(
            Arc::clone(&workspace_repo),
            &conversation_id,
            pr_number,
        )
        .await
        {
            active.remove(&conversation_id);
            stopping.remove(&conversation_id);
            return;
        }

        let permit = match semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => {
                active.remove(&conversation_id);
                stopping.remove(&conversation_id);
                return;
            }
        };

        if stopping.contains_key(&conversation_id) {
            active.remove(&conversation_id);
            stopping.remove(&conversation_id);
            return;
        }

        // One hub read serves the terminal-status check and the health branches below; every
        // other workspace polling this repository in the same window is served from the same
        // batched response.
        let polled_snapshot = pr_snapshot_hub
            .get_snapshot(&repo_key, pr_number, &github, &working_dir)
            .await;
        match polled_snapshot
            .as_ref()
            .map(|snapshot| snapshot.sync_state.status.clone())
        {
            Ok(PrStatus::Merged { .. }) => {
                drop(permit);
                terminalize_polled_agent_workspace_with_notifications(
                    &workspace_repo,
                    &agent_run_repo,
                    &plan_branch_repo,
                    &chat_service,
                    &stopping,
                    &conversation_id,
                    &project,
                    pr_number,
                    TerminalAgentWorkspaceCause::MergedPr,
                    "merged",
                    "Pull request merged",
                    // Local retry backoff for cleanup/persistence failures — deliberately the
                    // fixed base, not the adaptive GitHub cadence.
                    base_interval,
                    notification_service.as_ref(),
                )
                .await;
                active.remove(&conversation_id);
                stopping.remove(&conversation_id);
                return;
            }
            Ok(PrStatus::Closed) => {
                drop(permit);
                terminalize_polled_agent_workspace_with_notifications(
                    &workspace_repo,
                    &agent_run_repo,
                    &plan_branch_repo,
                    &chat_service,
                    &stopping,
                    &conversation_id,
                    &project,
                    pr_number,
                    TerminalAgentWorkspaceCause::ClosedPr,
                    "closed",
                    "Pull request closed without merging",
                    base_interval,
                    notification_service.as_ref(),
                )
                .await;
                active.remove(&conversation_id);
                stopping.remove(&conversation_id);
                return;
            }
            Ok(PrStatus::Open) => {
                drop(permit);
                match mark_agent_workspace_pr_open(
                    Arc::clone(&workspace_repo),
                    &conversation_id,
                    pr_number,
                )
                .await
                {
                    Ok(false) => {
                        active.remove(&conversation_id);
                        stopping.remove(&conversation_id);
                        return;
                    }
                    Ok(true) => {}
                    Err(error) => {
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR poller: failed to mark PR open"
                        );
                    }
                }

                // One health value serves every branch below in this iteration. Its view half
                // comes from the shared batch; only the comments are read per PR, on the
                // response-cached REST path.
                let mut polled_health: Option<PrHealth> = None;
                let health_result = match polled_snapshot.as_ref() {
                    Ok(snapshot) => github
                        .fetch_pr_issue_comments(&working_dir, pr_number)
                        .await
                        .map(|comments| {
                            PrHealth::from_snapshot_and_comments(snapshot.clone(), comments)
                        }),
                    Err(error) => Err(AppError::Infrastructure(error.to_string())),
                };
                match health_result {
                    Ok(health) => {
                        branch_status_cache.observe_pr_sync(
                            &working_dir,
                            &health.sync_state,
                            chrono::Utc::now(),
                        );
                        if let Err(error) = import_agent_workspace_pr_comment_evidence(
                            Arc::clone(&workspace_repo),
                            &conversation_id,
                            pr_number,
                            &health,
                        )
                        .await
                        {
                            tracing::warn!(
                                conversation_id = conversation_id.as_str(),
                                pr_number,
                                error = %error,
                                "Agent workspace PR poller: failed to import PR comment evidence"
                            );
                        }

                        if let Some(recovery_state) = recovery_state.as_deref() {
                            match re_drive_held_unpublished_agent_workspace_repair(
                                recovery_state,
                                &workspace_repo,
                                &conversation_id,
                                &health,
                            )
                            .await
                            {
                                Ok(true) => {
                                    tracing::info!(
                                        conversation_id = conversation_id.as_str(),
                                        pr_number,
                                        "Agent workspace PR poller re-drove the existing unpublished repair continuation"
                                    );
                                    continue;
                                }
                                Ok(false) => {}
                                Err(error) => {
                                    tracing::warn!(
                                        conversation_id = conversation_id.as_str(),
                                        pr_number,
                                        error = %error,
                                        "Agent workspace PR poller refused unpublished repair re-drive after an authority read failed"
                                    );
                                    continue;
                                }
                            }

                            match re_arm_escalated_open_effect_continuation(
                                recovery_state,
                                &workspace_repo,
                                &conversation_id,
                                pr_number,
                                &health,
                            )
                            .await
                            {
                                Ok(true) => {
                                    tracing::info!(
                                        conversation_id = conversation_id.as_str(),
                                        pr_number,
                                        "Agent workspace PR poller re-armed an escalated open-effect continuation with new evidence"
                                    );
                                    continue;
                                }
                                Ok(false) => {}
                                Err(error) => {
                                    tracing::warn!(
                                        conversation_id = conversation_id.as_str(),
                                        pr_number,
                                        error = %error,
                                        "Agent workspace PR poller failed to re-arm an escalated open-effect continuation"
                                    );
                                    continue;
                                }
                            }
                        }

                        if repair_repo.is_none() {
                            if let Err(error) = mark_agent_workspace_pr_merge_conflict_if_needed(
                                pr_number,
                                &health,
                                &conversation_id,
                                Arc::clone(&workspace_repo),
                            )
                            .await
                            {
                                tracing::warn!(
                                    conversation_id = conversation_id.as_str(),
                                    pr_number,
                                    error = %error,
                                    "Agent workspace PR poller: failed to mark PR merge conflict state"
                                );
                            }
                        }

                        match route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
                            Arc::clone(&github),
                            &working_dir,
                            pr_number,
                            &health,
                            &conversation_id,
                            Arc::clone(&workspace_repo),
                            Some(Arc::clone(&agent_run_repo)),
                            repair_repo.as_ref().map(Arc::clone),
                            branch_update_repo.as_ref().map(Arc::clone),
                            Arc::clone(&chat_service),
                        )
                        .await
                        {
                            Ok(true) => {
                                tracing::info!(
                                    conversation_id = conversation_id.as_str(),
                                    pr_number,
                                    "Agent workspace PR poller: GitHub PR conflict routed to workspace repair agent"
                                );
                                active.remove(&conversation_id);
                                stopping.remove(&conversation_id);
                                return;
                            }
                            Ok(false) => {}
                            Err(error) => {
                                tracing::warn!(
                                    conversation_id = conversation_id.as_str(),
                                    pr_number,
                                    error = %error,
                                    "Agent workspace PR poller: failed to route GitHub PR conflict repair"
                                );
                            }
                        }

                        polled_health = Some(health);
                    }
                    Err(error) => {
                        if matches!(error, AppError::GithubRateLimited { .. }) {
                            note_rate_limited(&rate_limit);
                        }
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR poller: failed to inspect PR mergeability"
                        );
                    }
                }

                // Any observable change resets the cadence; a fully idle iteration lets it grow.
                // `PrHealth` is `PartialEq`, so this covers head SHA, checks, comments, review
                // decision, and auto-merge state without a hand-rolled fingerprint that could
                // silently miss a field.
                let mut observed_activity = polled_health != previous_health;
                previous_health = polled_health.clone();

                match route_agent_workspace_pr_autofix_if_needed_with_notifications(
                    Arc::clone(&github),
                    &working_dir,
                    pr_number,
                    &conversation_id,
                    Arc::clone(&workspace_repo),
                    Some(Arc::clone(&agent_run_repo)),
                    repair_repo.as_ref().map(Arc::clone),
                    branch_update_repo.as_ref().map(Arc::clone),
                    Arc::clone(&chat_service),
                    notification_service.as_ref().map(Arc::clone),
                    Some(&project),
                    polled_health.as_ref(),
                )
                .await
                {
                    Ok(true) => {
                        tracing::info!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            "Agent workspace PR poller: supervised PR issue routed to fixer agent"
                        );
                        active.remove(&conversation_id);
                        stopping.remove(&conversation_id);
                        return;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR poller: failed to inspect supervised PR health"
                        );
                    }
                }

                match route_agent_workspace_pr_review_monitor_if_needed_with_notifications(
                    Arc::clone(&github),
                    &working_dir,
                    pr_number,
                    &conversation_id,
                    Arc::clone(&workspace_repo),
                    Arc::clone(&agent_run_repo),
                    Arc::clone(&chat_service),
                    notification_service.clone(),
                    polled_health.as_ref(),
                )
                .await
                {
                    Ok(true) => {
                        tracing::info!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            "Agent workspace PR poller: Review PR monitor routed new head to review agent"
                        );
                        observed_activity = true;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        // A branch that could not complete is a reason to look again soon, not a
                        // reason to slow down.
                        observed_activity = true;
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR poller: failed to inspect Review PR monitor state"
                        );
                    }
                }

                match route_agent_workspace_review_feedback_if_present_with_repair_repo(
                    Arc::clone(&github),
                    &working_dir,
                    pr_number,
                    &conversation_id,
                    Arc::clone(&workspace_repo),
                    Some(Arc::clone(&agent_run_repo)),
                    repair_repo.as_ref().map(Arc::clone),
                    branch_update_repo.as_ref().map(Arc::clone),
                    Arc::clone(&chat_service),
                    polled_health.as_ref(),
                )
                .await
                {
                    Ok(true) => {
                        tracing::info!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            "Agent workspace PR poller: GitHub requested changes routed to workspace agent"
                        );
                        active.remove(&conversation_id);
                        stopping.remove(&conversation_id);
                        return;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        observed_activity = true;
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR poller: failed to inspect GitHub review feedback"
                        );
                    }
                }

                interval = if observed_activity {
                    base_interval
                } else {
                    (interval * 2).clamp(base_interval, max_interval)
                };
            }
            Err(error) => {
                drop(permit);
                if matches!(error, AppError::GithubRateLimited { .. }) {
                    note_rate_limited(&rate_limit);
                }
                // An unreadable PR status leaves this iteration with no evidence either way, so
                // the cadence is reset rather than escalated.
                interval = base_interval;
                previous_health = None;
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Agent workspace PR poller: failed to check PR status"
                );
            }
        }
    }
}

/// A successful PR-health read is the poller's authority to re-enter a held Ready attempt, but
/// only the durable reconciler may acquire its target lease, transition it, and invoke the
/// command-composed publisher. Missing or stale evidence deliberately leaves the hold intact.
async fn re_drive_held_unpublished_agent_workspace_repair(
    state: &AppState,
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    health: &PrHealth,
) -> crate::AppResult<bool> {
    let Some(attempt) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await?
    else {
        return Ok(false);
    };
    if attempt.phase != AgentWorkspaceRepairPhase::Ready
        || !crate::application::agent_workspace_publish_repair_state::agent_workspace_repair_is_health_held(&attempt)
        || !held_repair_has_unpublished_head(&attempt, health.sync_state.head_ref_oid.as_deref())
    {
        return Ok(false);
    }
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for unpublished repair continuation {}",
                conversation_id
            ))
        })?;
    let (_, outcome) =
        recover_stale_publish_repair_for_workspace_in_state_result(state, workspace).await?;
    Ok(!matches!(outcome, StalePublishRepairRecoveryOutcome::Noop))
}

/// Maps `PrMergeStateStatus` to a fixed evidence token. Unit variants get a stable lowercase
/// literal; `Other(payload)` is prefixed so it can never collide with a unit-variant token or with
/// another `Other` payload's token.
fn pr_merge_state_status_evidence_token(status: Option<&PrMergeStateStatus>) -> String {
    match status {
        None => "absent-merge-state".to_string(),
        Some(PrMergeStateStatus::Clean) => "clean".to_string(),
        Some(PrMergeStateStatus::Behind) => "behind".to_string(),
        Some(PrMergeStateStatus::Dirty) => "dirty".to_string(),
        Some(PrMergeStateStatus::Blocked) => "blocked".to_string(),
        Some(PrMergeStateStatus::Draft) => "draft".to_string(),
        Some(PrMergeStateStatus::Unknown) => "unknown".to_string(),
        Some(PrMergeStateStatus::Unstable) => "unstable".to_string(),
        Some(PrMergeStateStatus::HasHooks) => "has_hooks".to_string(),
        Some(PrMergeStateStatus::Other(payload)) => format!("other:{payload}"),
    }
}

/// Stable identity of what the PR looked like when a continuation escalated. Any change here is
/// new input for unattended repair; an unchanged identity must never buy another budget.
fn agent_workspace_pr_evidence_identity(
    health: &PrHealth,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) -> String {
    let head_oid = health
        .sync_state
        .head_ref_oid
        .as_deref()
        .unwrap_or("absent-head-oid");
    let base_oid = health
        .sync_state
        .base_ref_oid
        .as_deref()
        .unwrap_or("absent-base-oid");
    let merge_state =
        pr_merge_state_status_evidence_token(health.sync_state.merge_state_status.as_ref());
    let autofix_classification = classify_agent_workspace_pr_autofix_issue(pr_number, health)
        .map(|issue| issue.classification)
        .unwrap_or_else(|| "absent-autofix-classification".to_string());
    let workspace_base_commit = workspace
        .base_commit
        .as_deref()
        .unwrap_or("absent-workspace-base-commit");

    let mut hasher = Sha256::new();
    hasher.update(head_oid);
    hasher.update(b"\0");
    hasher.update(base_oid);
    hasher.update(b"\0");
    hasher.update(merge_state.as_str());
    hasher.update(b"\0");
    hasher.update(autofix_classification.as_str());
    hasher.update(b"\0");
    hasher.update(workspace_base_commit);
    let digest = format!("{:x}", hasher.finalize());
    digest[..16].to_string()
}

/// A fresh PR-health read is the poller's authority to re-arm an escalated continuation, but only
/// the durable reconciler may transition it. Unchanged evidence deliberately leaves the escalation
/// intact.
async fn re_arm_escalated_open_effect_continuation(
    state: &AppState,
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    health: &PrHealth,
) -> crate::AppResult<bool> {
    let Some(attempt) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await?
    else {
        return Ok(false);
    };
    if !matches!(
        attempt.phase,
        AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
    ) || !attempt
        .pending_reasons
        .iter()
        .any(|reason| reason == CONTINUATION_OPEN_EFFECT_ATTENTION_REASON)
    {
        return Ok(false);
    }
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for escalated open-effect continuation {}",
                conversation_id
            ))
        })?;
    let identity = agent_workspace_pr_evidence_identity(health, &workspace, pr_number);
    let evidence_marker = format!("{CONTINUATION_OPEN_EFFECT_EVIDENCE_REASON_PREFIX}{identity}");
    if attempt
        .pending_reasons
        .iter()
        .any(|reason| reason == &evidence_marker)
    {
        return Ok(false);
    }

    let expected_updated_at = attempt.updated_at;
    let phase = attempt.phase;
    let attempt_id = attempt.id.clone();
    let mut marked = attempt;
    marked.pending_reasons.retain(|reason| {
        reason != CONTINUATION_OPEN_EFFECT_ATTENTION_REASON
            && !reason.starts_with(CONTINUATION_OPEN_EFFECT_RECOVERY_REASON_PREFIX)
            && !reason.starts_with(CONTINUATION_OPEN_EFFECT_EVIDENCE_REASON_PREFIX)
    });
    marked.pending_reasons.push(evidence_marker);
    marked.summary = Some(format!(
        "Workspace repair open-effect continuation observed changed PR evidence ({identity}); re-arming for another reconciliation pass."
    ));
    marked.updated_at = std::cmp::max(
        chrono::Utc::now(),
        expected_updated_at + chrono::Duration::microseconds(1),
    );
    let applied = match state
        .agent_workspace_repair_repo
        .transition_repair_attempt(AgentWorkspaceRepairAttemptTransition {
            attempt: marked,
            expected_phase: phase,
            expected_updated_at,
            next_phase: phase,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        AgentWorkspaceRepairAttemptTransitionOutcome::Applied(applied) => applied,
        AgentWorkspaceRepairAttemptTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairAttemptTransitionOutcome::Missing => return Ok(false),
    };

    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            applied.conversation_id.clone(),
            CONTINUATION_OPEN_EFFECT_REARMED_STEP,
            "active",
            format!(
                "Workspace repair open-effect continuation re-armed after observing changed PR evidence ({identity})."
            ),
            Some(attempt_id.to_string()),
        ))
        .await
    {
        tracing::warn!(
            error = %error,
            attempt_id = %attempt_id,
            "Failed to append workspace repair open-effect re-arm event"
        );
    }
    state
        .notification_service()
        .resolve_workflow_notification(&format!(
            "repair_open_effect:{}:{}",
            applied.conversation_id, attempt_id
        ))
        .await;

    let (_, outcome) =
        recover_stale_publish_repair_for_workspace_in_state_result(state, workspace).await?;
    Ok(!matches!(outcome, StalePublishRepairRecoveryOutcome::Noop))
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
async fn terminalize_polled_agent_workspace(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
    chat_service: &Arc<dyn ChatService>,
    stopping: &Arc<DashMap<ChatConversationId, ()>>,
    conversation_id: &ChatConversationId,
    project: &Project,
    pr_number: i64,
    cause: TerminalAgentWorkspaceCause,
    status: &str,
    summary: &str,
    retry_interval: Duration,
) {
    terminalize_polled_agent_workspace_with_notifications(
        workspace_repo,
        agent_run_repo,
        plan_branch_repo,
        chat_service,
        stopping,
        conversation_id,
        project,
        pr_number,
        cause,
        status,
        summary,
        retry_interval,
        None,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn terminalize_polled_agent_workspace_with_notifications(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    plan_branch_repo: &Arc<dyn PlanBranchRepository>,
    chat_service: &Arc<dyn ChatService>,
    stopping: &Arc<DashMap<ChatConversationId, ()>>,
    conversation_id: &ChatConversationId,
    project: &Project,
    pr_number: i64,
    cause: TerminalAgentWorkspaceCause,
    status: &str,
    summary: &str,
    retry_interval: Duration,
    notification_service: Option<&Arc<NotificationService>>,
) {
    let review_pr_number = loop {
        match workspace_repo.get_by_conversation_id(conversation_id).await {
            Ok(Some(workspace)) if workspace.mode == AgentConversationWorkspaceMode::ReviewPr => {
                break workspace
                    .source_pull_request
                    .map(|pull_request| pull_request.number)
                    .or(workspace.publication_pr_number);
            }
            Ok(Some(workspace)) if edit_workspace_owns_pr(&workspace, pr_number) => break None,
            Ok(Some(workspace)) => {
                tracing::info!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    mode = %workspace.mode,
                    publication_pr_number = workspace.publication_pr_number,
                    "Agent workspace PR poller: terminal result no longer belongs to this workspace"
                );
                return;
            }
            Ok(None) => {
                tracing::info!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    "Agent workspace PR poller: terminal workspace row disappeared"
                );
                return;
            }
            Err(error) => tracing::error!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                retry_secs = retry_interval.as_secs(),
                "Agent workspace PR poller: failed to load terminal workspace authority; retrying"
            ),
        }
        tokio::time::sleep(retry_interval).await;
        if stopping.contains_key(conversation_id) {
            return;
        }
    };

    if let Some(pr_number) = review_pr_number {
        loop {
            match settle_review_pr_terminal_observation(
                Arc::clone(workspace_repo),
                Arc::clone(agent_run_repo),
                Some(Arc::clone(plan_branch_repo)),
                Some(Arc::clone(chat_service)),
                notification_service.cloned(),
                conversation_id,
                project,
                pr_number,
                status,
                summary,
            )
            .await
            {
                Ok(outcome) => {
                    if outcome.require_runtime_shutdown().is_ok() {
                        return;
                    }
                    tracing::error!(
                        conversation_id = conversation_id.as_str(),
                        error = outcome
                            .message
                            .as_deref()
                            .unwrap_or("terminal cleanup incomplete"),
                        retry_secs = retry_interval.as_secs(),
                        "Agent workspace PR poller: terminal runtime shutdown failed; retrying"
                    );
                }
                Err(error) => tracing::error!(
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    retry_secs = retry_interval.as_secs(),
                    "Agent workspace PR poller: terminal authority persistence failed; retrying"
                ),
            }
            tokio::time::sleep(retry_interval).await;
            if stopping.contains_key(conversation_id) {
                return;
            }
        }
    }

    loop {
        match mark_agent_workspace_pr_terminal(
            Arc::clone(workspace_repo),
            conversation_id,
            pr_number,
            status,
            summary,
        )
        .await
        {
            Ok(true) => break,
            Ok(false) => return,
            Err(error) => tracing::error!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                retry_secs = retry_interval.as_secs(),
                "Agent workspace PR poller: terminal authority persistence failed; retrying"
            ),
        }
        tokio::time::sleep(retry_interval).await;
        if stopping.contains_key(conversation_id) {
            return;
        }
    }

    loop {
        let terminalized = terminalize_agent_workspace_after_pr(
            Arc::clone(workspace_repo),
            Arc::clone(agent_run_repo),
            Some(Arc::clone(plan_branch_repo)),
            Some(Arc::clone(chat_service)),
            conversation_id,
            project,
            cause,
        )
        .await;
        if terminalized.require_runtime_shutdown().is_ok() {
            return;
        }
        tracing::error!(
            conversation_id = conversation_id.as_str(),
            error = terminalized
                .message
                .as_deref()
                .unwrap_or("terminal cleanup incomplete"),
            retry_secs = retry_interval.as_secs(),
            "Agent workspace PR poller: terminal runtime shutdown failed; retrying"
        );
        tokio::time::sleep(retry_interval).await;
        if stopping.contains_key(conversation_id) {
            return;
        }
    }
}

async fn agent_workspace_pr_polling_should_continue(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> bool {
    match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace))
            if agent_workspace_pr_polling_is_current(
                Arc::clone(&workspace_repo),
                &workspace,
                pr_number,
            )
            .await =>
        {
            true
        }
        Ok(Some(workspace)) => {
            tracing::info!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                mode = %workspace.mode,
                linked_plan_branch_id = workspace
                    .linked_plan_branch_id
                    .as_ref()
                    .map(|id| id.as_str()),
                publication_pr_number = workspace.publication_pr_number,
                publication_pr_status = workspace.publication_pr_status.as_deref(),
                publication_push_status = workspace.publication_push_status.as_deref(),
                "Agent workspace PR poller: workspace is no longer direct-pollable"
            );
            false
        }
        Ok(None) => {
            tracing::info!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                "Agent workspace PR poller: workspace row disappeared"
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Agent workspace PR poller: failed to refresh workspace ownership"
            );
            true
        }
    }
}

async fn agent_workspace_pr_polling_is_current(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) -> bool {
    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.has_terminal_publication_pr_status()
    {
        return false;
    }

    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        return agent_workspace_pr_review_monitor_is_current(
            workspace_repo,
            &workspace.conversation_id,
            pr_number,
        )
        .await;
    }

    edit_workspace_owns_pr(workspace, pr_number)
}

fn edit_workspace_owns_pr(workspace: &AgentConversationWorkspace, pr_number: i64) -> bool {
    workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_pr_number == Some(pr_number)
        && workspace.has_pr_status_pollable_push_status()
}

async fn agent_workspace_pr_review_monitor_is_current(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> bool {
    match workspace_repo.get_pr_review_monitor(conversation_id).await {
        Ok(Some(monitor)) => {
            monitor.pr_number == pr_number
                && monitor.status != AgentWorkspacePrReviewMonitorStatus::Terminal
        }
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Agent workspace PR poller: failed to refresh Review PR monitor state"
            );
            true
        }
    }
}

async fn mark_agent_workspace_pr_open(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> crate::AppResult<bool> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(false);
    };

    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        return Ok(true);
    }

    if !edit_workspace_owns_pr(&workspace, pr_number) {
        tracing::info!(
            conversation_id = conversation_id.as_str(),
            pr_number,
            mode = %workspace.mode,
            publication_pr_number = workspace.publication_pr_number,
            "Agent workspace PR poller: refusing open publication update for an unowned PR"
        );
        return Ok(false);
    }

    if workspace.has_terminal_publication_pr_status() {
        tracing::info!(
            conversation_id = conversation_id.as_str(),
            pr_number,
            publication_pr_status = workspace.publication_pr_status.as_deref(),
            "Agent workspace PR poller: refusing to reopen a terminal publication status"
        );
        return Ok(false);
    }

    if workspace.publication_pr_status.as_deref() == Some("open")
        && workspace.publication_push_status.as_deref() == Some("pushed")
    {
        return Ok(true);
    }

    workspace_repo
        .update_publication(
            conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            Some("open"),
            Some("pushed"),
        )
        .await?;
    Ok(true)
}

async fn mark_agent_workspace_pr_terminal(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    status: &str,
    summary: &str,
) -> crate::AppResult<bool> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(false);
    };

    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        let pr_number = workspace
            .source_pull_request
            .as_ref()
            .map(|pull_request| pull_request.number)
            .or(workspace.publication_pr_number)
            .ok_or_else(|| {
                AppError::Conflict(
                    "Review PR terminal settlement requires a linked pull request".to_string(),
                )
            })?;
        return workspace_repo
            .settle_pr_review_terminal(conversation_id, pr_number, status, summary)
            .await
            .map(|_| true);
    }

    if !edit_workspace_owns_pr(&workspace, pr_number) {
        tracing::info!(
            conversation_id = conversation_id.as_str(),
            pr_number,
            mode = %workspace.mode,
            publication_pr_number = workspace.publication_pr_number,
            "Agent workspace PR poller: refusing terminal publication update for an unowned PR"
        );
        return Ok(false);
    }

    workspace_repo
        .update_publication(
            conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            Some(status),
            workspace.publication_push_status.as_deref(),
        )
        .await?;
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            format!("pr_{status}"),
            "succeeded",
            summary,
            None,
        ))
        .await?;
    Ok(true)
}

async fn mark_agent_workspace_pr_merge_conflict_if_needed(
    pr_number: i64,
    health: &PrHealth,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
) -> crate::AppResult<bool> {
    let details = agent_workspace_pr_merge_conflict_details(health);
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(false);
    };

    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.linked_plan_branch_id.is_some()
        || workspace.publication_pr_number != Some(pr_number)
        || workspace.has_terminal_publication_pr_status()
    {
        return Ok(false);
    }

    if details.is_empty() {
        if workspace.pr_supervision_status.as_deref() == Some("blocked")
            && agent_workspace_summary_is_merge_conflict(
                pr_number,
                workspace.pr_supervision_summary.as_deref(),
            )
        {
            let status = if workspace.auto_publish_enabled {
                "monitoring"
            } else {
                "paused"
            };
            let summary = if workspace.auto_publish_enabled {
                "RalphX is monitoring PR health."
            } else {
                "Auto Publish is paused for this PR."
            };
            update_agent_workspace_pr_supervision_state(
                workspace_repo.as_ref(),
                None,
                conversation_id,
                workspace.pr_auto_merge_current,
                Some(status),
                Some(summary),
            )
            .await?;
            return Ok(true);
        }
        return Ok(false);
    }

    let summary = agent_workspace_pr_conflict_summary(pr_number, &details);
    let classification =
        agent_workspace_pr_conflict_event_classification(pr_number, health, &details);
    let already_recorded = workspace_repo
        .list_publication_events(conversation_id)
        .await?
        .into_iter()
        .any(|event| event.classification.as_deref() == Some(classification.as_str()));
    let already_blocked = workspace.pr_supervision_status.as_deref() == Some("blocked")
        && workspace.pr_supervision_summary.as_deref() == Some(summary.as_str());

    if !already_blocked {
        update_agent_workspace_pr_supervision_state(
            workspace_repo.as_ref(),
            None,
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("blocked"),
            Some(&summary),
        )
        .await?;
    }

    if !already_recorded {
        workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "pr_conflict",
                "blocked",
                summary,
                Some(classification),
            ))
            .await?;
    }

    Ok(!already_blocked || !already_recorded)
}

async fn settle_pr_conflict_repair_dispatch_failure(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    settlement: AgentWorkspaceRepairDispatchSettlement,
    auto_merge_current: Option<bool>,
) -> crate::AppResult<()> {
    let _ = settle_agent_workspace_repair_dispatch_outcome(
        repair_repo,
        branch_update_repo,
        attempt,
        settlement,
        summary,
        auto_merge_current,
    )
    .await?;
    Ok(())
}

async fn record_agent_workspace_repair_routed_to_existing_attempt(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    outcome: &str,
    signal_kind: &str,
    attempt: &AgentWorkspaceRepairAttempt,
    signal_summary: &str,
) -> crate::AppResult<bool> {
    let summary = format!(
        "PR #{pr_number} {signal_kind} signal was routed to an existing workspace repair attempt ({outcome}): {signal_summary}"
    );
    let classification = format!(
        "agent_workspace_repair_routed:{pr_number}:{outcome}:{signal_kind}:{}:{}",
        attempt.id, attempt.generation
    );
    // A process restart may record the current signal once more, but the durable fingerprint
    // prevents every later poll from creating another row for this repair generation.
    let already_recorded = workspace_repo
        .list_publication_events(conversation_id)
        .await?
        .iter()
        .any(|event| {
            event.step == "repair_routed"
                && event.classification.as_deref() == Some(classification.as_str())
        });
    if already_recorded {
        return Ok(false);
    }
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_routed",
            "waiting",
            summary,
            Some(classification),
        ))
        .await?;
    Ok(true)
}

#[cfg(test)]
async fn route_agent_workspace_pr_conflict_repair_if_needed(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    health: &PrHealth,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    route_agent_workspace_pr_conflict_repair_legacy(
        github,
        working_dir,
        pr_number,
        health,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        chat_service,
    )
    .await
}

async fn route_agent_workspace_pr_conflict_repair_if_needed_with_repair_repo(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    health: &PrHealth,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    _agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    let details = agent_workspace_pr_merge_conflict_details(health);
    if details.is_empty() {
        return Ok(false);
    }
    let Some(repair_repo) = repair_repo else {
        return Err(AppError::Infrastructure(
            "durable PR conflict repair dispatch requires workspace repair authority".to_string(),
        ));
    };
    let Some(branch_update_repo) = branch_update_repo else {
        return Err(AppError::Infrastructure(
            "durable PR conflict repair dispatch requires canonical Git target authority"
                .to_string(),
        ));
    };

    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;

    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.linked_plan_branch_id.is_some()
        || workspace.publication_pr_number != Some(pr_number)
        || workspace.has_terminal_publication_pr_status()
        || !workspace.auto_publish_enabled
    {
        return Ok(false);
    }

    // GitHub can continue to report Dirty while the current repair has already produced a local
    // rebased head. That head must enter its durable publish continuation before the poller can
    // consider another repair generation; starting or joining here would waste a fixer turn and
    // inject a false "fix the workspace" instruction into an already-clean workspace.
    if let Some(current) = repair_repo
        .get_current_repair_attempt(conversation_id)
        .await?
    {
        if held_repair_has_unpublished_head(&current, health.sync_state.head_ref_oid.as_deref()) {
            tracing::info!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                attempt_id = current.id.as_str(),
                "PR merge-conflict signal deferred to the existing unpublished repair head"
            );
            return Ok(false);
        }
    }

    let repair_summary =
        format!("Auto Publish routed PR #{pr_number} merge conflicts to workspace repair.");
    let conflict_summary = agent_workspace_pr_conflict_summary(pr_number, &details);
    let start = start_or_join_agent_workspace_repair_without_projection(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo),
        AgentWorkspaceRepairStartRequest {
            conversation_id: conversation_id.clone(),
            source: AgentWorkspaceRepairSource::PrConflict,
            continuation: AgentWorkspaceRepairContinuation::ResumePrSupervision,
            target_base_ref: workspace.base_ref.clone(),
            target_base_commit: workspace.base_commit.clone(),
            verified_newer_base: false,
            reason: repair_summary.clone(),
            summary: repair_summary.clone(),
            auto_merge_current: workspace.pr_auto_merge_current,
            explicit_publish_requested: false,
            retry_blocked: false,
            carryover_pr_autofix_evidence: None,
        },
    )
    .await?;
    let attempt = match start {
        AgentWorkspaceRepairStartOutcome::Started(attempt) => attempt,
        AgentWorkspaceRepairStartOutcome::Joined(attempt) => {
            let recorded = record_agent_workspace_repair_routed_to_existing_attempt(
                workspace_repo.as_ref(),
                conversation_id,
                pr_number,
                "joined",
                "merge-conflict",
                &attempt,
                &conflict_summary,
            )
            .await?;
            if recorded {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "joined",
                    "PR merge-conflict signal was routed to an existing workspace repair attempt"
                );
            } else {
                tracing::debug!(conversation_id = conversation_id.as_str(), pr_number, outcome = "joined", "PR merge-conflict signal remains routed to the existing workspace repair attempt");
            }
            return Ok(false);
        }
        AgentWorkspaceRepairStartOutcome::SuccessorStarted(attempt) => {
            let recorded = record_agent_workspace_repair_routed_to_existing_attempt(
                workspace_repo.as_ref(),
                conversation_id,
                pr_number,
                "successor_started",
                "merge-conflict",
                &attempt,
                &conflict_summary,
            )
            .await?;
            if recorded {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "successor_started",
                    "PR merge-conflict signal was routed to an existing workspace repair attempt"
                );
            } else {
                tracing::debug!(conversation_id = conversation_id.as_str(), pr_number, outcome = "successor_started", "PR merge-conflict signal remains routed to the existing workspace repair attempt");
            }
            return Ok(false);
        }
        AgentWorkspaceRepairStartOutcome::BlockedByCurrent(attempt) => {
            let recorded = record_agent_workspace_repair_routed_to_existing_attempt(
                workspace_repo.as_ref(),
                conversation_id,
                pr_number,
                "blocked_by_current",
                "merge-conflict",
                &attempt,
                &conflict_summary,
            )
            .await?;
            if recorded {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "blocked_by_current",
                    "PR merge-conflict signal was routed to an existing workspace repair attempt"
                );
            } else {
                tracing::debug!(conversation_id = conversation_id.as_str(), pr_number, outcome = "blocked_by_current", "PR merge-conflict signal remains routed to the existing workspace repair attempt");
            }
            return Ok(false);
        }
    };
    let message = build_agent_workspace_pr_conflict_repair_message(pr_number, &workspace, &details);
    let target_identity =
        GitService::canonical_target_identity(working_dir, &workspace.branch_name).await?;
    let repair_run_id = AgentRunId::new();
    let dispatch = match reserve_agent_workspace_repair_dispatch(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        target_identity,
        attempt,
        repair_run_id.clone(),
        &repair_summary,
        workspace.pr_auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairDispatchOutcome::Reserved(attempt) => attempt,
        AgentWorkspaceRepairDispatchOutcome::Stale(_)
        | AgentWorkspaceRepairDispatchOutcome::Missing => {
            return Ok(false);
        }
    };
    let auto_merge_current = match prepare_agent_workspace_pr_repair_auto_merge_state(
        Arc::clone(&github),
        working_dir,
        pr_number,
        conversation_id,
        health,
        Arc::clone(&workspace_repo),
        Some(repair_repo.as_ref()),
    )
    .await
    {
        Ok(Some(auto_merge_current)) => auto_merge_current,
        Ok(None) => {
            settle_pr_conflict_repair_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch,
                "Workspace repair dispatch was reserved, but GitHub auto-merge could not be disabled.",
                AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
                workspace.pr_auto_merge_current,
            )
            .await?;
            return Ok(false);
        }
        Err(error) => {
            let summary = format!(
                "Workspace repair dispatch was reserved, but GitHub auto-merge preparation failed: {error}"
            );
            settle_pr_conflict_repair_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch,
                &summary,
                AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
                workspace.pr_auto_merge_current,
            )
            .await?;
            return Ok(false);
        }
    };
    let delivery = chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            SendMessageOptions {
                preallocated_agent_run_id: Some(repair_run_id),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                conversation_id_override: Some(workspace.conversation_id.clone()),
                agent_name_override: Some(AGENT_WORKSPACE_REPAIR.to_string()),
                working_directory_override: Some(PathBuf::from(&workspace.worktree_path)),
                force_new_provider_session: true,
                preserve_conversation_provider_session_ref: true,
                ..Default::default()
            },
        )
        .await;
    let settlement = classify_agent_workspace_repair_delivery(
        delivery.as_ref(),
        conversation_id,
        &repair_run_id,
    );
    match delivery {
        Ok(_) if settlement == AgentWorkspaceRepairDispatchSettlement::Delivered => {
            if let Err(error) = settle_agent_workspace_repair_dispatch_outcome(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch,
                settlement,
                "Sent base update failure to workspace agent",
                Some(auto_merge_current),
            )
            .await
            {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Failed to persist successful PR conflict repair dispatch"
                );
            }
        }
        Ok(_) => {
            let summary =
                "Workspace repair launch did not preserve its reserved immediate-start authority";
            settle_pr_conflict_repair_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch,
                summary,
                settlement,
                Some(auto_merge_current),
            )
            .await?;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Failed to send GitHub PR conflict repair message"
            );
            let summary = format!("Failed to send base update failure to workspace agent: {error}");
            settle_pr_conflict_repair_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch,
                &summary,
                settlement,
                Some(auto_merge_current),
            )
            .await?;
        }
    }

    Ok(true)
}

/// Compatibility path for recovery callers that have not yet been given the durable repair
/// repository. Live polling always reaches the coordinator variant above.
#[cfg(test)]
async fn settle_pr_conflict_repair_dispatch_failure_legacy(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
) -> crate::AppResult<()> {
    if settle_agent_workspace_repair_failure(Arc::clone(&workspace_repo), claim, summary).await? {
        workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                claim.conversation_id.clone(),
                AGENT_WORKSPACE_REPAIR_SENT_STEP,
                "failed",
                summary,
                Some("operational".to_string()),
            ))
            .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
async fn route_agent_workspace_pr_conflict_repair_legacy(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    health: &PrHealth,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    let details = agent_workspace_pr_merge_conflict_details(health);
    if details.is_empty() {
        return Ok(false);
    }
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;
    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.linked_plan_branch_id.is_some()
        || workspace.publication_pr_number != Some(pr_number)
        || workspace.has_terminal_publication_pr_status()
        || !workspace.auto_publish_enabled
        || agent_workspace_pr_autofix_repair_in_flight(&workspace, agent_run_repo.as_ref()).await?
    {
        return Ok(false);
    }
    let classification =
        agent_workspace_pr_conflict_repair_event_classification(pr_number, health, &details);
    if workspace_repo
        .list_publication_events(conversation_id)
        .await?
        .into_iter()
        .any(|event| event.classification.as_deref() == Some(classification.as_str()))
    {
        return Ok(false);
    }
    let Some(auto_merge_current) = prepare_agent_workspace_pr_repair_auto_merge_state(
        Arc::clone(&github),
        working_dir,
        pr_number,
        conversation_id,
        health,
        Arc::clone(&workspace_repo),
        None,
    )
    .await?
    else {
        return Ok(false);
    };
    let repair_summary =
        format!("Auto Publish routed PR #{pr_number} merge conflicts to workspace repair.");
    let Some(claim) = claim_agent_workspace_repair(
        Arc::clone(&workspace_repo),
        conversation_id,
        &repair_summary,
        Some(auto_merge_current),
    )
    .await?
    else {
        return Ok(false);
    };
    for event in [
        AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_conflict_repair",
            "needs_agent",
            &repair_summary,
            Some(classification),
        ),
        AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            AGENT_WORKSPACE_REPAIR_REQUESTED_STEP,
            "started",
            "Workspace agent repair requested before the base update can complete",
            Some(AGENT_WORKSPACE_REPAIR_ACTION_UPDATE_ONLY_CLASSIFICATION.to_string()),
        ),
    ] {
        if let Err(error) = workspace_repo.append_publication_event(event).await {
            let summary = format!("Failed to record PR conflict repair request: {error}");
            settle_pr_conflict_repair_dispatch_failure_legacy(
                Arc::clone(&workspace_repo),
                &claim,
                &summary,
            )
            .await?;
            return Err(error);
        }
    }

    let message = build_agent_workspace_pr_conflict_repair_message(pr_number, &workspace, &details);
    let repair_run_id = AgentRunId::new();
    let repair_run_id_value = repair_run_id.to_string();
    let repair_run_classification = repair_run_event_classification(&repair_run_id);
    if let Err(error) = workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            AGENT_WORKSPACE_REPAIR_SENT_STEP,
            "started",
            "Starting workspace repair agent for base update failure",
            Some(repair_run_classification.clone()),
        ))
        .await
    {
        let summary = format!("Failed to reserve workspace repair dispatch: {error}");
        settle_pr_conflict_repair_dispatch_failure_legacy(
            Arc::clone(&workspace_repo),
            &claim,
            &summary,
        )
        .await?;
        return Err(error);
    }
    match chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            SendMessageOptions {
                preallocated_agent_run_id: Some(repair_run_id),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                conversation_id_override: Some(workspace.conversation_id.clone()),
                agent_name_override: Some(AGENT_WORKSPACE_REPAIR.to_string()),
                working_directory_override: Some(PathBuf::from(&workspace.worktree_path)),
                force_new_provider_session: true,
                preserve_conversation_provider_session_ref: true,
                ..Default::default()
            },
        )
        .await
    {
        Ok(result)
            if !result.was_queued
                && !result.queued_as_pending
                && result.conversation_id == conversation_id.as_str()
                && result.agent_run_id == repair_run_id_value =>
        {
            if let Err(error) = workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    AGENT_WORKSPACE_REPAIR_SENT_STEP,
                    "succeeded",
                    "Sent base update failure to workspace agent",
                    Some(repair_run_classification),
                ))
                .await
            {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Failed to record successful legacy PR conflict repair dispatch"
                );
            }
        }
        Ok(_) => {
            settle_pr_conflict_repair_dispatch_failure_legacy(
                Arc::clone(&workspace_repo),
                &claim,
                "Workspace repair launch did not preserve its reserved immediate-start authority",
            )
            .await?;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Failed to send legacy GitHub PR conflict repair message"
            );
            let summary = format!("Failed to send base update failure to workspace agent: {error}");
            settle_pr_conflict_repair_dispatch_failure_legacy(
                Arc::clone(&workspace_repo),
                &claim,
                &summary,
            )
            .await?;
        }
    }
    Ok(true)
}

async fn route_review_feedback_if_present(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    task_id: &TaskId,
    transition_service: Arc<TaskTransitionService>,
    history_actor: &str,
) -> crate::AppResult<bool> {
    let Some(feedback) = github
        .check_pr_review_feedback(working_dir, pr_number)
        .await?
    else {
        return Ok(false);
    };

    let Some(auto_merge_disabled_for_correction) =
        prepare_task_pr_correction_auto_merge_state(Arc::clone(&github), working_dir, pr_number)
            .await?
    else {
        return Ok(false);
    };

    transition_service
        .route_github_pr_changes_requested_with_auto_merge_marker(
            task_id,
            pr_number,
            feedback,
            history_actor,
            auto_merge_disabled_for_correction.disabled_for_correction,
            auto_merge_disabled_for_correction.method,
        )
        .await?;
    Ok(true)
}

async fn clear_task_auto_merge_correction_marker_for_terminal_pr(
    transition_service: Arc<TaskTransitionService>,
    task_id: &TaskId,
    pr_status: &str,
) {
    match transition_service
        .clear_github_auto_merge_correction_marker_for_terminal_pr(task_id, pr_status)
        .await
    {
        Ok(true) => tracing::info!(
            task_id = task_id.as_str(),
            pr_status,
            "PR poller: cleared disabled auto-merge correction marker after terminal PR state"
        ),
        Ok(false) => {}
        Err(error) => tracing::warn!(
            task_id = task_id.as_str(),
            pr_status,
            error = %error,
            "PR poller: failed to clear disabled auto-merge correction marker after terminal PR state"
        ),
    }
}

struct TaskPrCorrectionAutoMergeState {
    disabled_for_correction: bool,
    method: Option<String>,
}

async fn prepare_task_pr_correction_auto_merge_state(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
) -> crate::AppResult<Option<TaskPrCorrectionAutoMergeState>> {
    let health = github.fetch_pr_health(working_dir, pr_number).await?;
    let Some(auto_merge_request) = health.auto_merge_request else {
        return Ok(Some(TaskPrCorrectionAutoMergeState {
            disabled_for_correction: false,
            method: None,
        }));
    };
    let method = auto_merge_request.merge_method;

    match github.disable_pr_auto_merge(working_dir, pr_number).await {
        Ok(()) => Ok(Some(TaskPrCorrectionAutoMergeState {
            disabled_for_correction: true,
            method,
        })),
        Err(error) => {
            tracing::warn!(
                pr_number,
                error = %auto_merge_disable_failure_summary(error),
                "PR monitor skipped GitHub review correction because auto-merge could not be disabled"
            );
            Ok(None)
        }
    }
}

async fn prepare_agent_workspace_pr_repair_auto_merge_state(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    health: &PrHealth,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    repair_repo: Option<&dyn AgentWorkspaceRepairRepository>,
) -> crate::AppResult<Option<bool>> {
    if health.auto_merge_request.is_none() {
        return Ok(Some(false));
    }

    match github.disable_pr_auto_merge(working_dir, pr_number).await {
        Ok(()) => {
            update_agent_workspace_pr_supervision_state(
                workspace_repo.as_ref(),
                repair_repo,
                conversation_id,
                Some(false),
                None,
                Some(AGENT_WORKSPACE_AUTO_MERGE_DISARM_SUMMARY),
            )
            .await?;
            workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    AGENT_WORKSPACE_AUTO_MERGE_DISARM_STEP,
                    "succeeded",
                    AGENT_WORKSPACE_AUTO_MERGE_DISARM_SUMMARY,
                    Some(format!("github_auto_merge_disabled_for_repair:{pr_number}")),
                ))
                .await?;
            Ok(Some(false))
        }
        Err(error) => {
            let summary = auto_merge_disable_failure_summary(error);
            update_agent_workspace_pr_supervision_state(
                workspace_repo.as_ref(),
                repair_repo,
                conversation_id,
                Some(true),
                Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                Some(&summary),
            )
            .await?;
            workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    AGENT_WORKSPACE_AUTO_MERGE_DISARM_STEP,
                    "waiting",
                    &summary,
                    Some(format!("github_auto_merge_disable_failed:{pr_number}")),
                ))
                .await?;
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentWorkspacePrAutofixIssueKind {
    Review,
    Checks,
    Mergeability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspacePrAutofixIssue {
    kind: AgentWorkspacePrAutofixIssueKind,
    summary: String,
    details: Vec<String>,
    pub(crate) classification: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AgentWorkspacePrAutofixTargetKind {
    DirectWorkspace,
    IdeationPlan {
        linked_plan_branch_id: PlanBranchId,
        linked_ideation_session_id: IdeationSessionId,
    },
}

#[derive(Debug, Clone)]
struct AgentWorkspacePrAutofixTarget {
    kind: AgentWorkspacePrAutofixTargetKind,
    project_id: ProjectId,
    branch_name: String,
    pr_number: i64,
    pr_url: Option<String>,
}

impl AgentWorkspacePrAutofixTarget {
    fn direct(workspace: &AgentConversationWorkspace, pr_number: i64) -> Self {
        Self {
            kind: AgentWorkspacePrAutofixTargetKind::DirectWorkspace,
            project_id: workspace.project_id.clone(),
            branch_name: workspace.branch_name.clone(),
            pr_number,
            pr_url: workspace.publication_pr_url.clone(),
        }
    }

    fn ideation_plan(plan_branch: &PlanBranch, pr_number: i64) -> Self {
        Self {
            kind: AgentWorkspacePrAutofixTargetKind::IdeationPlan {
                linked_plan_branch_id: plan_branch.id.clone(),
                linked_ideation_session_id: plan_branch.session_id.clone(),
            },
            project_id: plan_branch.project_id.clone(),
            branch_name: plan_branch.branch_name.clone(),
            pr_number,
            pr_url: plan_branch.pr_url.clone(),
        }
    }

    fn review_feedback(workspace: &AgentConversationWorkspace, pr_number: i64) -> Option<Self> {
        match workspace.mode {
            AgentConversationWorkspaceMode::Edit
                if workspace.linked_plan_branch_id.is_none()
                    && workspace.publication_pr_number == Some(pr_number) =>
            {
                Some(Self::direct(workspace, pr_number))
            }
            AgentConversationWorkspaceMode::Ideation => Some(Self {
                kind: AgentWorkspacePrAutofixTargetKind::IdeationPlan {
                    linked_plan_branch_id: workspace.linked_plan_branch_id.clone()?,
                    linked_ideation_session_id: workspace.linked_ideation_session_id.clone()?,
                },
                project_id: workspace.project_id.clone(),
                branch_name: workspace.branch_name.clone(),
                pr_number,
                pr_url: workspace.publication_pr_url.clone(),
            }),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match &self.kind {
            AgentWorkspacePrAutofixTargetKind::DirectWorkspace => "agent workspace",
            AgentWorkspacePrAutofixTargetKind::IdeationPlan { .. } => "linked ideation plan",
        }
    }

    fn updates_workspace_publication(&self) -> bool {
        matches!(
            &self.kind,
            AgentWorkspacePrAutofixTargetKind::DirectWorkspace
        )
    }

    fn authorizes(&self, workspace: &AgentConversationWorkspace) -> bool {
        if !self.authorizes_identity_and_preferences(workspace) {
            return false;
        }

        match &self.kind {
            AgentWorkspacePrAutofixTargetKind::DirectWorkspace => {
                workspace.mode == AgentConversationWorkspaceMode::Edit
                    && workspace.linked_plan_branch_id.is_none()
                    && workspace.publication_pr_number == Some(self.pr_number)
                    && !workspace.has_terminal_publication_pr_status()
                    && workspace.has_pr_status_pollable_push_status()
            }
            AgentWorkspacePrAutofixTargetKind::IdeationPlan {
                linked_plan_branch_id,
                linked_ideation_session_id,
            } => {
                workspace.mode == AgentConversationWorkspaceMode::Ideation
                    && workspace.linked_plan_branch_id.as_ref() == Some(linked_plan_branch_id)
                    && workspace.linked_ideation_session_id.as_ref()
                        == Some(linked_ideation_session_id)
            }
        }
    }

    #[cfg(test)]
    fn authorizes_claimed_repair(&self, workspace: &AgentConversationWorkspace) -> bool {
        if !self.authorizes_identity_and_preferences(workspace) {
            return false;
        }

        match &self.kind {
            AgentWorkspacePrAutofixTargetKind::DirectWorkspace => {
                workspace.mode == AgentConversationWorkspaceMode::Edit
                    && workspace.linked_plan_branch_id.is_none()
                    && workspace.publication_pr_number == Some(self.pr_number)
                    && !workspace.has_terminal_publication_pr_status()
                    && workspace.publication_push_status.as_deref() == Some("needs_agent")
                    && workspace.pr_supervision_status.as_deref() == Some("fixing")
            }
            AgentWorkspacePrAutofixTargetKind::IdeationPlan {
                linked_plan_branch_id,
                linked_ideation_session_id,
            } => {
                workspace.mode == AgentConversationWorkspaceMode::Ideation
                    && workspace.linked_plan_branch_id.as_ref() == Some(linked_plan_branch_id)
                    && workspace.linked_ideation_session_id.as_ref()
                        == Some(linked_ideation_session_id)
            }
        }
    }

    fn authorizes_identity_and_preferences(&self, workspace: &AgentConversationWorkspace) -> bool {
        workspace.status == AgentConversationWorkspaceStatus::Active
            && workspace.project_id == self.project_id
            && workspace.branch_name == self.branch_name
            && workspace.auto_publish_enabled
            && workspace.pr_autofix_enabled
    }
}

async fn authorize_agent_workspace_pr_autofix(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    target: &AgentWorkspacePrAutofixTarget,
) -> crate::AppResult<Option<AgentConversationWorkspace>> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };

    Ok(target.authorizes(&workspace).then_some(workspace))
}

/// Resolves the PR health a supervision branch should reason about.
///
/// The workspace poll loop already reads health once per iteration, so every branch in that
/// iteration receives the same snapshot instead of paying its own GitHub read. Callers outside a
/// poll iteration pass `None` and fetch their own. Reusing the snapshot is never staler than the
/// per-branch fetch it replaces: all consumers run inside the same iteration, and the pre-change
/// behavior already tolerated that much intra-iteration drift.
async fn resolve_polled_pr_health(
    github: &Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    polled_health: Option<&PrHealth>,
) -> crate::AppResult<PrHealth> {
    match polled_health {
        Some(health) => Ok(health.clone()),
        None => github.fetch_pr_health(working_dir, pr_number).await,
    }
}

#[cfg(test)]
async fn route_agent_workspace_pr_autofix_if_needed(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
        github,
        working_dir,
        pr_number,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        None,
        None,
        chat_service,
        None,
    )
    .await
}

/// Compatibility entry point without user notifications. Production polling uses
/// [`route_agent_workspace_pr_autofix_if_needed_with_notifications`] so hand-offs reach the Inbox.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
async fn route_agent_workspace_pr_autofix_if_needed_with_repair_repo(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    chat_service: Arc<dyn ChatService>,
    polled_health: Option<&PrHealth>,
) -> crate::AppResult<bool> {
    route_agent_workspace_pr_autofix_if_needed_with_notifications(
        github,
        working_dir,
        pr_number,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        repair_repo,
        branch_update_repo,
        chat_service,
        None,
        None,
        polled_health,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn route_agent_workspace_pr_autofix_if_needed_with_notifications(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    chat_service: Arc<dyn ChatService>,
    notification_service: Option<Arc<NotificationService>>,
    project: Option<&Project>,
    polled_health: Option<&PrHealth>,
) -> crate::AppResult<bool> {
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;

    let target = AgentWorkspacePrAutofixTarget::direct(&workspace, pr_number);
    route_agent_workspace_pr_autofix_for_target(
        github,
        working_dir,
        target,
        workspace,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        repair_repo,
        branch_update_repo,
        chat_service,
        notification_service,
        project,
        None,
        polled_health,
    )
    .await
}

/// Performs one authoritative held-health reconciliation and shares its completion with
/// concurrent callers for the same conversation.
pub(crate) async fn recheck_agent_workspace_pr_health(
    state: &AppState,
    conversation_id: &ChatConversationId,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    let rechecks = HELD_PR_HEALTH_RECHECKS.get_or_init(DashMap::new);
    let shared = rechecks
        .entry(conversation_id.clone())
        .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
        .clone();
    let result = shared
        .get_or_init(|| async {
            recheck_agent_workspace_pr_health_once(state, conversation_id, chat_service)
                .await
                .map_err(|error| error.to_string())
        })
        .await
        .clone();
    if rechecks
        .get(conversation_id)
        .is_some_and(|current| Arc::ptr_eq(current.value(), &shared))
    {
        rechecks.remove(conversation_id);
    }
    result.map_err(AppError::Infrastructure)
}

async fn recheck_agent_workspace_pr_health_once(
    state: &AppState,
    conversation_id: &ChatConversationId,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {conversation_id}"
            ))
        })?;
    let pr_number = workspace.publication_pr_number.ok_or_else(|| {
        AppError::Conflict("Held PR health recheck requires a linked pull request".to_string())
    })?;
    let attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::Conflict(
                "Held PR health recheck requires a current repair attempt".to_string(),
            )
        })?;
    // Flipping `status` to `Held` for an escalated ContinuationPending/Continuing publication
    // effect (Phase 3) makes the typed-status check above pass for a state this command was
    // never designed to touch. That hold is settled only through the dedicated publication-
    // effect retry command, never through the ordinary held-PR-health recheck.
    if !attempt.is_unsettled()
        || attempt.operation_snapshot().status != AgentWorkspaceRepairOperationStatus::Held
        || matches!(
            attempt.phase,
            AgentWorkspaceRepairPhase::ContinuationPending | AgentWorkspaceRepairPhase::Continuing
        )
    {
        return Err(AppError::Conflict(
            "The PR repair hold is no longer current".to_string(),
        ));
    }
    let authority = HeldPrHealthRecheckAuthority {
        attempt_id: attempt.id,
        generation: attempt.generation,
        updated_at: attempt.updated_at,
    };
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.to_string()))?;
    let working_dir = resolve_valid_agent_conversation_workspace_path(&project, &workspace).await?;
    let github = state
        .github_service
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| AppError::Infrastructure("GitHub service is unavailable".to_string()))?;

    route_agent_workspace_pr_autofix_for_target(
        github,
        &working_dir,
        AgentWorkspacePrAutofixTarget::direct(&workspace, pr_number),
        workspace,
        conversation_id,
        Arc::clone(&state.agent_conversation_workspace_repo),
        Some(Arc::clone(&state.agent_run_repo)),
        Some(Arc::clone(&state.agent_workspace_repair_repo)),
        Some(Arc::clone(&state.branch_update_repo)),
        chat_service,
        Some(state.notification_service()),
        Some(&project),
        Some(authority),
        None,
    )
    .await
}

async fn agent_workspace_base_update_unsettled_reason(
    working_dir: &Path,
    workspace: &AgentConversationWorkspace,
    attempt: &AgentWorkspaceRepairAttempt,
) -> crate::AppResult<Option<String>> {
    let current_branch = GitService::get_current_branch(working_dir).await?;
    if current_branch != workspace.branch_name {
        return Ok(Some(format!(
            "workspace branch '{}' is checked out instead of '{}'",
            current_branch, workspace.branch_name
        )));
    }

    let workspace_head_sha = GitService::get_head_sha(working_dir).await?;
    let reported_head_sha = attempt
        .repair_head_commit
        .as_deref()
        .map(str::trim)
        .filter(|head| !head.is_empty())
        .unwrap_or(workspace_head_sha.as_str());
    let unfinished = GitService::unfinished_operation_state(working_dir)?;
    let settled = verify_agent_workspace_settled_current_head(AgentWorkspaceSettledHeadCheck {
        reported_head_sha,
        workspace_head_sha: &workspace_head_sha,
        has_uncommitted_changes: GitService::has_uncommitted_changes(working_dir).await?,
        is_merge_in_progress: unfinished.merge_in_progress,
        is_rebase_in_progress: unfinished.rebase_in_progress,
        has_conflict_files: !GitService::get_conflict_files(working_dir)
            .await?
            .is_empty(),
        has_conflict_markers: GitService::has_conflict_markers(working_dir).await?,
    });
    Ok(settled.err())
}

async fn settle_ready_agent_workspace_repair_attempt(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    branch_update_repo: &dyn BranchUpdateRepository,
    attempt: &AgentWorkspaceRepairAttempt,
) -> crate::AppResult<bool> {
    let attempt = match release_and_clear_agent_workspace_repair_target_lease(
        repair_repo,
        branch_update_repo,
        attempt.clone(),
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(false);
        }
    };
    let outcome = repair_repo
        .settle_repair_attempt(SettleAgentWorkspaceRepairAttempt {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Ready,
            expected_updated_at: attempt.updated_at,
            outcome: crate::domain::entities::AgentWorkspaceRepairOutcome::Succeeded,
            settled_at: chrono::Utc::now(),
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?;
    Ok(matches!(
        outcome,
        SettleAgentWorkspaceRepairAttemptOutcome::Applied(_)
    ))
}

fn add_base_update_assignment(issue: &mut Option<AgentWorkspacePrAutofixIssue>, reason: &str) {
    let Some(issue) = issue.as_mut() else {
        return;
    };
    issue.summary = format!("{}; update the branch from base", issue.summary);
    issue.details.insert(
        0,
        format!("Update the branch from its configured base and publish it: {reason}"),
    );
}

async fn record_agent_workspace_base_update_route(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    observed_base_oid: &str,
    status: &str,
    summary: &str,
) -> crate::AppResult<()> {
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_base_update",
            status,
            summary,
            Some(format!(
                "github_pr_base_update:{pr_number}:{observed_base_oid}:{status}"
            )),
        ))
        .await
}

async fn persist_agent_workspace_observed_base(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    observed_base_oid: String,
) -> crate::AppResult<AgentConversationWorkspace> {
    let mut workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;
    workspace.base_commit = Some(observed_base_oid);
    workspace.updated_at = chrono::Utc::now();
    workspace_repo.create_or_update(workspace).await
}

#[allow(clippy::too_many_arguments)]
async fn hold_agent_workspace_base_update_route(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    attempt: AgentWorkspaceRepairAttempt,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    observed_base_oid: &str,
    message: &str,
    auto_merge_current: Option<bool>,
) -> crate::AppResult<()> {
    if let AgentWorkspaceRepairTransitionOutcome::Applied(_) =
        reserve_agent_workspace_base_stale_hold(
            repair_repo,
            attempt,
            observed_base_oid,
            message,
            auto_merge_current,
        )
        .await?
    {
        record_agent_workspace_base_update_route(
            workspace_repo,
            conversation_id,
            pr_number,
            observed_base_oid,
            "blocked",
            message,
        )
        .await?;
    }
    Ok(())
}

enum BehindBaseUpdateRoute {
    Updated {
        reserved: AgentWorkspaceRepairAttempt,
    },
    DeferToAgent {
        reserved: AgentWorkspaceRepairAttempt,
        reason: String,
    },
    Blocked {
        reserved: AgentWorkspaceRepairAttempt,
        message: String,
    },
    Rejected,
}

async fn mark_behind_base_update_route(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    route: BehindBaseUpdateRoute,
    observed_base_oid: &str,
    auto_merge_current: Option<bool>,
) -> crate::AppResult<BehindBaseUpdateRoute> {
    let (attempt, summary) = match &route {
        BehindBaseUpdateRoute::Updated { reserved } => (
            reserved.clone(),
            "RalphX updated and pushed the branch from its current base.".to_string(),
        ),
        // A defer performs no automatic update. Settling it is the authority for dispatching a
        // new fixer; persisting the anti-runaway marker first would make a crash look successful.
        BehindBaseUpdateRoute::DeferToAgent { .. } => return Ok(route),
        BehindBaseUpdateRoute::Blocked { reserved, message } => (reserved.clone(), message.clone()),
        BehindBaseUpdateRoute::Rejected => return Ok(route),
    };
    let marked = match mark_agent_workspace_base_update_target(
        repair_repo,
        attempt,
        observed_base_oid,
        &summary,
        auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(BehindBaseUpdateRoute::Rejected)
        }
    };
    Ok(match route {
        BehindBaseUpdateRoute::Updated { .. } => {
            BehindBaseUpdateRoute::Updated { reserved: marked }
        }
        BehindBaseUpdateRoute::Blocked { message, .. } => BehindBaseUpdateRoute::Blocked {
            reserved: marked,
            message,
        },
        BehindBaseUpdateRoute::DeferToAgent { .. } | BehindBaseUpdateRoute::Rejected => {
            unreachable!("returned before marking")
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn drive_agent_workspace_behind_base_update(
    github: &dyn GithubServiceTrait,
    working_dir: &Path,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    observed_base_oid: &str,
    summary: &str,
) -> crate::AppResult<BehindBaseUpdateRoute> {
    let reserved = match reserve_agent_workspace_base_update(
        Arc::clone(&repair_repo),
        attempt,
        observed_base_oid,
        summary,
        workspace.pr_auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return Ok(BehindBaseUpdateRoute::Rejected)
        }
    };
    if let Some(reason) =
        agent_workspace_base_update_unsettled_reason(working_dir, workspace, &reserved).await?
    {
        return mark_behind_base_update_route(
            repair_repo,
            BehindBaseUpdateRoute::DeferToAgent { reserved, reason },
            observed_base_oid,
            workspace.pr_auto_merge_current,
        )
        .await;
    }

    let outcome = ensure_publish_branch_fresh(
        working_dir,
        project,
        &workspace.branch_name,
        &workspace.base_ref,
        &workspace.conversation_id.as_str(),
        None,
    )
    .await;
    let route = match outcome {
        PublishBranchFreshnessOutcome::Updated { .. } => {
            match github.push_branch(working_dir, &workspace.branch_name).await {
                Ok(()) => BehindBaseUpdateRoute::Updated { reserved },
                Err(error) => BehindBaseUpdateRoute::Blocked {
                    reserved,
                    message: format!(
                        "RalphX updated the branch to {observed_base_oid} but could not push it: {error}"
                    ),
                },
            }
        }
        PublishBranchFreshnessOutcome::AlreadyFresh { .. } => BehindBaseUpdateRoute::Blocked {
            reserved,
            message: format!(
                "GitHub reports the PR behind {} at {}, but the local branch already contains that base tip.",
                workspace.base_ref, observed_base_oid
            ),
        },
        PublishBranchFreshnessOutcome::OperationalError { message } => {
            BehindBaseUpdateRoute::Blocked { reserved, message }
        }
        PublishBranchFreshnessOutcome::NeedsAgent {
            message,
            conflict_files,
            ..
        } => BehindBaseUpdateRoute::DeferToAgent {
            reserved,
            reason: if conflict_files.is_empty() {
                message
            } else {
                format!("{message} Conflicts: {}", conflict_files.join(", "))
            },
            },
    };
    mark_behind_base_update_route(
        repair_repo,
        route,
        observed_base_oid,
        workspace.pr_auto_merge_current,
    )
    .await
}

pub(crate) async fn route_ideation_plan_pr_autofix_if_needed(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    plan_branch: &PlanBranch,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    let Some(pr_number) = plan_branch.pr_number else {
        return Ok(false);
    };

    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;

    if workspace.status != AgentConversationWorkspaceStatus::Active
        || workspace.mode != AgentConversationWorkspaceMode::Ideation
        || workspace.project_id != plan_branch.project_id
        || workspace.linked_plan_branch_id.as_ref() != Some(&plan_branch.id)
        || workspace.linked_ideation_session_id.as_ref() != Some(&plan_branch.session_id)
        || workspace.branch_name != plan_branch.branch_name
        || matches!(
            plan_branch.pr_status,
            Some(DbPrStatus::Closed | DbPrStatus::Merged)
        )
    {
        return Ok(false);
    }

    let target = AgentWorkspacePrAutofixTarget::ideation_plan(plan_branch, pr_number);
    route_agent_workspace_pr_autofix_for_target(
        github,
        working_dir,
        target,
        workspace,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        None,
        None,
        chat_service,
        None,
        None,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn route_agent_workspace_pr_autofix_for_target(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    target: AgentWorkspacePrAutofixTarget,
    mut workspace: AgentConversationWorkspace,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    chat_service: Arc<dyn ChatService>,
    notification_service: Option<Arc<NotificationService>>,
    project: Option<&Project>,
    expected_held_attempt: Option<HeldPrHealthRecheckAuthority>,
    polled_health: Option<&PrHealth>,
) -> crate::AppResult<bool> {
    let target_matches_workspace_mode = matches!(
        (workspace.mode, &target.kind),
        (
            AgentConversationWorkspaceMode::Edit,
            AgentWorkspacePrAutofixTargetKind::DirectWorkspace
        ) | (
            AgentConversationWorkspaceMode::Ideation,
            AgentWorkspacePrAutofixTargetKind::IdeationPlan { .. }
        )
    );
    if !workspace.allows_owned_pr_mutation() || !target_matches_workspace_mode {
        return Ok(false);
    }

    if !workspace.auto_publish_enabled {
        return Ok(false);
    }
    if repair_repo.is_none() {
        #[cfg(not(test))]
        return Err(AppError::Infrastructure(
            "PR autofix dispatch requires durable workspace repair authority".to_string(),
        ));
    }
    if repair_repo.is_some() && branch_update_repo.is_none() {
        return Err(AppError::Infrastructure(
            "durable PR autofix dispatch requires canonical Git target authority".to_string(),
        ));
    }
    if repair_repo.is_none()
        && agent_workspace_pr_autofix_repair_in_flight(&workspace, agent_run_repo.as_ref()).await?
    {
        return Ok(false);
    }

    let health =
        resolve_polled_pr_health(&github, working_dir, target.pr_number, polled_health).await?;
    let mut current_issue = classify_agent_workspace_pr_autofix_issue(target.pr_number, &health);
    let mut superseding_base_commit = None;
    // A durable completion may have already reserved a rerun or classified this exact state as
    // pre-existing on base. Neither outcome authorizes a new fixer until GitHub changes health.
    // Do not launch a new fixer generation until GitHub reports a different conclusion.
    if let Some(repair_repo) = repair_repo.as_ref() {
        let current_attempt = repair_repo
            .get_current_repair_attempt(conversation_id)
            .await?;
        if expected_held_attempt.is_some() && current_attempt.is_none() {
            return Ok(false);
        }
        if let Some(mut attempt) = current_attempt {
            if expected_held_attempt.as_ref().is_some_and(|expected| {
                attempt.id != expected.attempt_id
                    || attempt.generation != expected.generation
                    || attempt.updated_at != expected.updated_at
                    || attempt.phase != AgentWorkspaceRepairPhase::Ready
            }) {
                return Ok(false);
            }
            if attempt.phase == AgentWorkspaceRepairPhase::Ready {
                let mut attempt_already_settled = false;
                let mut health_suppressed = agent_workspace_repair_is_health_held(&attempt);
                let mut ci_held = agent_workspace_repair_is_ci_held(&attempt);
                let mut base_stale_held = agent_workspace_repair_is_base_stale_held(&attempt);
                let disposition = classify_health_hold_disposition(BaseStalenessObservation {
                    merge_state_status: health.sync_state.merge_state_status.as_ref(),
                    observed_base_oid: health.sync_state.base_ref_oid.as_deref(),
                    attempt_target_base_commit: attempt.target_base_commit.as_deref(),
                    last_base_update_oid: attempt.base_update_target_commit.as_deref(),
                });
                let merge_state_is_known = !matches!(
                    health.sync_state.merge_state_status.as_ref(),
                    None | Some(PrMergeStateStatus::Unknown | PrMergeStateStatus::Other(_))
                );
                let observed_base_is_known = health
                    .sync_state
                    .base_ref_oid
                    .as_deref()
                    .is_some_and(|oid| !oid.trim().is_empty());
                if base_stale_held
                    && merge_state_is_known
                    && observed_base_is_known
                    && !matches!(
                        disposition,
                        HealthHoldDisposition::BlockedStaleAfterUpdate { .. }
                    )
                {
                    attempt = match release_agent_workspace_base_stale_hold(
                        Arc::clone(repair_repo),
                        attempt,
                        "GitHub no longer reports the PR behind the targeted base tip; RalphX resumed CI evidence supervision.",
                        workspace.pr_auto_merge_current,
                    )
                    .await?
                    {
                        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
                        AgentWorkspaceRepairTransitionOutcome::Stale(_)
                        | AgentWorkspaceRepairTransitionOutcome::Missing => return Ok(false),
                    };
                    health_suppressed = agent_workspace_repair_is_health_held(&attempt);
                    ci_held = agent_workspace_repair_is_ci_held(&attempt);
                    base_stale_held = agent_workspace_repair_is_base_stale_held(&attempt);
                }
                if base_stale_held && (!merge_state_is_known || !observed_base_is_known) {
                    return Ok(false);
                }
                let hold_active = health_suppressed || ci_held || base_stale_held;
                if hold_active {
                    if let HealthHoldDisposition::BlockedStaleAfterUpdate { observed_base_oid } =
                        &disposition
                    {
                        if base_stale_held {
                            return Ok(false);
                        }
                        let summary = format!(
                            "PR branch is still behind {} at {} after RalphX updated to that base tip.",
                            health.sync_state.base_ref_name, observed_base_oid
                        );
                        hold_agent_workspace_base_update_route(
                            Arc::clone(repair_repo),
                            workspace_repo.as_ref(),
                            attempt.clone(),
                            conversation_id,
                            target.pr_number,
                            observed_base_oid,
                            &summary,
                            workspace.pr_auto_merge_current,
                        )
                        .await?;
                        return Ok(false);
                    }
                }

                if hold_active
                    && matches!(
                        (&target.kind, &disposition, project),
                        (
                            AgentWorkspacePrAutofixTargetKind::DirectWorkspace,
                            HealthHoldDisposition::SupersedeForBaseUpdate { .. },
                            Some(_)
                        )
                    )
                {
                    let HealthHoldDisposition::SupersedeForBaseUpdate { observed_base_oid } =
                        disposition.clone()
                    else {
                        unreachable!("guarded by the disposition match above")
                    };
                    let project = project.expect("guarded by the project match above");
                    let Some(branch_update_repo) = branch_update_repo.as_ref() else {
                        return Ok(false);
                    };
                    let persisted_target = match validate_agent_workspace_repair_target_lease(
                        branch_update_repo.as_ref(),
                        &attempt,
                    )
                    .await
                    {
                        Ok(target) => target,
                        Err(error) => {
                            tracing::warn!(
                                conversation_id = conversation_id.as_str(),
                                pr_number = target.pr_number,
                                attempt_id = attempt.id.as_str(),
                                error = %error,
                                "PR base update skipped because its durable target lease is not current"
                            );
                            return Ok(false);
                        }
                    };
                    let observed_target =
                        GitService::canonical_target_identity(working_dir, &workspace.branch_name)
                            .await?;
                    if observed_target != persisted_target {
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number = target.pr_number,
                            attempt_id = attempt.id.as_str(),
                            "PR base update skipped because the worktree no longer matches its durable target lease"
                        );
                        return Ok(false);
                    }
                    let summary = format!(
                        "PR branch is behind {} at {}; RalphX reserved a base update before accepting more CI evidence.",
                        health.sync_state.base_ref_name, observed_base_oid
                    );
                    match drive_agent_workspace_behind_base_update(
                        github.as_ref(),
                        working_dir,
                        project,
                        &workspace,
                        Arc::clone(repair_repo),
                        attempt.clone(),
                        &observed_base_oid,
                        &summary,
                    )
                    .await?
                    {
                        BehindBaseUpdateRoute::Updated { .. } => {
                            persist_agent_workspace_observed_base(
                                workspace_repo.as_ref(),
                                conversation_id,
                                observed_base_oid.clone(),
                            )
                            .await?;
                            record_agent_workspace_base_update_route(
                                workspace_repo.as_ref(),
                                conversation_id,
                                target.pr_number,
                                &observed_base_oid,
                                "updated",
                                "RalphX merged the current base and pushed the PR branch; waiting for fresh GitHub evidence.",
                            )
                            .await?;
                            return Ok(false);
                        }
                        BehindBaseUpdateRoute::DeferToAgent { reserved, reason } => {
                            if !settle_ready_agent_workspace_repair_attempt(
                                repair_repo.as_ref(),
                                branch_update_repo.as_ref(),
                                &reserved,
                            )
                            .await?
                            {
                                return Ok(false);
                            }
                            record_agent_workspace_base_update_route(
                                workspace_repo.as_ref(),
                                conversation_id,
                                target.pr_number,
                                &observed_base_oid,
                                "deferred",
                                &reason,
                            )
                            .await?;
                            superseding_base_commit = Some(observed_base_oid);
                            add_base_update_assignment(&mut current_issue, &reason);
                            attempt_already_settled = true;
                        }
                        BehindBaseUpdateRoute::Blocked { reserved, message } => {
                            hold_agent_workspace_base_update_route(
                                Arc::clone(repair_repo),
                                workspace_repo.as_ref(),
                                reserved,
                                conversation_id,
                                target.pr_number,
                                &observed_base_oid,
                                &message,
                                workspace.pr_auto_merge_current,
                            )
                            .await?;
                            return Ok(false);
                        }
                        BehindBaseUpdateRoute::Rejected => return Ok(false),
                    }
                }
                if !attempt_already_settled {
                    if ci_held
                        && ci_rerun_hold_still_pending(
                            &health,
                            attempt.ci_rerun_fingerprint.as_deref(),
                        )
                    {
                        return Ok(false);
                    }
                    if health_suppressed
                        && matches!(disposition, HealthHoldDisposition::Retain)
                        && current_issue.as_ref().is_some_and(|issue| {
                            attempt.pr_autofix_health_fingerprint.as_deref()
                                == Some(issue.classification.as_str())
                        })
                    {
                        if held_repair_has_unpublished_head(
                            &attempt,
                            health.sync_state.head_ref_oid.as_deref(),
                        ) {
                            tracing::info!(
                                conversation_id = conversation_id.as_str(),
                                pr_number = target.pr_number,
                                attempt_id = attempt.id.as_str(),
                                "PR health hold retained while durable recovery re-drives its unpublished repair head"
                            );
                        }
                        return Ok(false);
                    }
                }
                if (health_suppressed || ci_held) && !attempt_already_settled {
                    superseding_base_commit = match disposition {
                        HealthHoldDisposition::SupersedeForNewEvidence { observed_base_oid }
                        | HealthHoldDisposition::SupersedeForBaseUpdate { observed_base_oid } => {
                            Some(observed_base_oid)
                        }
                        HealthHoldDisposition::Retain
                        | HealthHoldDisposition::BlockedStaleAfterUpdate { .. } => None,
                    };
                    // A changed conclusion ends the rerun-pending generation. The next normal
                    // dispatch below creates a fresh, independently fenced repair attempt.
                    if !settle_ready_agent_workspace_repair_attempt(
                        repair_repo.as_ref(),
                        branch_update_repo
                            .as_ref()
                            .expect(
                                "durable repair dispatch requires canonical Git target authority",
                            )
                            .as_ref(),
                        &attempt,
                    )
                    .await?
                    {
                        return Ok(false);
                    }
                }
            }
        }
    }
    if let Some(base_commit) = superseding_base_commit {
        workspace = persist_agent_workspace_observed_base(
            workspace_repo.as_ref(),
            conversation_id,
            base_commit,
        )
        .await?;
    }
    import_agent_workspace_pr_comment_evidence(
        Arc::clone(&workspace_repo),
        conversation_id,
        target.pr_number,
        &health,
    )
    .await?;
    if let Some((terminal_status, summary)) =
        agent_workspace_terminal_status_from_pr_health(&health)
    {
        if target.updates_workspace_publication() {
            workspace_repo
                .update_publication(
                    conversation_id,
                    workspace.publication_pr_number,
                    workspace.publication_pr_url.as_deref(),
                    Some(terminal_status),
                    workspace.publication_push_status.as_deref(),
                )
                .await?;
        }
        workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                "pr_terminal",
                terminal_status,
                summary,
                Some(format!(
                    "github_pr_terminal:{}:{terminal_status}",
                    target.pr_number
                )),
            ))
            .await?;
        return Ok(false);
    }
    let Some(issue) = current_issue else {
        let auto_merge_current = sync_agent_workspace_auto_merge_preference(
            Arc::clone(&github),
            working_dir,
            target.pr_number,
            &workspace,
            &health,
            Arc::clone(&workspace_repo),
            repair_repo.as_deref(),
        )
        .await?;
        let auto_merge_guard_blocks_enable = workspace_review_auto_merge_guard_blocks_enable(
            workspace_repo.as_ref(),
            conversation_id,
        )
        .await?;
        let auto_merge_pending = workspace.pr_auto_merge_desired
            && !auto_merge_current
            && !auto_merge_guard_blocks_enable;
        if !auto_merge_pending
            && !auto_merge_guard_blocks_enable
            && (workspace.pr_supervision_status.as_deref() != Some("monitoring")
                || workspace.pr_auto_merge_current != Some(auto_merge_current))
        {
            update_agent_workspace_pr_supervision_state(
                workspace_repo.as_ref(),
                repair_repo.as_deref(),
                conversation_id,
                Some(auto_merge_current),
                Some("monitoring"),
                Some("RalphX is monitoring PR health."),
            )
            .await?;
        }
        return Ok(false);
    };
    if !agent_workspace_pr_health_has_head(&health) {
        update_agent_workspace_pr_supervision_state(
            workspace_repo.as_ref(),
            repair_repo.as_deref(),
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("blocked"),
            Some("PR autofix is blocked because GitHub did not report the current head commit."),
        )
        .await?;
        return Ok(false);
    }

    let Some(agent_run_repo) = agent_run_repo.as_ref() else {
        tracing::error!(
            conversation_id = conversation_id.as_str(),
            pr_number = target.pr_number,
            "Agent workspace PR autofix requires an AgentRun repository"
        );
        return Ok(false);
    };
    if repair_repo.is_none() {
        let legacy_event_exists = workspace_repo
            .list_publication_events(conversation_id)
            .await?
            .into_iter()
            .any(|event| event.classification.as_deref() == Some(issue.classification.as_str()));
        let attempt_decision = load_pr_autofix_attempt_decision(
            agent_run_repo.as_ref(),
            conversation_id,
            target.pr_number,
            &issue.classification,
            legacy_event_exists,
        )
        .await?;
        if !attempt_decision.allows_start() {
            if let Some(summary) = attempt_decision.manual_summary() {
                update_agent_workspace_pr_supervision_state(
                    workspace_repo.as_ref(),
                    repair_repo.as_deref(),
                    conversation_id,
                    workspace.pr_auto_merge_current,
                    Some("blocked"),
                    Some(summary),
                )
                .await?;
            }
            return Ok(false);
        }
    }
    if cross_streak_fingerprint_suppresses_dispatch(
        workspace_repo.as_ref(),
        conversation_id,
        &issue.classification,
    )
    .await?
    {
        return Ok(false);
    }
    // Checking the base costs one API call; sending an agent to "fix" a failure the PR did not
    // cause costs a full generation and cannot succeed.
    if issue.kind == AgentWorkspacePrAutofixIssueKind::Checks
        && pr_failures_already_fail_on_base(github.as_ref(), working_dir, &health).await
    {
        record_pre_existing_on_base_detection(
            workspace_repo.as_ref(),
            conversation_id,
            &workspace,
            &issue.classification,
            &health,
            notification_service.as_ref(),
        )
        .await?;
        return Ok(false);
    }
    if authorize_agent_workspace_pr_autofix(workspace_repo.as_ref(), conversation_id, &target)
        .await?
        .is_none()
    {
        return Ok(false);
    }

    let Some(workspace_for_options) =
        authorize_agent_workspace_pr_autofix(workspace_repo.as_ref(), conversation_id, &target)
            .await?
    else {
        return Ok(false);
    };
    // Test-only compatibility dispatch has no durable target authority. Keep its historical
    // behavior isolated; production always takes the reservation-first path below.
    let auto_merge_before_reservation = if repair_repo.is_none() {
        match prepare_agent_workspace_pr_repair_auto_merge_state(
            Arc::clone(&github),
            working_dir,
            target.pr_number,
            conversation_id,
            &health,
            Arc::clone(&workspace_repo),
            repair_repo.as_deref(),
        )
        .await
        {
            Ok(Some(auto_merge_current)) => Some(auto_merge_current),
            Ok(None) => return Ok(false),
            Err(error) => {
                let summary = format!(
                    "PR autofix could not persist the GitHub auto-merge disarm state: {error}"
                );
                record_agent_workspace_pr_autofix_pre_start_failure(
                    Arc::clone(&github),
                    working_dir,
                    target.pr_number,
                    workspace_repo.as_ref(),
                    conversation_id,
                    health.auto_merge_request.is_some(),
                    &summary,
                )
                .await?;
                return Ok(false);
            }
        }
    } else {
        workspace_for_options.pr_auto_merge_current
    };
    let message = build_agent_workspace_pr_autofix_message(
        target.pr_number,
        target.pr_url.as_deref(),
        target.label(),
        &workspace_for_options,
        &issue,
    );
    dispatch_agent_workspace_pr_autofix(
        repair_repo,
        branch_update_repo,
        workspace_repo,
        agent_run_repo,
        chat_service,
        github,
        &health,
        working_dir,
        conversation_id,
        &workspace_for_options,
        &target,
        target.pr_number,
        &issue.classification,
        auto_merge_before_reservation,
        AgentWorkspacePrAutofixDispatch {
            repair_summary: &issue.summary,
            #[cfg(test)]
            publication_status: if target.updates_workspace_publication() {
                Some(if issue.kind == AgentWorkspacePrAutofixIssueKind::Review {
                    "changes_requested"
                } else {
                    "open"
                })
            } else {
                None
            },
            message,
            #[cfg(test)]
            audit_step: "pr_autofix",
            #[cfg(test)]
            audit_summary: issue.summary.clone(),
            dispatch_label: "PR autofix",
        },
    )
    .await
}

/// The step recorded when a fresh PR autofix streak is suppressed by cross-streak memory.
pub(crate) const CROSS_STREAK_FINGERPRINT_HOLD_STEP: &str = "repair_fingerprint_cross_streak_hold";
/// The step recorded when RalphX proves a PR's failing check already fails on the base branch.
pub(crate) const PRE_EXISTING_ON_BASE_DETECTED_STEP: &str = "repair_pre_existing_on_base_detected";

/// Records a base-caused failure as a hand-off rather than a repair.
///
/// Nothing here spends an agent. The publication event explains why supervision stopped, the
/// remembered fingerprint makes the cross-streak gate keep it stopped, and the Inbox notification
/// tells the user the fix belongs on the base branch — work this workspace cannot do.
async fn record_pre_existing_on_base_detection(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    classification: &str,
    health: &PrHealth,
    notification_service: Option<&Arc<NotificationService>>,
) -> crate::error::AppResult<bool> {
    let base_ref = health.sync_state.base_ref_name.trim();
    let summary = format!(
        "The failing checks on this PR already fail on {base_ref}. RalphX did not start a fixer \
         because the fix belongs on the base branch, not on this PR."
    );

    let already_recorded = workspace_repo
        .list_publication_events(conversation_id)
        .await?
        .into_iter()
        .any(|event| {
            event.step == PRE_EXISTING_ON_BASE_DETECTED_STEP
                && event.classification.as_deref() == Some(classification)
        });
    if already_recorded {
        return Ok(false);
    }

    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            PRE_EXISTING_ON_BASE_DETECTED_STEP,
            "blocked",
            &summary,
            Some(classification.to_string()),
        ))
        .await?;
    workspace_repo
        .set_last_blocked_pr_health_fingerprint(conversation_id, Some(classification))
        .await?;

    if let Some(notification_service) = notification_service {
        notification_service
            .record(NewNotification {
                project_id: Some(workspace.project_id.to_string()),
                category: NotificationCategory::TaskBlocked,
                severity: NotificationSeverity::ActionRequired,
                title: match workspace.publication_pr_number {
                    Some(pr_number) => format!("PR #{pr_number} is blocked by {base_ref}"),
                    None => format!("Workspace is blocked by {base_ref}"),
                },
                body: Some(summary),
                target: NotificationTarget {
                    kind: NotificationTargetKind::AgentConversation,
                    project_id: Some(workspace.project_id.to_string()),
                    task_id: None,
                    conversation_id: Some(conversation_id.to_string()),
                    setup_conversation_id: None,
                    automation_id: None,
                    run_id: None,
                },
                dedupe_key: Some(format!(
                    "repair_pre_existing_on_base:{}:{classification}",
                    conversation_id.as_str()
                )),
            })
            .await;
    }

    tracing::info!(
        conversation_id = conversation_id.as_str(),
        base_ref,
        classification,
        "PR failure already fails on base; handing off instead of dispatching a fixer"
    );
    Ok(true)
}

/// True only when GitHub proves the PR's failing checks already fail on the base branch tip.
///
/// This authorizes skipping an agent entirely, so it is deliberately conservative in one
/// direction: every ambiguity answers "no". An unreadable base, an unimplemented backend, a check
/// absent from the base (the scope-gated-CI case), and an in-progress base run all fall through to
/// the normal dispatch. Being wrong here means a wasted agent generation; being wrong the other
/// way means silently ignoring a real PR failure.
async fn pr_failures_already_fail_on_base(
    github: &dyn GithubServiceTrait,
    working_dir: &Path,
    health: &PrHealth,
) -> bool {
    let failing_pr_checks = health
        .checks
        .iter()
        .filter(|check| {
            check.conclusion.as_deref().is_some_and(|value| {
                matches!(value.to_ascii_uppercase().as_str(), "FAILURE" | "TIMED_OUT")
            })
        })
        .collect::<Vec<_>>();
    if failing_pr_checks.is_empty() {
        return false;
    }

    let base_ref = health.sync_state.base_ref_name.trim();
    if base_ref.is_empty() {
        return false;
    }
    let base_checks = match github
        .list_branch_check_conclusions(working_dir, base_ref)
        .await
    {
        Ok(Some(checks)) => checks,
        Ok(None) => return false,
        Err(error) => {
            tracing::warn!(
                base_ref,
                %error,
                "Could not read base branch check conclusions; dispatching PR autofix as usual"
            );
            return false;
        }
    };
    if base_checks.is_empty() {
        return false;
    }

    // Every failing check must be proven failing on base. One PR-caused failure means the PR does
    // own work, even if another check is broken upstream.
    failing_pr_checks.iter().all(|pr_check| {
        base_checks.iter().any(|base_check| {
            base_check.name.eq_ignore_ascii_case(pr_check.name.trim())
                && base_check.conclusion.as_deref().is_some_and(|value| {
                    matches!(value.to_ascii_uppercase().as_str(), "FAILURE" | "TIMED_OUT")
                })
        })
    })
}

/// A repair attempt's fingerprint hold dies with its streak. Once a streak exhausts its retries,
/// the next poll would otherwise start a brand new streak against the exact same failing check —
/// the outer loop that turned one unchanged failure into four Opus generations on 2026-07-31.
///
/// Returns `true` when this dispatch must be suppressed. Different health clears the memory so a
/// genuinely new failure is never held by a stale one. Repository errors propagate rather than
/// resolving to "not suppressed": an unreadable workspace cannot authorize spending an agent, and
/// the poll loop surfaces the failure instead of quietly starting another generation.
async fn cross_streak_fingerprint_suppresses_dispatch(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    classification: &str,
) -> crate::error::AppResult<bool> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(false);
    };
    let Some(remembered) = workspace.last_blocked_pr_health_fingerprint.as_deref() else {
        return Ok(false);
    };
    if remembered != classification {
        workspace_repo
            .set_last_blocked_pr_health_fingerprint(conversation_id, None)
            .await?;
        return Ok(false);
    }

    // Record the hold once. Repeating it every poll would bury the publication timeline.
    let already_recorded = workspace_repo
        .list_publication_events(conversation_id)
        .await?
        .into_iter()
        .any(|event| {
            event.step == CROSS_STREAK_FINGERPRINT_HOLD_STEP
                && event.classification.as_deref() == Some(classification)
        });
    if !already_recorded {
        workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                conversation_id.clone(),
                CROSS_STREAK_FINGERPRINT_HOLD_STEP,
                "blocked",
                "A previous repair streak already exhausted itself against this exact PR failure. \
                 RalphX is waiting for GitHub to report something different instead of starting \
                 another fixer generation.",
                Some(classification.to_string()),
            ))
            .await?;
    }
    tracing::info!(
        conversation_id = conversation_id.as_str(),
        classification,
        "Suppressing a fresh PR autofix streak against an already-exhausted failure identity"
    );
    Ok(true)
}

async fn record_agent_workspace_pr_autofix_pre_start_failure(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
    restore_auto_merge: bool,
    summary: &str,
) -> crate::AppResult<()> {
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;
    workspace_repo
        .update_publication(
            conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some("failed"),
        )
        .await?;
    update_agent_workspace_pr_supervision_state(
        workspace_repo,
        None,
        conversation_id,
        Some(false),
        Some("blocked"),
        Some(summary),
    )
    .await?;

    if !restore_auto_merge || !workspace.pr_auto_merge_desired {
        return Ok(());
    }

    let (auto_merge_current, final_summary) = match github
        .enable_pr_auto_merge(working_dir, pr_number, &workspace.pr_auto_merge_method)
        .await
    {
        Ok(()) => (true, format!("{summary} GitHub auto-merge was restored.")),
        Err(error) => (
            false,
            format!("{summary} {}", auto_merge_enable_failure_summary(&error)),
        ),
    };
    update_agent_workspace_pr_supervision_state(
        workspace_repo,
        None,
        conversation_id,
        Some(auto_merge_current),
        Some("blocked"),
        Some(&final_summary),
    )
    .await
}

async fn settle_agent_workspace_pr_autofix_dispatch_failure(
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
    branch_update_repo: Arc<dyn BranchUpdateRepository>,
    attempt: AgentWorkspaceRepairAttempt,
    summary: &str,
    settlement: AgentWorkspaceRepairDispatchSettlement,
    auto_merge_current: Option<bool>,
) -> crate::AppResult<()> {
    let _ = settle_agent_workspace_repair_dispatch_outcome(
        repair_repo,
        branch_update_repo,
        attempt,
        settlement,
        summary,
        auto_merge_current,
    )
    .await?;
    Ok(())
}

struct AgentWorkspacePrAutofixDispatch<'a> {
    repair_summary: &'a str,
    #[cfg(test)]
    publication_status: Option<&'a str>,
    message: String,
    #[cfg(test)]
    audit_step: &'static str,
    #[cfg(test)]
    audit_summary: String,
    dispatch_label: &'static str,
}

async fn dispatch_agent_workspace_pr_autofix(
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
    github: Arc<dyn GithubServiceTrait>,
    health: &PrHealth,
    working_dir: &Path,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    _target: &AgentWorkspacePrAutofixTarget,
    pr_number: i64,
    classification: &str,
    auto_merge_before_reservation: Option<bool>,
    dispatch: AgentWorkspacePrAutofixDispatch<'_>,
) -> crate::AppResult<bool> {
    let Some(repair_repo) = repair_repo else {
        #[cfg(test)]
        return dispatch_agent_workspace_pr_autofix_legacy(
            workspace_repo,
            agent_run_repo,
            chat_service,
            github,
            working_dir,
            conversation_id,
            workspace,
            _target,
            pr_number,
            classification,
            auto_merge_before_reservation,
            health.auto_merge_request.is_some(),
            dispatch,
        )
        .await;
        #[cfg(not(test))]
        return Err(AppError::Infrastructure(
            "PR autofix dispatch requires durable workspace repair authority".to_string(),
        ));
    };
    let Some(branch_update_repo) = branch_update_repo else {
        return Err(AppError::Infrastructure(
            "durable PR autofix dispatch requires canonical Git target authority".to_string(),
        ));
    };
    let preallocated_run_id = AgentRunId::new();
    let start = start_or_join_agent_workspace_repair(
        Arc::clone(&repair_repo),
        Arc::clone(&workspace_repo),
        AgentWorkspaceRepairStartRequest {
            conversation_id: conversation_id.clone(),
            source: AgentWorkspaceRepairSource::PrAutofix,
            continuation: AgentWorkspaceRepairContinuation::ResumePrSupervision,
            target_base_ref: workspace.base_ref.clone(),
            target_base_commit: workspace.base_commit.clone(),
            verified_newer_base: false,
            reason: dispatch.repair_summary.to_string(),
            summary: dispatch.repair_summary.to_string(),
            auto_merge_current: auto_merge_before_reservation,
            explicit_publish_requested: false,
            retry_blocked: false,
            carryover_pr_autofix_evidence: None,
        },
    )
    .await?;
    let mut attempt = match start {
        AgentWorkspaceRepairStartOutcome::Started(attempt) => attempt,
        AgentWorkspaceRepairStartOutcome::Joined(attempt) => {
            let recorded = record_agent_workspace_repair_routed_to_existing_attempt(
                workspace_repo.as_ref(),
                conversation_id,
                pr_number,
                "joined",
                "CI-failure",
                &attempt,
                dispatch.repair_summary,
            )
            .await?;
            if recorded {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "joined",
                    "PR CI-failure signal was routed to an existing workspace repair attempt"
                );
            } else {
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "joined",
                    "PR CI-failure signal remains routed to the existing workspace repair attempt"
                );
            }
            return Ok(false);
        }
        AgentWorkspaceRepairStartOutcome::SuccessorStarted(attempt) => {
            let recorded = record_agent_workspace_repair_routed_to_existing_attempt(
                workspace_repo.as_ref(),
                conversation_id,
                pr_number,
                "successor_started",
                "CI-failure",
                &attempt,
                dispatch.repair_summary,
            )
            .await?;
            if recorded {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "successor_started",
                    "PR CI-failure signal was routed to an existing workspace repair attempt"
                );
            } else {
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "successor_started",
                    "PR CI-failure signal remains routed to the existing workspace repair attempt"
                );
            }
            return Ok(false);
        }
        AgentWorkspaceRepairStartOutcome::BlockedByCurrent(attempt) => {
            let recorded = record_agent_workspace_repair_routed_to_existing_attempt(
                workspace_repo.as_ref(),
                conversation_id,
                pr_number,
                "blocked_by_current",
                "CI-failure",
                &attempt,
                dispatch.repair_summary,
            )
            .await?;
            if recorded {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "blocked_by_current",
                    "PR CI-failure signal was routed to an existing workspace repair attempt"
                );
            } else {
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    outcome = "blocked_by_current",
                    "PR CI-failure signal remains routed to the existing workspace repair attempt"
                );
            }
            return Ok(false);
        }
    };
    // Backend-derived dispatch evidence fences later success completion and suppression.
    attempt.pr_autofix_dispatch_head_commit = health.sync_state.head_ref_oid.clone();
    attempt.pr_autofix_health_fingerprint = Some(classification.to_string());
    let target_identity =
        GitService::canonical_target_identity(working_dir, &workspace.branch_name).await?;
    let dispatch_attempt = match reserve_agent_workspace_repair_dispatch(
        Arc::clone(&repair_repo),
        Arc::clone(&branch_update_repo),
        target_identity,
        attempt,
        preallocated_run_id.clone(),
        dispatch.repair_summary,
        auto_merge_before_reservation,
    )
    .await?
    {
        AgentWorkspaceRepairDispatchOutcome::Reserved(attempt) => attempt,
        AgentWorkspaceRepairDispatchOutcome::Stale(_)
        | AgentWorkspaceRepairDispatchOutcome::Missing => {
            return Ok(false);
        }
    };
    let canonical_target = async {
        let persisted = validate_agent_workspace_repair_target_lease(
            branch_update_repo.as_ref(),
            &dispatch_attempt,
        )
        .await?;
        let observed =
            GitService::canonical_target_identity(working_dir, &workspace.branch_name).await?;
        if observed != persisted {
            return Err(AppError::Conflict(
                "PR autofix workspace/ref differs from its durable dispatch target".to_string(),
            ));
        }
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(error) = canonical_target {
        let summary = format!(
            "{} reserved a durable worker, but its canonical target could not be revalidated: {error}",
            dispatch.dispatch_label
        );
        settle_agent_workspace_pr_autofix_dispatch_failure(
            Arc::clone(&repair_repo),
            Arc::clone(&branch_update_repo),
            dispatch_attempt,
            &summary,
            AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
            auto_merge_before_reservation,
        )
        .await?;
        return Ok(false);
    }
    let auto_merge_current = match prepare_agent_workspace_pr_repair_auto_merge_state(
        github,
        working_dir,
        pr_number,
        conversation_id,
        health,
        Arc::clone(&workspace_repo),
        Some(repair_repo.as_ref()),
    )
    .await
    {
        Ok(Some(auto_merge_current)) => auto_merge_current,
        Ok(None) => {
            settle_agent_workspace_pr_autofix_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch_attempt,
                "PR autofix reserved a durable worker, but GitHub auto-merge could not be disabled.",
                AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
                auto_merge_before_reservation,
            )
            .await?;
            return Ok(false);
        }
        Err(error) => {
            let summary = format!(
                "{} reserved a durable worker, but GitHub auto-merge preparation failed: {error}",
                dispatch.dispatch_label
            );
            settle_agent_workspace_pr_autofix_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch_attempt,
                &summary,
                AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
                auto_merge_before_reservation,
            )
            .await?;
            return Ok(false);
        }
    };
    let mut send_options =
        match agent_workspace_pr_fixer_send_options(workspace, working_dir, Some(agent_run_repo))
            .await
        {
            Ok(options) => options,
            Err(error) => {
                let summary = format!(
                    "{} could not prepare its reserved run for dispatch: {error}",
                    dispatch.dispatch_label
                );
                settle_agent_workspace_pr_autofix_dispatch_failure(
                    Arc::clone(&repair_repo),
                    Arc::clone(&branch_update_repo),
                    dispatch_attempt,
                    &summary,
                    AgentWorkspaceRepairDispatchSettlement::RetryableFailure,
                    Some(auto_merge_current),
                )
                .await?;
                return Ok(false);
            }
        };
    send_options.preallocated_agent_run_id = Some(preallocated_run_id.clone());
    send_options.queue_policy = SendQueuePolicy::RequireImmediateStart;
    send_options.metadata = Some(pr_autofix_action_metadata(pr_number, classification));

    let delivery = chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &dispatch.message,
            send_options,
        )
        .await;
    let settlement = classify_agent_workspace_repair_delivery(
        delivery.as_ref(),
        conversation_id,
        &preallocated_run_id,
    );
    let send_result = match delivery {
        Ok(result) if settlement == AgentWorkspaceRepairDispatchSettlement::Delivered => result,
        Ok(_) => {
            let summary = format!(
                "{} did not start immediately with its reserved run identity.",
                dispatch.dispatch_label
            );
            settle_agent_workspace_pr_autofix_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch_attempt,
                &summary,
                settlement,
                Some(auto_merge_current),
            )
            .await?;
            return Ok(false);
        }
        Err(error) => {
            let summary = format!("{} dispatch failed: {error}", dispatch.dispatch_label);
            settle_agent_workspace_pr_autofix_dispatch_failure(
                Arc::clone(&repair_repo),
                Arc::clone(&branch_update_repo),
                dispatch_attempt,
                &summary,
                settlement,
                Some(auto_merge_current),
            )
            .await?;
            return Ok(false);
        }
    };
    debug_assert_eq!(send_result.agent_run_id, preallocated_run_id.as_str());

    let _ = settle_agent_workspace_repair_dispatch_outcome(
        repair_repo,
        branch_update_repo,
        dispatch_attempt,
        AgentWorkspaceRepairDispatchSettlement::Delivered,
        dispatch.repair_summary,
        Some(auto_merge_current),
    )
    .await?;

    Ok(true)
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
async fn dispatch_agent_workspace_pr_autofix_legacy(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspacePrAutofixTarget,
    pr_number: i64,
    classification: &str,
    auto_merge_current: Option<bool>,
    restore_auto_merge: bool,
    dispatch: AgentWorkspacePrAutofixDispatch<'_>,
) -> crate::AppResult<bool> {
    let preallocated_run_id = AgentRunId::new();
    let Some(claim) = claim_agent_workspace_repair(
        Arc::clone(&workspace_repo),
        conversation_id,
        dispatch.repair_summary,
        auto_merge_current,
    )
    .await?
    else {
        return Ok(false);
    };
    let claim_is_authorized = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .is_some_and(|workspace| target.authorizes_claimed_repair(&workspace));
    if !claim_is_authorized {
        record_agent_workspace_pr_autofix_pre_start_failure(
            github,
            working_dir,
            pr_number,
            workspace_repo.as_ref(),
            conversation_id,
            restore_auto_merge,
            "PR autofix authorization changed after GitHub auto-merge was disabled.",
        )
        .await?;
        return Ok(false);
    }
    let mut send_options =
        match agent_workspace_pr_fixer_send_options(workspace, working_dir, Some(agent_run_repo))
            .await
        {
            Ok(options) => options,
            Err(error) => {
                let summary = format!(
                    "{} could not prepare its reserved run for dispatch: {error}",
                    dispatch.dispatch_label
                );
                let _ = settle_agent_workspace_repair_failure(
                    Arc::clone(&workspace_repo),
                    &claim,
                    &summary,
                )
                .await?;
                return Ok(false);
            }
        };
    send_options.preallocated_agent_run_id = Some(preallocated_run_id.clone());
    send_options.queue_policy = SendQueuePolicy::RequireImmediateStart;
    send_options.metadata = Some(pr_autofix_action_metadata(pr_number, classification));
    if let Some(pr_status) = dispatch.publication_status {
        if let Err(error) = workspace_repo
            .update_publication(
                conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                Some(pr_status),
                Some("needs_agent"),
            )
            .await
        {
            let summary = format!(
                "{} could not prepare workspace state for dispatch: {error}",
                dispatch.dispatch_label
            );
            let _ = settle_agent_workspace_repair_failure(
                Arc::clone(&workspace_repo),
                &claim,
                &summary,
            )
            .await?;
            return Ok(false);
        }
    }
    let send_failure = match chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &dispatch.message,
            send_options,
        )
        .await
    {
        Ok(result)
            if !result.was_queued
                && !result.queued_as_pending
                && result.conversation_id == conversation_id.as_str()
                && result.agent_run_id == preallocated_run_id.as_str() =>
        {
            None
        }
        Ok(_) => Some(format!(
            "{} did not start immediately with its reserved run identity.",
            dispatch.dispatch_label
        )),
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Legacy PR autofix dispatch failed"
            );
            Some(format!(
                "{} dispatch failed: {error}",
                dispatch.dispatch_label
            ))
        }
    };
    if let Some(summary) = send_failure {
        let _ =
            settle_agent_workspace_repair_failure(Arc::clone(&workspace_repo), &claim, &summary)
                .await?;
        return Ok(false);
    }
    if let Err(error) = workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            dispatch.audit_step,
            "needs_agent",
            dispatch.audit_summary,
            Some(classification.to_string()),
        ))
        .await
    {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            pr_number,
            error = %error,
            "Legacy PR autofix started but its audit event could not be recorded"
        );
    }
    Ok(true)
}

async fn agent_workspace_pr_autofix_repair_in_flight(
    workspace: &AgentConversationWorkspace,
    agent_run_repo: Option<&Arc<dyn AgentRunRepository>>,
) -> crate::AppResult<bool> {
    if workspace.publication_push_status.as_deref() == Some("needs_agent")
        && workspace.pr_supervision_status.as_deref() == Some("fixing")
    {
        return Ok(true);
    }
    if matches!(
        workspace.pr_supervision_status.as_deref(),
        Some("fixing" | "publishing")
    ) {
        if workspace.publication_push_status.as_deref() != Some("pushed") {
            return Ok(true);
        }
        let Some(agent_run_repo) = agent_run_repo else {
            return Ok(true);
        };
        return Ok(agent_run_repo
            .get_active_for_conversation(&workspace.conversation_id)
            .await?
            .is_some());
    }
    let Some(agent_run_repo) = agent_run_repo else {
        return Ok(false);
    };
    Ok(agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
        .is_some())
}

/// Shared by the poller's first dispatch and by durable redelivery so a recovered PR autofix keeps
/// the same recipient agent and the same provider/model/effort continuity as its first generation.
pub(crate) async fn agent_workspace_pr_fixer_send_options(
    workspace: &AgentConversationWorkspace,
    working_directory: &Path,
    agent_run_repo: Option<&Arc<dyn AgentRunRepository>>,
) -> crate::AppResult<SendMessageOptions> {
    let latest_run = match agent_run_repo {
        Some(repo) => {
            repo.get_latest_for_conversation(&workspace.conversation_id)
                .await?
        }
        None => None,
    };

    Ok(SendMessageOptions {
        conversation_id_override: Some(workspace.conversation_id.clone()),
        agent_name_override: Some(AGENT_WORKSPACE_PR_FIXER.to_string()),
        harness_override: latest_run.as_ref().and_then(|run| run.harness),
        model_override: latest_run.as_ref().and_then(|run| {
            run.logical_model
                .clone()
                .or_else(|| run.effective_model_id.clone())
        }),
        logical_effort_override: latest_run.as_ref().and_then(|run| run.logical_effort),
        service_tier_override: latest_run.as_ref().and_then(|run| run.service_tier.clone()),
        working_directory_override: Some(working_directory.to_path_buf()),
        force_new_provider_session: true,
        preserve_conversation_provider_session_ref: true,
        ..Default::default()
    })
}

#[cfg(test)]
async fn route_agent_workspace_pr_review_monitor_if_needed(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    route_agent_workspace_pr_review_monitor_if_needed_with_notifications(
        github,
        working_dir,
        pr_number,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        chat_service,
        None,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn route_agent_workspace_pr_review_monitor_if_needed_with_notifications(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    chat_service: Arc<dyn ChatService>,
    notification_service: Option<Arc<NotificationService>>,
    polled_health: Option<&PrHealth>,
) -> crate::AppResult<bool> {
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr {
        return Ok(false);
    }

    let Some(mut monitor) = workspace_repo
        .get_pr_review_monitor(conversation_id)
        .await?
    else {
        return Ok(false);
    };
    if monitor.pr_number != pr_number
        || !monitor.monitor_enabled
        || matches!(
            monitor.status,
            AgentWorkspacePrReviewMonitorStatus::Paused
                | AgentWorkspacePrReviewMonitorStatus::Terminal
                | AgentWorkspacePrReviewMonitorStatus::Submitting
        )
    {
        return Ok(false);
    }
    if agent_run_repo
        .get_active_for_conversation(conversation_id)
        .await?
        .is_some()
    {
        return Ok(false);
    }

    let health = resolve_polled_pr_health(&github, working_dir, pr_number, polled_health).await?;
    import_agent_workspace_pr_comment_evidence(
        Arc::clone(&workspace_repo),
        conversation_id,
        pr_number,
        &health,
    )
    .await?;
    let Some(head_sha) = health
        .sync_state
        .head_ref_oid
        .clone()
        .or_else(|| workspace.source_pull_request.as_ref()?.head_ref_oid.clone())
        .filter(|value| !value.trim().is_empty())
    else {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            pr_number,
            "Agent workspace PR poller: Review PR monitor could not resolve current head SHA"
        );
        return Ok(false);
    };

    if monitor.last_reviewed_head_sha.as_deref() == Some(head_sha.as_str()) {
        monitor.last_seen_head_sha = Some(head_sha);
        workspace_repo.upsert_pr_review_monitor(monitor).await?;
        return Ok(false);
    }
    if workspace_repo
        .get_pending_pr_review_action_for_head(conversation_id, pr_number, &head_sha)
        .await?
        .is_some()
    {
        monitor.last_seen_head_sha = Some(head_sha);
        workspace_repo.upsert_pr_review_monitor(monitor).await?;
        return Ok(false);
    }

    let superseded_action_ids = workspace_repo
        .supersede_pending_pr_review_actions_except_head(conversation_id, pr_number, &head_sha)
        .await?;
    if let Some(notification_service) = notification_service {
        for action_id in superseded_action_ids {
            notification_service
                .resolve_workflow_notification(&pr_review_notification_key(
                    conversation_id.as_str(),
                    &action_id,
                ))
                .await;
        }
    }

    monitor.status = AgentWorkspacePrReviewMonitorStatus::Reviewing;
    monitor.last_seen_head_sha = Some(head_sha.clone());
    monitor.last_error = None;
    workspace_repo.upsert_pr_review_monitor(monitor).await?;

    let message = build_agent_workspace_pr_monitor_review_message(pr_number, &workspace, &health);
    let send_result = match chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            SendMessageOptions {
                conversation_id_override: Some(workspace.conversation_id.clone()),
                agent_name_override: Some(
                    agent_name_for_workspace_mode(workspace.mode).to_string(),
                ),
                working_directory_override: Some(working_dir.to_path_buf()),
                ..Default::default()
            },
        )
        .await
    {
        Ok(result) => result,
        Err(error) => {
            if let Some(mut current) = workspace_repo
                .get_pr_review_monitor(conversation_id)
                .await?
            {
                current.last_error = Some(error.to_string());
                current.status = current.settlement_status();
                workspace_repo.upsert_pr_review_monitor(current).await?;
            }
            return Err(AppError::Infrastructure(error.to_string()));
        }
    };
    if let Some(mut current) = workspace_repo
        .get_pr_review_monitor(conversation_id)
        .await?
    {
        if current.monitor_enabled
            && current.status == AgentWorkspacePrReviewMonitorStatus::Reviewing
            && current.last_seen_head_sha.as_deref() == Some(head_sha.as_str())
        {
            current.last_review_run_id =
                (!send_result.agent_run_id.trim().is_empty()).then_some(send_result.agent_run_id);
            workspace_repo.upsert_pr_review_monitor(current).await?;
        }
    }
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_review_monitor",
            "reviewing",
            format!("Review PR monitor detected a new head on PR #{pr_number}."),
            Some(format!("github_pr_review_monitor:{pr_number}:{head_sha}")),
        ))
        .await?;

    Ok(true)
}

async fn workspace_review_auto_merge_guard_blocks_enable(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    conversation_id: &ChatConversationId,
) -> crate::AppResult<bool> {
    let monitor = workspace_repo
        .get_workspace_review_monitor(conversation_id)
        .await?;
    Ok(
        crate::application::agent_workspace_review_auto_merge::auto_merge_guard_blocks_enable(
            monitor.as_ref(),
        ),
    )
}

async fn sync_agent_workspace_auto_merge_preference(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    workspace: &AgentConversationWorkspace,
    health: &PrHealth,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    repair_repo: Option<&dyn AgentWorkspaceRepairRepository>,
) -> crate::AppResult<bool> {
    let remote_current = health.auto_merge_request.is_some();
    let mut current = remote_current;

    if workspace_review_auto_merge_guard_blocks_enable(
        workspace_repo.as_ref(),
        &workspace.conversation_id,
    )
    .await?
    {
        if remote_current {
            github.disable_pr_auto_merge(working_dir, pr_number).await?;
        }
        update_agent_workspace_pr_supervision_state(
            workspace_repo.as_ref(),
            repair_repo,
            &workspace.conversation_id,
            Some(false),
            Some("review_paused"),
            Some("GitHub auto-merge is paused while the workspace Review is authoritative."),
        )
        .await?;
        return Ok(false);
    }

    if workspace.pr_auto_merge_desired && !remote_current {
        let enable_result = async {
            if health.sync_state.is_draft {
                github.mark_pr_ready(working_dir, pr_number).await?;
            }
            github
                .enable_pr_auto_merge(working_dir, pr_number, &workspace.pr_auto_merge_method)
                .await
        }
        .await;

        match enable_result {
            Ok(()) => {
                current = true;
                update_agent_workspace_pr_supervision_state(
                    workspace_repo.as_ref(),
                    repair_repo,
                    &workspace.conversation_id,
                    Some(true),
                    Some("monitoring"),
                    Some("GitHub auto-merge is enabled; RalphX is monitoring PR health."),
                )
                .await?;
            }
            Err(error) => {
                update_agent_workspace_pr_supervision_state(
                    workspace_repo.as_ref(),
                    repair_repo,
                    &workspace.conversation_id,
                    Some(false),
                    Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                    Some(&auto_merge_enable_failure_summary(&error)),
                )
                .await?;
            }
        }
    } else if !workspace.pr_auto_merge_desired && remote_current {
        match github.disable_pr_auto_merge(working_dir, pr_number).await {
            Ok(()) => {
                current = false;
                update_agent_workspace_pr_supervision_state(
                    workspace_repo.as_ref(),
                    repair_repo,
                    &workspace.conversation_id,
                    Some(false),
                    None,
                    Some("GitHub auto-merge is disabled."),
                )
                .await?;
            }
            Err(error) => {
                update_agent_workspace_pr_supervision_state(
                    workspace_repo.as_ref(),
                    repair_repo,
                    &workspace.conversation_id,
                    Some(true),
                    Some(AUTO_MERGE_SUPERVISION_STATUS_WAITING),
                    Some(&auto_merge_disable_failure_summary(&error)),
                )
                .await?;
            }
        }
    } else if workspace.pr_auto_merge_current != Some(remote_current) {
        update_agent_workspace_pr_supervision_state(
            workspace_repo.as_ref(),
            repair_repo,
            &workspace.conversation_id,
            Some(remote_current),
            None,
            None,
        )
        .await?;
    }

    Ok(current)
}

pub async fn sync_agent_workspace_auto_merge_preference_for_workspace(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    workspace: &AgentConversationWorkspace,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    repair_repo: Arc<dyn AgentWorkspaceRepairRepository>,
) -> crate::AppResult<bool> {
    if !workspace.allows_owned_pr_mutation() {
        let message = if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
            "GitHub auto-merge synchronization is unavailable in Review PR mode"
        } else {
            "GitHub auto-merge synchronization is unavailable for this workspace"
        };
        return Err(AppError::Validation(message.to_string()));
    }
    let health = github.fetch_pr_health(working_dir, pr_number).await?;
    sync_agent_workspace_auto_merge_preference(
        github,
        working_dir,
        pr_number,
        workspace,
        &health,
        workspace_repo,
        Some(repair_repo.as_ref()),
    )
    .await
}

pub(crate) fn classify_agent_workspace_pr_autofix_issue(
    pr_number: i64,
    health: &PrHealth,
) -> Option<AgentWorkspacePrAutofixIssue> {
    let review_decision = health
        .review_decision
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if review_decision == "changes_requested" {
        return Some(agent_workspace_pr_review_issue(pr_number, health));
    }

    let failing_checks: Vec<String> = health
        .checks
        .iter()
        .filter(|check| agent_workspace_check_is_failing(check))
        .map(format_agent_workspace_health_check)
        .collect();
    if !failing_checks.is_empty() {
        return Some(AgentWorkspacePrAutofixIssue {
            kind: AgentWorkspacePrAutofixIssueKind::Checks,
            summary: format!(
                "PR #{pr_number} has {} failing check{}",
                failing_checks.len(),
                if failing_checks.len() == 1 { "" } else { "s" }
            ),
            classification: agent_workspace_pr_autofix_event_classification(
                pr_number,
                health,
                "checks",
                &failing_checks,
            ),
            details: failing_checks,
        });
    }

    let mut mergeability_details = Vec::new();
    match health.sync_state.merge_state_status.as_ref() {
        Some(PrMergeStateStatus::Behind) => {
            mergeability_details.push("PR branch is behind its base".to_string());
        }
        Some(PrMergeStateStatus::Dirty) => {
            mergeability_details.push("PR branch has merge conflicts".to_string());
        }
        Some(PrMergeStateStatus::Blocked) => {
            // GitHub uses BLOCKED for branch-protection waits such as pending
            // required checks or reviews. Route only the concrete signals above
            // or explicit conflicts below.
        }
        _ => {}
    }
    if matches!(
        health.sync_state.mergeable.as_ref(),
        Some(PrMergeableState::Conflicting)
    ) {
        mergeability_details.push("PR is reported as conflicting".to_string());
    }
    if !mergeability_details.is_empty() {
        return Some(AgentWorkspacePrAutofixIssue {
            kind: AgentWorkspacePrAutofixIssueKind::Mergeability,
            summary: format!("PR #{pr_number} has mergeability blockers"),
            classification: agent_workspace_pr_autofix_event_classification(
                pr_number,
                health,
                "mergeability",
                &mergeability_details,
            ),
            details: mergeability_details,
        });
    }

    None
}

fn agent_workspace_pr_review_issue(
    pr_number: i64,
    health: &PrHealth,
) -> AgentWorkspacePrAutofixIssue {
    let details = vec!["GitHub review decision is CHANGES_REQUESTED".to_string()];
    AgentWorkspacePrAutofixIssue {
        kind: AgentWorkspacePrAutofixIssueKind::Review,
        summary: format!("PR #{pr_number} has requested changes"),
        classification: agent_workspace_pr_autofix_event_classification(
            pr_number, health, "review", &details,
        ),
        details,
    }
}

fn agent_workspace_pr_health_has_head(health: &PrHealth) -> bool {
    health
        .sync_state
        .head_ref_oid
        .as_deref()
        .is_some_and(|head| !head.trim().is_empty())
}

fn agent_workspace_pr_merge_conflict_details(health: &PrHealth) -> Vec<String> {
    let mut details = Vec::new();
    if matches!(
        health.sync_state.merge_state_status.as_ref(),
        Some(PrMergeStateStatus::Dirty)
    ) {
        details.push("PR branch has merge conflicts".to_string());
    }
    if matches!(
        health.sync_state.mergeable.as_ref(),
        Some(PrMergeableState::Conflicting)
    ) {
        details.push("PR is reported as conflicting".to_string());
    }
    details
}

fn agent_workspace_pr_conflict_summary(pr_number: i64, details: &[String]) -> String {
    if details.is_empty() {
        return format!("PR #{pr_number} has merge conflicts.");
    }
    format!(
        "PR #{pr_number} has merge conflicts. GitHub reports: {}.",
        details.join("; ")
    )
}

fn agent_workspace_summary_is_merge_conflict(pr_number: i64, summary: Option<&str>) -> bool {
    let Some(summary) = summary else {
        return false;
    };
    let normalized = summary.trim().to_ascii_lowercase();
    normalized.contains(&format!("pr #{pr_number}"))
        && (normalized.contains("merge conflict") || normalized.contains("conflicting"))
}

fn agent_workspace_terminal_status_from_pr_health(
    health: &PrHealth,
) -> Option<(&'static str, &'static str)> {
    match &health.sync_state.status {
        PrStatus::Merged { .. } => Some(("merged", "Pull request merged")),
        PrStatus::Closed => Some(("closed", "Pull request closed without merging")),
        PrStatus::Open => None,
    }
}

pub(crate) async fn import_agent_workspace_pr_comment_evidence(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
    health: &PrHealth,
) -> crate::AppResult<()> {
    let comments = health
        .issue_comments
        .iter()
        .filter(|comment| !comment.id.trim().is_empty())
        .map(|comment| {
            AgentWorkspacePrCommentEvidenceUpsert::new(
                pr_number,
                comment.id.clone(),
                comment.author.clone(),
                comment.body.clone(),
                comment.url.clone(),
                comment.created_at.clone(),
                comment.updated_at.clone(),
                comment.is_codecov,
                comment.is_bot,
            )
        })
        .collect::<Vec<_>>();
    workspace_repo
        .upsert_pr_comment_evidence(conversation_id, comments)
        .await
}

fn agent_workspace_check_is_failing(check: &PrHealthCheck) -> bool {
    check
        .conclusion
        .as_deref()
        .or(check.status.as_deref())
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "failure"
                    | "failed"
                    | "error"
                    | "cancelled"
                    | "canceled"
                    | "timed_out"
                    | "timedout"
                    | "action_required"
                    | "startup_failure"
                    | "stale"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
fn compact_pr_feedback_text(body: &str, max_chars: usize) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let truncated: String = compact.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}...")
}

fn format_agent_workspace_health_check(check: &PrHealthCheck) -> String {
    let status = check
        .conclusion
        .as_deref()
        .or(check.status.as_deref())
        .unwrap_or("unknown");
    match check.details_url.as_deref() {
        Some(url) if !url.trim().is_empty() => format!("{} ({status}) - {url}", check.name),
        _ => format!("{} ({status})", check.name),
    }
}

fn agent_workspace_pr_autofix_event_classification(
    pr_number: i64,
    health: &PrHealth,
    kind: &str,
    details: &[String],
) -> String {
    let head = health
        .sync_state
        .head_ref_oid
        .as_deref()
        .unwrap_or("unknown-head");
    let mut hasher = Sha256::new();
    hasher.update(pr_number.to_string());
    hasher.update(b"\0");
    hasher.update(head);
    hasher.update(b"\0");
    hasher.update(kind);
    for detail in details {
        hasher.update(b"\0");
        hasher.update(detail);
    }
    let digest = format!("{:x}", hasher.finalize());
    let head_short: String = head
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect();
    format!(
        "github_pr_autofix:{pr_number}:{}:{}",
        if head_short.is_empty() {
            "unknown"
        } else {
            head_short.as_str()
        },
        &digest[..16]
    )
}

fn agent_workspace_pr_conflict_event_classification(
    pr_number: i64,
    health: &PrHealth,
    details: &[String],
) -> String {
    let head = health
        .sync_state
        .head_ref_oid
        .as_deref()
        .unwrap_or("unknown-head");
    let mut hasher = Sha256::new();
    hasher.update(pr_number.to_string());
    hasher.update(b"\0");
    hasher.update(head);
    for detail in details {
        hasher.update(b"\0");
        hasher.update(detail);
    }
    let digest = format!("{:x}", hasher.finalize());
    let head_short: String = head
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect();
    format!(
        "github_pr_conflict:{pr_number}:{}:{}",
        if head_short.is_empty() {
            "unknown"
        } else {
            head_short.as_str()
        },
        &digest[..16]
    )
}

#[cfg(test)]
fn agent_workspace_pr_conflict_repair_event_classification(
    pr_number: i64,
    health: &PrHealth,
    details: &[String],
) -> String {
    let head = health
        .sync_state
        .head_ref_oid
        .as_deref()
        .unwrap_or("unknown-head");
    let mut hasher = Sha256::new();
    hasher.update(pr_number.to_string());
    hasher.update(b"\0");
    hasher.update(head);
    hasher.update(b"\0repair");
    for detail in details {
        hasher.update(b"\0");
        hasher.update(detail);
    }
    let digest = format!("{:x}", hasher.finalize());
    let head_short: String = head
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect();
    format!(
        "github_pr_conflict_repair:{pr_number}:{}:{}",
        if head_short.is_empty() {
            "unknown"
        } else {
            head_short.as_str()
        },
        &digest[..16]
    )
}

fn build_agent_workspace_pr_conflict_repair_message(
    pr_number: i64,
    workspace: &AgentConversationWorkspace,
    details: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("Update from base failed for this agent workspace.\n\n");
    out.push_str("Please fix the workspace so the base update can be completed.\n");
    out.push_str("After the repair is committed, call `complete_agent_workspace_repair` with a concise summary. If the repair cannot be completed safely, call it with a concise summary and blocker.\n\n");
    out.push_str(&format!(
        "Error: PR #{pr_number} has merge conflicts. GitHub reports: {}.\n",
        details.join("; ")
    ));
    out.push_str(&format!("Workspace branch: {}\n", workspace.branch_name));
    if let Some(pr_url) = workspace.publication_pr_url.as_deref() {
        out.push_str(&format!("Pull request: {pr_url}\n"));
    }
    out
}

pub(crate) fn build_agent_workspace_pr_autofix_message(
    pr_number: i64,
    pr_url: Option<&str>,
    target_label: &str,
    workspace: &AgentConversationWorkspace,
    issue: &AgentWorkspacePrAutofixIssue,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "RalphX PR supervision detected a merge-blocking issue on GitHub PR #{pr_number} for this {target_label}.\n\n"
    ));
    out.push_str(
        "Please fix the PR in the current workspace branch, commit any focused changes, then call `complete_agent_workspace_pr_fix`.\n\n",
    );
    out.push_str(&format!(
        "Conversation ID: {}\n",
        workspace.conversation_id.as_str()
    ));
    out.push_str(&format!("Workspace branch: {}\n", workspace.branch_name));
    if let Some(pr_url) = pr_url {
        out.push_str(&format!("Pull request: {pr_url}\n"));
    }
    out.push_str(&format!("Detected issue: {}\n", issue.summary));
    out.push_str(&format!("Fingerprint: {}\n", issue.classification));

    if !issue.details.is_empty() {
        out.push_str("\nDetails:\n");
        for detail in &issue.details {
            out.push_str("- ");
            out.push_str(detail);
            out.push('\n');
        }
    }

    out.push_str(
        "\nStart by calling `get_agent_workspace_pr_fix_context` with the conversation ID above.",
    );
    out
}

fn build_agent_workspace_pr_monitor_review_message(
    pr_number: i64,
    workspace: &AgentConversationWorkspace,
    health: &PrHealth,
) -> String {
    let head_sha = health
        .sync_state
        .head_ref_oid
        .as_deref()
        .unwrap_or("unknown");
    let mut out = String::new();
    out.push_str(&format!(
        "Review PR monitor detected new changes on GitHub PR #{pr_number}.\n\n"
    ));
    out.push_str(
        "Please perform a fresh local code review of this PR in the current workspace. Write the versioned Review artifact for the current PR head before proposing any GitHub review action.\n\n",
    );
    out.push_str(&format!(
        "Conversation ID: {}\n",
        workspace.conversation_id.as_str()
    ));
    out.push_str(&format!("Workspace branch: {}\n", workspace.branch_name));
    if let Some(pr_url) = workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.url.as_deref())
        .or(workspace.publication_pr_url.as_deref())
    {
        out.push_str(&format!("Pull request: {pr_url}\n"));
    }
    out.push_str(&format!("Current head SHA: {head_sha}\n"));
    out.push_str("\nStart by inspecting the PR context, then create or update the Review artifact for this head before proposing Request Changes, Approve PR, Comment, or no action.");
    out
}

#[cfg(test)]
async fn route_agent_workspace_review_feedback_if_present(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    route_agent_workspace_review_feedback_if_present_with_repair_repo(
        github,
        working_dir,
        pr_number,
        conversation_id,
        workspace_repo,
        agent_run_repo,
        None,
        None,
        chat_service,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn route_agent_workspace_review_feedback_if_present_with_repair_repo(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Option<Arc<dyn AgentRunRepository>>,
    repair_repo: Option<Arc<dyn AgentWorkspaceRepairRepository>>,
    branch_update_repo: Option<Arc<dyn BranchUpdateRepository>>,
    chat_service: Arc<dyn ChatService>,
    polled_health: Option<&PrHealth>,
) -> crate::AppResult<bool> {
    let workspace = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            ))
        })?;
    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        return Ok(false);
    }
    let Some(target) = AgentWorkspacePrAutofixTarget::review_feedback(&workspace, pr_number) else {
        return Ok(false);
    };
    if repair_repo.is_none() {
        #[cfg(not(test))]
        return Err(AppError::Infrastructure(
            "PR review-feedback dispatch requires durable workspace repair authority".to_string(),
        ));
    }
    if repair_repo.is_some() && branch_update_repo.is_none() {
        return Err(AppError::Infrastructure(
            "durable PR review-feedback dispatch requires canonical Git target authority"
                .to_string(),
        ));
    }

    let Some(feedback) = github
        .check_pr_review_feedback(working_dir, pr_number)
        .await?
    else {
        return Ok(false);
    };

    if authorize_agent_workspace_pr_autofix(workspace_repo.as_ref(), conversation_id, &target)
        .await?
        .is_none()
    {
        return Ok(false);
    }

    let health = resolve_polled_pr_health(&github, working_dir, pr_number, polled_health).await?;
    let issue = agent_workspace_pr_review_issue(pr_number, &health);
    if !agent_workspace_pr_health_has_head(&health) {
        update_agent_workspace_pr_supervision_state(
            workspace_repo.as_ref(),
            repair_repo.as_deref(),
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("blocked"),
            Some("PR autofix is blocked because GitHub did not report the current head commit."),
        )
        .await?;
        return Ok(false);
    }
    let Some(agent_run_repo) = agent_run_repo.as_ref() else {
        tracing::error!(
            conversation_id = conversation_id.as_str(),
            pr_number,
            "Agent workspace PR review autofix requires an AgentRun repository"
        );
        return Ok(false);
    };
    if repair_repo.is_none() {
        let legacy_classification =
            agent_workspace_review_event_classification(&feedback.review_id);
        let legacy_event_exists = workspace_repo
            .list_publication_events(conversation_id)
            .await?
            .iter()
            .any(|event| {
                matches!(
                    event.classification.as_deref(),
                    Some(value)
                        if value == issue.classification.as_str()
                            || value == legacy_classification.as_str()
                )
            });
        let attempt_decision = load_pr_autofix_attempt_decision(
            agent_run_repo.as_ref(),
            conversation_id,
            pr_number,
            &issue.classification,
            legacy_event_exists,
        )
        .await?;
        if !attempt_decision.allows_start() {
            if let Some(summary) = attempt_decision.manual_summary() {
                update_agent_workspace_pr_supervision_state(
                    workspace_repo.as_ref(),
                    repair_repo.as_deref(),
                    conversation_id,
                    workspace.pr_auto_merge_current,
                    Some("blocked"),
                    Some(summary),
                )
                .await?;
            }
            return Ok(false);
        }
    }
    let repair_summary = "GitHub requested changes routed to the PR fixer.";
    #[cfg(test)]
    let summary = format!(
        "GitHub PR #{pr_number} requested changes from @{}",
        feedback.author
    );
    let Some(workspace_for_dispatch) =
        authorize_agent_workspace_pr_autofix(workspace_repo.as_ref(), conversation_id, &target)
            .await?
    else {
        return Ok(false);
    };
    // See the matching PR-health route: legacy test dispatch has no durable reservation seam.
    let auto_merge_before_reservation = if repair_repo.is_none() {
        let Some(auto_merge_current) = prepare_agent_workspace_pr_repair_auto_merge_state(
            Arc::clone(&github),
            working_dir,
            pr_number,
            conversation_id,
            &health,
            Arc::clone(&workspace_repo),
            repair_repo.as_deref(),
        )
        .await?
        else {
            return Ok(false);
        };
        Some(auto_merge_current)
    } else {
        workspace_for_dispatch.pr_auto_merge_current
    };
    dispatch_agent_workspace_pr_autofix(
        repair_repo,
        branch_update_repo,
        workspace_repo,
        agent_run_repo,
        chat_service,
        github,
        &health,
        working_dir,
        conversation_id,
        &workspace_for_dispatch,
        &target,
        pr_number,
        &issue.classification,
        auto_merge_before_reservation,
        AgentWorkspacePrAutofixDispatch {
            repair_summary,
            #[cfg(test)]
            publication_status: Some("changes_requested"),
            message: build_agent_workspace_pr_review_message(
                pr_number,
                &workspace_for_dispatch,
                &feedback,
            ),
            #[cfg(test)]
            audit_step: "github_review",
            #[cfg(test)]
            audit_summary: summary,
            dispatch_label: "PR review autofix",
        },
    )
    .await
}

fn agent_workspace_review_event_classification(review_id: &str) -> String {
    format!("github_pr_review:{review_id}")
}

fn build_agent_workspace_pr_review_message(
    pr_number: i64,
    workspace: &AgentConversationWorkspace,
    feedback: &PrReviewFeedback,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "GitHub PR #{pr_number} requested changes for this agent workspace.\n\n"
    ));
    out.push_str(
        "Please address this review in the current workspace branch, commit any fixes if needed, and report what changed.\n\n",
    );
    if workspace.pr_autofix_enabled {
        out.push_str(
            "Start by calling `get_agent_workspace_pr_fix_context`; after committing the fix, call `complete_agent_workspace_pr_fix`.\n\n",
        );
        out.push_str(&format!(
            "Conversation ID: {}\n",
            workspace.conversation_id.as_str()
        ));
    }
    out.push_str(&format!("Review author: @{}\n", feedback.author));
    if let Some(submitted_at) = feedback.submitted_at.as_deref() {
        out.push_str(&format!("Submitted: {submitted_at}\n"));
    }
    out.push_str(&format!("GitHub review id: {}\n", feedback.review_id));
    out.push_str(&format!("Workspace branch: {}\n", workspace.branch_name));

    if let Some(body) = feedback
        .body
        .as_deref()
        .filter(|body| !body.trim().is_empty())
    {
        out.push_str("\nReview body:\n");
        out.push_str(body.trim());
        out.push('\n');
    }

    if !feedback.comments.is_empty() {
        out.push_str("\nInline comments:\n");
        for comment in &feedback.comments {
            out.push_str(&format_agent_workspace_review_comment(comment));
        }
    }

    out
}

fn format_agent_workspace_review_comment(comment: &PrReviewCommentFeedback) -> String {
    let location = match (comment.path.as_deref(), comment.line) {
        (Some(path), Some(line)) => format!("{path}:{line}"),
        (Some(path), None) => path.to_string(),
        (None, Some(line)) => format!("line {line}"),
        (None, None) => "inline comment".to_string(),
    };
    format!(
        "- @{} on {}: {}\n",
        comment.author,
        location,
        comment.body.trim()
    )
}

#[cfg(test)]
#[path = "pr_merge_poller_tests.rs"]
mod tests;
