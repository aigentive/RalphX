// PR merge poller registry (AD1, AD9, AD11, AD18)
//
// Manages background polling tasks that watch GitHub PRs until they are merged,
// then trigger the existing post_merge_cleanup pipeline.
//
// Phase 3: Full poll_loop implementation with adaptive polling, rate limiting,
// crash recovery, and cancellation.

use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

use crate::application::agent_conversation_workspace::agent_name_for_workspace_mode;
use crate::application::chat_service::{ChatService, SendMessageOptions};
use crate::application::task_transition_service::PrBranchFreshnessOutcome;
use crate::application::TaskTransitionService;
use crate::domain::entities::plan_branch::PrStatus as DbPrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, ChatContextType,
    ChatConversationId,
};
use crate::domain::entities::{InternalStatus, PlanBranchId, Project, TaskId};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, PlanBranchRepository};
use crate::domain::services::github_service::{PrReviewCommentFeedback, PrReviewFeedback};
use crate::domain::services::{GithubServiceTrait, PrStatus};
use crate::error::AppError;

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

    /// GitHub service for PR status checks. None when GitHub integration is disabled.
    github_service: Option<Arc<dyn GithubServiceTrait>>,

    /// Plan branch repository for reading/updating branch metadata.
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
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
            github_service,
            plan_branch_repo,
        }
    }

    pub fn start_agent_workspace_polling(
        &self,
        conversation_id: ChatConversationId,
        pr_number: i64,
        project: Project,
        working_dir: PathBuf,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        chat_service: Arc<dyn ChatService>,
    ) {
        use dashmap::mapref::entry::Entry;

        if let Some(handle) = self.workspace_active.get(&conversation_id) {
            if !handle.is_finished() {
                tracing::debug!(
                    conversation_id = conversation_id.as_str(),
                    pr_number,
                    "start_agent_workspace_polling: already polling, skipping"
                );
                return;
            }
        }
        self.workspace_active.remove(&conversation_id);

        let Some(github) = self.github_service.as_ref().map(Arc::clone) else {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                "start_agent_workspace_polling: github_service is None — skipping"
            );
            return;
        };

        let active = Arc::clone(&self.workspace_active);
        let stopping = Arc::clone(&self.workspace_stopping);
        let semaphore = Arc::clone(&self.semaphore);
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
                workspace_repo,
                chat_service,
            )
            .await;
        });

        match self.workspace_active.entry(conversation_id) {
            Entry::Vacant(vacant) => {
                vacant.insert(handle);
            }
            Entry::Occupied(_) => {
                handle.abort();
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
        transition_service: Arc<TaskTransitionService<tauri::Wry>>,
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
        transition_service: Arc<TaskTransitionService<tauri::Wry>>,
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

    pub async fn process_agent_workspace_review_feedback_once(
        &self,
        conversation_id: &ChatConversationId,
        pr_number: i64,
        working_dir: &Path,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
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
            chat_service,
        )
        .await
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
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    transition_service: Arc<TaskTransitionService<tauri::Wry>>,
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
            Ok(PrStatus::Merged { merge_commit_sha }) => {
                // Release semaphore before potentially-long fetch operation
                drop(_permit);

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

async fn agent_workspace_poll_loop(
    conversation_id: ChatConversationId,
    pr_number: i64,
    project: Project,
    working_dir: PathBuf,
    github: Arc<dyn GithubServiceTrait>,
    active: Arc<DashMap<ChatConversationId, JoinHandle<()>>>,
    stopping: Arc<DashMap<ChatConversationId, ()>>,
    semaphore: Arc<tokio::sync::Semaphore>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    chat_service: Arc<dyn ChatService>,
) {
    let interval = Duration::from_secs(60);
    let mut first_poll = true;

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

        match github.check_pr_status(&working_dir, pr_number).await {
            Ok(PrStatus::Merged { .. }) => {
                drop(permit);
                let _ = mark_agent_workspace_pr_terminal(
                    Arc::clone(&workspace_repo),
                    &conversation_id,
                    "merged",
                    "Pull request merged",
                )
                .await;
                cleanup_terminal_agent_workspace_after_pr(
                    Arc::clone(&workspace_repo),
                    &conversation_id,
                    &project,
                    Some(Arc::clone(&github)),
                    true,
                )
                .await;
                active.remove(&conversation_id);
                stopping.remove(&conversation_id);
                return;
            }
            Ok(PrStatus::Closed) => {
                drop(permit);
                let _ = mark_agent_workspace_pr_terminal(
                    Arc::clone(&workspace_repo),
                    &conversation_id,
                    "closed",
                    "Pull request closed without merging",
                )
                .await;
                cleanup_terminal_agent_workspace_after_pr(
                    Arc::clone(&workspace_repo),
                    &conversation_id,
                    &project,
                    None,
                    false,
                )
                .await;
                active.remove(&conversation_id);
                stopping.remove(&conversation_id);
                return;
            }
            Ok(PrStatus::Open) => {
                drop(permit);
                if let Err(error) =
                    mark_agent_workspace_pr_open(Arc::clone(&workspace_repo), &conversation_id)
                        .await
                {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        pr_number,
                        error = %error,
                        "Agent workspace PR poller: failed to mark PR open"
                    );
                }

                match route_agent_workspace_review_feedback_if_present(
                    Arc::clone(&github),
                    &working_dir,
                    pr_number,
                    &conversation_id,
                    Arc::clone(&workspace_repo),
                    Arc::clone(&chat_service),
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
                        tracing::warn!(
                            conversation_id = conversation_id.as_str(),
                            pr_number,
                            error = %error,
                            "Agent workspace PR poller: failed to inspect GitHub review feedback"
                        );
                    }
                }
            }
            Err(error) => {
                drop(permit);
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

async fn agent_workspace_pr_polling_should_continue(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    pr_number: i64,
) -> bool {
    match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace)) if agent_workspace_pr_polling_is_current(&workspace, pr_number) => true,
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

fn agent_workspace_pr_polling_is_current(
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
) -> bool {
    workspace.status == AgentConversationWorkspaceStatus::Active
        && workspace.mode == AgentConversationWorkspaceMode::Edit
        && workspace.linked_plan_branch_id.is_none()
        && workspace.publication_pr_number == Some(pr_number)
        && matches!(
            workspace.publication_push_status.as_deref(),
            None | Some("pushed")
        )
        && !matches!(
            workspace.publication_pr_status.as_deref(),
            Some("closed") | Some("merged")
        )
}

async fn mark_agent_workspace_pr_open(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
) -> crate::AppResult<()> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(());
    };

    if workspace.publication_pr_status.as_deref() == Some("open")
        && workspace.publication_push_status.as_deref() == Some("pushed")
    {
        return Ok(());
    }

    workspace_repo
        .update_publication(
            conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            Some("open"),
            Some("pushed"),
        )
        .await
}

async fn mark_agent_workspace_pr_terminal(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    status: &str,
    summary: &str,
) -> crate::AppResult<()> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(());
    };

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
        .await
}

async fn cleanup_terminal_agent_workspace_after_pr(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    project: &Project,
    github: Option<Arc<dyn GithubServiceTrait>>,
    delete_branch_if_merged: bool,
) {
    let workspace = match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(conversation_id = conversation_id.as_str(), error = %error, "Agent workspace PR cleanup: failed to load workspace");
            return;
        }
    };

    if delete_branch_if_merged {
        if let Some(github) = github {
            if let Err(error) = github
                .fetch_remote(Path::new(&project.working_directory), &workspace.base_ref)
                .await
            {
                tracing::warn!(conversation_id = conversation_id.as_str(), base_ref = workspace.base_ref.as_str(), error = %error, "Agent workspace PR cleanup: failed to fetch base before local branch cleanup");
            }
        }
    }

    match crate::application::git_artifact_cleanup::cleanup_terminal_agent_workspace_local_artifacts(
        project,
        &workspace,
        delete_branch_if_merged,
    )
    .await
    {
        Ok(report) => {
            tracing::info!(conversation_id = conversation_id.as_str(), worktree_removed = report.worktree_removed, branch_deleted = report.branch_deleted, skipped_reason = report.skipped_reason.as_deref(), "Agent workspace PR cleanup: local artifact cleanup completed");
        }
        Err(error) => {
            tracing::warn!(conversation_id = conversation_id.as_str(), error = %error, "Agent workspace PR cleanup: local artifact cleanup failed (non-fatal)");
        }
    }
}

async fn route_review_feedback_if_present(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    task_id: &TaskId,
    transition_service: Arc<TaskTransitionService<tauri::Wry>>,
    history_actor: &str,
) -> crate::AppResult<bool> {
    let Some(feedback) = github
        .check_pr_review_feedback(working_dir, pr_number)
        .await?
    else {
        return Ok(false);
    };

    transition_service
        .route_github_pr_changes_requested(task_id, pr_number, feedback, history_actor)
        .await?;
    Ok(true)
}

async fn route_agent_workspace_review_feedback_if_present(
    github: Arc<dyn GithubServiceTrait>,
    working_dir: &Path,
    pr_number: i64,
    conversation_id: &ChatConversationId,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    chat_service: Arc<dyn ChatService>,
) -> crate::AppResult<bool> {
    let Some(feedback) = github
        .check_pr_review_feedback(working_dir, pr_number)
        .await?
    else {
        return Ok(false);
    };

    let classification = agent_workspace_review_event_classification(&feedback.review_id);
    let already_routed = workspace_repo
        .list_publication_events(conversation_id)
        .await?
        .into_iter()
        .any(|event| event.classification.as_deref() == Some(classification.as_str()));
    if already_routed {
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

    let message = build_agent_workspace_pr_review_message(pr_number, &workspace, &feedback);
    chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &message,
            SendMessageOptions {
                conversation_id_override: Some(workspace.conversation_id),
                agent_name_override: Some(
                    agent_name_for_workspace_mode(workspace.mode).to_string(),
                ),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;

    workspace_repo
        .update_publication(
            conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            Some("changes_requested"),
            Some("needs_agent"),
        )
        .await?;
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "github_review",
            "needs_agent",
            format!(
                "GitHub PR #{pr_number} requested changes from @{}",
                feedback.author
            ),
            Some(classification),
        ))
        .await?;

    Ok(true)
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
