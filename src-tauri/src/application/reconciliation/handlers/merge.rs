// Merge-specific reconciliation handlers: Merging, PendingMerge, MergeIncomplete, MergeConflict.

use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::application::git_service::git_cmd;
use crate::application::harness_runtime_registry::{
    default_reconciliation_attempt_merge_deadline_secs,
    default_reconciliation_merge_circuit_breaker_threshold,
    default_reconciliation_merge_circuit_breaker_window,
    default_reconciliation_merge_conflict_max_retries,
    default_reconciliation_merge_incomplete_max_retries,
    default_reconciliation_merge_registry_grace_period_secs,
    default_reconciliation_merge_starvation_guard_secs, default_reconciliation_merger_timeout_secs,
    default_reconciliation_merging_max_retries, default_reconciliation_pending_merge_stale_minutes,
    default_reconciliation_validation_failure_circuit_breaker_count,
    default_reconciliation_validation_retry_min_cooldown_secs,
    default_reconciliation_validation_revert_max_count,
};
use crate::domain::entities::{
    task_metadata::RetryStrategy, AgentRunStatus, ChatContextType, InternalStatus,
    MergeFailureSource, MergeRecoveryEventKind, MergeRecoveryMetadata, MergeRecoveryReasonCode,
    PlanBranch, Task,
};
use crate::domain::state_machine::transition_handler::has_branch_missing_metadata;
use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;

use super::super::policy::{
    RecoveryActionKind, RecoveryContext, RecoveryDecision, RecoveryEvidence, ShaComparisonResult,
};
use super::super::ReconciliationRunner;

impl ReconciliationRunner {
    fn effective_failure_source_from_event(
        event: &crate::domain::entities::MergeRecoveryEvent,
    ) -> Option<MergeFailureSource> {
        if event.kind == MergeRecoveryEventKind::Deferred
            && event.reason_code == MergeRecoveryReasonCode::TargetBranchBusy
        {
            return Some(MergeFailureSource::TargetBranchBusy);
        }

        let classified_from_message = git_cmd::classify_git_failure_text(&event.message);
        if matches!(
            classified_from_message,
            MergeFailureSource::AuthFailure
                | MergeFailureSource::DiskFull
                | MergeFailureSource::DeterministicInfra
        ) {
            return Some(classified_from_message);
        }

        match event.failure_source {
            Some(MergeFailureSource::TransientGit) | Some(MergeFailureSource::Unknown) | None => {
                if classified_from_message != MergeFailureSource::Unknown {
                    Some(classified_from_message)
                } else {
                    event.failure_source.or(Some(MergeFailureSource::Unknown))
                }
            }
            Some(source) => Some(source),
        }
    }

    fn merge_failure_source_from_flat_metadata(task: &Task) -> Option<MergeFailureSource> {
        let metadata = task.metadata.as_deref()?;
        let json = serde_json::from_str::<serde_json::Value>(metadata).ok()?;
        let source = json
            .get("merge_failure_source")
            .and_then(|value| serde_json::from_value::<MergeFailureSource>(value.clone()).ok())?;
        let classified_from_error = json
            .get("error")
            .and_then(|value| value.as_str())
            .map(git_cmd::classify_git_failure_text)
            .unwrap_or(MergeFailureSource::Unknown);

        if matches!(
            classified_from_error,
            MergeFailureSource::AuthFailure
                | MergeFailureSource::DiskFull
                | MergeFailureSource::DeterministicInfra
        ) {
            return Some(classified_from_error);
        }

        Some(source)
    }

    fn latest_merge_incomplete_failure_source(task: &Task) -> MergeFailureSource {
        if let Some(source) = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
            .ok()
            .flatten()
            .and_then(|meta| {
                meta.events
                    .iter()
                    .rev()
                    .find(|event| {
                        matches!(
                            event.kind,
                            MergeRecoveryEventKind::AttemptFailed
                                | MergeRecoveryEventKind::Deferred
                                | MergeRecoveryEventKind::AutoRetryTriggered
                        )
                    })
                    .and_then(Self::effective_failure_source_from_event)
            })
        {
            return source;
        }

        if let Some(source) = Self::merge_failure_source_from_flat_metadata(task) {
            return source;
        }

        if crate::domain::state_machine::transition_handler::task_has_pr_branch_publication_failure(
            task,
        ) {
            return MergeFailureSource::TransientGit;
        }

        MergeFailureSource::Unknown
    }

    #[doc(hidden)]
    pub async fn reconcile_merging_task(
        &self,
        task: &crate::domain::entities::Task,
        status: InternalStatus,
    ) -> bool {
        if !matches!(
            status,
            InternalStatus::Merging | InternalStatus::WaitingOnPr
        ) {
            return false;
        }

        // PR-mode check: WaitingOnPr tasks have no IPR/agent because they are
        // waiting for GitHub. Legacy Merging+PR rows are treated the same during
        // migration. Never let them fall through to local merge-agent
        // timeout recovery, even if startup wiring has not attached a registry yet.
        if let Some(ref repo) = self.plan_branch_repo {
            if let Ok(Some(plan_branch)) = repo.get_by_merge_task_id(&task.id).await {
                if let (true, Some(pr_number)) = (plan_branch.pr_eligible, plan_branch.pr_number) {
                    if let Some(ref registry) = self.pr_poller_registry {
                        if registry.is_polling(&task.id) {
                            // Poller alive — check for stale heartbeat (>5min)
                            if let Some(last) = plan_branch.last_polled_at {
                                let elapsed = chrono::Utc::now() - last;
                                if elapsed > chrono::Duration::seconds(300) {
                                    warn!(
                                        task_id = task.id.as_str(),
                                        "Stale PR poller (no heartbeat >5min) — restarting"
                                    );
                                    registry.stop_polling(&task.id);
                                    self.restart_pr_merge_poller(
                                        task,
                                        &plan_branch,
                                        pr_number,
                                        "stale heartbeat",
                                    )
                                    .await;
                                }
                            }
                            return true; // Healthy poller — skip
                        }

                        // Poller not running but this already has a PR. Repair polling
                        // state and return handled so local merge timeout logic cannot fire.
                        if plan_branch.pr_polling_active {
                            warn!(
                                task_id = task.id.as_str(),
                                "Dead PR poller detected (pr_polling_active=true but not running) — restarting"
                            );
                            self.restart_pr_merge_poller(
                                task,
                                &plan_branch,
                                pr_number,
                                "dead poller",
                            )
                            .await;
                            return true;
                        }

                        warn!(
                            task_id = task.id.as_str(),
                            pr_number = pr_number,
                            "PR-backed Merging task had pr_polling_active=false — repairing polling state"
                        );
                        if let Err(e) = repo
                            .update_last_polled_at(&plan_branch.id, chrono::Utc::now())
                            .await
                        {
                            warn!(
                                task_id = task.id.as_str(),
                                error = %e,
                                "Failed to repair pr_polling_active for PR-backed Merging task"
                            );
                        }
                        self.restart_pr_merge_poller(
                            task,
                            &plan_branch,
                            pr_number,
                            "missing polling flag",
                        )
                        .await;
                        return true;
                    }

                    warn!(
                        task_id = task.id.as_str(),
                        pr_number = pr_number,
                        "PR-backed Merging task has no poller registry wired — skipping local merge reconciliation"
                    );
                    return true;
                }
            }
        }

        if status == InternalStatus::WaitingOnPr {
            warn!(
                task_id = task.id.as_str(),
                "WaitingOnPr task has no PR metadata — skipping local merge reconciliation"
            );
            return true;
        }

        // Skip if there's a live interactive process — the agent is alive between turns.
        // Cross-references IPR against registry PID to detect stale entries.
        if self
            .is_ipr_process_alive(ChatContextType::Merge, task.id.as_str())
            .await
        {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Skipping merge reconciliation: interactive process active"
            );
            return true;
        }

        // Auto-complete in-flight guard: if attempt_merge_auto_complete is already running
        // for this task (e.g. triggered by agent completion), skip this reconciliation cycle.
        // Without this, the reconciler misinterprets the dedup guard's "skip" as a failure
        // and may incorrectly escalate (increment retry count, eventually → MergeIncomplete).
        if self
            .execution_state
            .is_auto_complete_in_flight(task.id.as_str())
        {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Skipping Merging reconciliation — auto-complete already in flight"
            );
            return true;
        }

        let run = self
            .lookup_latest_run_for_task_context(task, ChatContextType::Merge)
            .await;
        let transition_age = match self.latest_status_transition_age(task, status).await {
            Some(age) => age,
            None => return false,
        };
        // Read last_active_at from registry to compute activity-based effective age.
        // Falls back to wall-clock transition age when no heartbeat exists.
        let merge_key = crate::domain::services::RunningAgentKey::new(
            ChatContextType::Merge.to_string(),
            task.id.as_str(),
        );
        let agent_info = self.running_agent_registry.get(&merge_key).await;
        let effective_age = agent_info
            .as_ref()
            .and_then(|info| info.last_active_at)
            .map(|last_active| chrono::Utc::now() - last_active)
            .unwrap_or(transition_age);
        let mut evidence = self
            .build_run_evidence(task, ChatContextType::Merge, run.as_ref())
            .await;
        evidence.is_stale = effective_age
            >= chrono::Duration::seconds(default_reconciliation_merger_timeout_secs() as i64);

        // Agent is running, registered, and not stale — let it work
        if evidence.run_status == Some(AgentRunStatus::Running)
            && evidence.registry_running
            && !evidence.is_stale
        {
            return true;
        }

        if evidence.is_stale {
            self.record_merge_timeout_event(task, effective_age).await;
        }

        // Gap 1: Check retry count — escalate to MergeConflict after max retries.
        // Re-read task to get updated metadata after record_merge_timeout_event.
        let updated_task = match self.task_repo.get_by_id(&task.id).await {
            Ok(Some(t)) => t,
            Ok(None) => return false,
            Err(_) => return false,
        };
        let retry_count = Self::merging_auto_retry_count(&updated_task);
        if retry_count >= default_reconciliation_merging_max_retries() {
            warn!(
                task_id = task.id.as_str(),
                retry_count = retry_count,
                max = default_reconciliation_merging_max_retries(),
                "Merging retry limit reached — transitioning to MergeIncomplete for user retry"
            );
            // Use MergeIncomplete (not MergeConflict) because timeout indicates a hung agent,
            // not an explicit merge conflict. MergeIncomplete surfaces the task in the
            // needs_attention panel and allows auto-retry via reconcile_merge_incomplete_task.
            return self
                .apply_recovery_decision(
                    &updated_task,
                    status,
                    RecoveryContext::Merge,
                    RecoveryDecision {
                        action: RecoveryActionKind::Transition(InternalStatus::MergeIncomplete),
                        reason: Some(format!(
                            "Merger agent timed out {} times — transitioning to MergeIncomplete for retry",
                            retry_count
                        )),
                    },
                )
                .await;
        }

        let decision = self
            .policy
            .decide_reconciliation(RecoveryContext::Merge, evidence);

        // Auto-recover merge conflicts instead of prompting the user.
        // When DB run state disagrees with registry (has_conflict), the agent likely died
        // silently. Instead of bothering the user, auto-restart within the retry budget.
        let decision = if decision.action == RecoveryActionKind::Prompt {
            // Grace period: if the agent run was created recently, the PID may not
            // have been registered yet — skip this cycle and let registration catch up.
            let grace_period_secs =
                default_reconciliation_merge_registry_grace_period_secs() as i64;
            let within_grace_period = run.as_ref().map_or(false, |r| {
                let age = chrono::Utc::now() - r.started_at;
                age < chrono::Duration::seconds(grace_period_secs)
            });

            if within_grace_period {
                tracing::debug!(
                    task_id = task.id.as_str(),
                    grace_period_secs = grace_period_secs,
                    "Merge conflict detection within grace period — skipping"
                );
                return false;
            }

            // Within retry budget: auto-restart the merger agent
            warn!(
                task_id = task.id.as_str(),
                retry_count = retry_count,
                "Auto-recovering merge conflict: restarting merger agent (run state vs registry mismatch)"
            );
            RecoveryDecision {
                action: RecoveryActionKind::ExecuteEntryActions,
                reason: Some(
                    "Auto-recovering merge run state conflict — restarting merger agent."
                        .to_string(),
                ),
            }
        } else {
            decision
        };

        // Gap 2: Don't re-spawn agent if one is still running in registry
        if decision.action == RecoveryActionKind::ExecuteEntryActions && evidence.registry_running {
            warn!(
                task_id = task.id.as_str(),
                "Skipping merger agent re-spawn — agent still running in registry"
            );
            return false;
        }

        // Gap 4: Don't retry if the merge worktree doesn't exist — retrying is futile
        // since the spawn will fail again with the same "no valid merge worktree" error.
        if decision.action == RecoveryActionKind::ExecuteEntryActions {
            if let Some(ref wt_path) = updated_task.worktree_path {
                let wt = std::path::PathBuf::from(wt_path);
                if !wt.exists() {
                    warn!(
                        task_id = task.id.as_str(),
                        worktree_path = %wt_path,
                        "Merge worktree does not exist — skipping futile retry, transitioning to MergeIncomplete"
                    );
                    return self
                        .apply_recovery_decision(
                            &updated_task,
                            status,
                            RecoveryContext::Merge,
                            RecoveryDecision {
                                action: RecoveryActionKind::Transition(
                                    InternalStatus::MergeIncomplete,
                                ),
                                reason: Some(format!(
                                    "Merge worktree {} does not exist — cannot spawn merger agent",
                                    wt_path
                                )),
                            },
                        )
                        .await;
                }
            }
        }

        // Gap 3: After auto-complete, check if task is still stuck in Merging
        if decision.action == RecoveryActionKind::AttemptMergeAutoComplete {
            self.apply_recovery_decision(&updated_task, status, RecoveryContext::Merge, decision)
                .await;
            // Re-read to see if auto-complete transitioned the task
            if let Ok(Some(post_task)) = self.task_repo.get_by_id(&task.id).await {
                if post_task.internal_status == InternalStatus::Merging {
                    warn!(
                        task_id = task.id.as_str(),
                        "Auto-complete did not transition task out of Merging — will escalate on next timeout"
                    );
                }
            }
            return true;
        }

        self.apply_recovery_decision(&updated_task, status, RecoveryContext::Merge, decision)
            .await
    }

    #[doc(hidden)]
    pub async fn reconcile_pending_merge_task(
        &self,
        task: &crate::domain::entities::Task,
        status: InternalStatus,
    ) -> bool {
        use crate::domain::state_machine::transition_handler::{
            has_main_merge_deferred_metadata, has_merge_deferred_metadata,
        };

        if status != InternalStatus::PendingMerge {
            return false;
        }

        // PR-mode skip: if PR polling is active, this task is awaiting a GitHub PR review.
        // PR reviews can take hours/days — do not mark the task as stale.
        if let Some(ref repo) = self.plan_branch_repo {
            if let Ok(Some(plan_branch)) = repo.get_by_merge_task_id(&task.id).await {
                if plan_branch.pr_polling_active {
                    tracing::debug!(
                        task_id = task.id.as_str(),
                        "Skipping PendingMerge reconciliation — PR review in progress"
                    );
                    return true;
                }
            }
        }

        // Merge-pipeline-active guard: if attempt_programmatic_merge is actively running
        // (set at start, cleared at end), skip reconciliation to prevent the reconciler
        // from killing the merge mid-pipeline (cleanup 60s + freshness 60s > stale 2min).
        let merge_pipeline_age_secs = task.merge_pipeline_active.as_deref().and_then(|ts| {
            chrono::DateTime::parse_from_rfc3339(ts)
                .ok()
                .map(|started| {
                    (chrono::Utc::now() - started.with_timezone(&chrono::Utc)).num_seconds()
                })
        });
        let merge_pipeline_ttl = default_reconciliation_attempt_merge_deadline_secs() as i64;
        if let Some(age_secs) = merge_pipeline_age_secs {
            if age_secs >= merge_pipeline_ttl {
                warn!(
                    task_id = task.id.as_str(),
                    age_secs = age_secs,
                    "merge_pipeline_active TTL expired after {}s — possible crash during prior merge attempt",
                    age_secs
                );
            }
        }
        if Self::has_merge_pipeline_active(task) {
            tracing::info!(
                task_id = task.id.as_str(),
                age_secs = merge_pipeline_age_secs.unwrap_or(0),
                ttl_secs = merge_pipeline_ttl,
                "Skipping PendingMerge reconciliation — merge_pipeline_active set {}s ago (TTL: {}s)",
                merge_pipeline_age_secs.unwrap_or(0),
                merge_pipeline_ttl
            );
            return true;
        }

        // Detect TTL expiry: merge_pipeline_active was set but has expired.
        // This means the merge pipeline started but the app crashed or was killed mid-run.
        // Write metadata so the task UI shows the failure source, then fast-track to
        // MergeIncomplete instead of waiting for the pending_merge_stale_minutes gate (2 min).
        if Self::is_merge_pipeline_active_expired(task) {
            warn!(
                task_id = task.id.as_str(),
                "merge_pipeline_active TTL expired — possible crash during prior merge attempt"
            );
            let fresh_task = if let Ok(Some(mut t)) = self.task_repo.get_by_id(&task.id).await {
                let mut meta: serde_json::Value = t
                    .metadata
                    .as_deref()
                    .and_then(|m| serde_json::from_str(m).ok())
                    .unwrap_or_else(|| serde_json::json!({}));
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert(
                        "merge_failure_source".to_string(),
                        serde_json::to_value(MergeFailureSource::PipelineActiveExpired)
                            .unwrap_or_default(),
                    );
                }
                t.metadata = Some(meta.to_string());
                t.touch();
                let _ = self.task_repo.update(&t).await;
                t
            } else {
                task.clone()
            };
            info!(
                task_id = task.id.as_str(),
                "merge_pipeline_active TTL expired — fast-tracking to MergeIncomplete"
            );
            return self
                .apply_recovery_decision(
                    &fresh_task,
                    status,
                    RecoveryContext::PendingMerge,
                    RecoveryDecision {
                        action: RecoveryActionKind::Transition(InternalStatus::MergeIncomplete),
                        reason: Some(
                            "merge_pipeline_active TTL expired — crash during prior merge attempt"
                                .to_string(),
                        ),
                    },
                )
                .await;
        }

        // Validation-in-progress guard: if validation commands are actively running
        // (set before run_validation_commands, cleared after), skip reconciliation to
        // prevent the reconciler from re-triggering merge while cargo test etc. runs.
        if Self::has_validation_in_progress(task) {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Skipping PendingMerge reconciliation — validation in progress"
            );
            return true;
        }

        // Phase 4: Handle main-merge-deferred tasks (deferred because agents were running)
        // If no agents are running now, retry the merge. If agents are still running, skip.
        if has_main_merge_deferred_metadata(task) {
            let running_count = self.execution_state.running_count();
            if running_count == 0 {
                tracing::info!(
                    task_id = task.id.as_str(),
                    "Main-merge-deferred task ready to retry: all agents completed"
                );
                return self
                    .apply_recovery_decision(
                        task,
                        status,
                        RecoveryContext::PendingMerge,
                        RecoveryDecision {
                            action: RecoveryActionKind::ExecuteEntryActions,
                            reason: Some(
                                "Main merge deferred while agents running — now retrying."
                                    .to_string(),
                            ),
                        },
                    )
                    .await;
            } else {
                tracing::debug!(
                    task_id = task.id.as_str(),
                    running_count = running_count,
                    "Main-merge-deferred task waiting for global idle: agents still running"
                );
                // Task is correctly deferred, not orphaned — skip reconciliation
                return true;
            }
        }

        let age = match self.latest_status_transition_age(task, status).await {
            Some(age) => age,
            None => return false,
        };

        let is_deferred = has_merge_deferred_metadata(task);

        // Deferred-orphan watchdog: if the recorded blocker is no longer active
        // (missing, archived, or no longer in merge workflow), immediately re-trigger
        // entry actions instead of waiting for stale timeout.
        if is_deferred {
            if let Some(blocker_id) = self.latest_deferred_blocker_id(task) {
                if !self.deferred_blocker_is_active(&blocker_id).await {
                    return self
                        .apply_recovery_decision(
                            task,
                            status,
                            RecoveryContext::PendingMerge,
                            RecoveryDecision {
                                action: RecoveryActionKind::ExecuteEntryActions,
                                reason: Some(
                                    "Deferred merge blocker is no longer active — re-triggering."
                                        .to_string(),
                                ),
                            },
                        )
                        .await;
                }
            }
        }

        let evidence = RecoveryEvidence {
            run_status: None,
            registry_running: false,
            can_start: true,
            is_stale: age
                >= chrono::Duration::minutes(
                    default_reconciliation_pending_merge_stale_minutes() as i64
                ),
            is_deferred,
        };
        let decision = self
            .policy
            .decide_reconciliation(RecoveryContext::PendingMerge, evidence);

        self.apply_recovery_decision(task, status, RecoveryContext::PendingMerge, decision)
            .await
    }

    #[doc(hidden)]
    pub async fn reconcile_merge_incomplete_task(
        &self,
        task: &crate::domain::entities::Task,
        status: InternalStatus,
    ) -> bool {
        if status != InternalStatus::MergeIncomplete {
            return false;
        }

        // mode_switch bypass: PR mode was disabled mid-plan — bypass all guards and retry PendingMerge.
        // The user explicitly requested mode switch; normal guards (circuit_breaker, rate_limit, etc.) must not block.
        let has_mode_switch = task
            .metadata
            .as_deref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
            .and_then(|v| v.get("mode_switch").and_then(|v| v.as_bool()))
            == Some(true);

        if has_mode_switch {
            warn!(
                task_id = task.id.as_str(),
                "MergeIncomplete mode_switch=true — bypassing guards, retrying PendingMerge"
            );
            return match self
                .transition_service
                .transition_task(&task.id, InternalStatus::PendingMerge)
                .await
            {
                Ok(_) => true,
                Err(e) => {
                    warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to transition MergeIncomplete -> PendingMerge during mode_switch bypass"
                    );
                    false
                }
            };
        }

        // Skip retry when branch_missing flag is set - surface to user instead
        if has_branch_missing_metadata(task) {
            return false;
        }

        // Mode-switch bypass (AD12): when PR mode is disabled mid-Merging, the task is
        // transitioned here with mode_switch=true. Bypass ALL guards — the user explicitly
        // requested a mode change, so we retry immediately via push-to-main path.
        if Self::is_mode_switch(task) {
            tracing::info!(
                task_id = task.id.as_str(),
                "Mode switch detected — bypassing all guards, retrying via PendingMerge (AD12)"
            );
            let clear_meta = MetadataUpdate::new()
                .with_null("mode_switch")
                .with_null("merge_failure_source")
                .with_null("circuit_breaker_count")
                .with_null("consecutive_validation_failures")
                .with_null("validation_revert_count");

            match self
                .transition_service
                .transition_task_with_metadata(
                    &task.id,
                    InternalStatus::PendingMerge,
                    Some(clear_meta),
                )
                .await
            {
                Ok(_) => {
                    tracing::info!(
                        task_id = task.id.as_str(),
                        "Mode switch retry: MergeIncomplete → PendingMerge (AD12)"
                    );
                    return true;
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Mode switch retry failed: could not transition to PendingMerge"
                    );
                    return false;
                }
            }
        }

        if crate::domain::state_machine::transition_handler::task_has_commit_hook_merge_failure(
            task,
        ) {
            let commit_hook_error =
                crate::domain::state_machine::transition_handler::extract_commit_hook_merge_error(
                    task,
                );
            if let Some(error) = commit_hook_error.as_deref() {
                let kind =
                    crate::domain::state_machine::transition_handler::classify_commit_hook_failure_text(
                        error,
                    );
                let fingerprint =
                    crate::domain::state_machine::transition_handler::commit_hook_failure_fingerprint(
                        error,
                    );
                let repeated =
                    crate::domain::state_machine::transition_handler::is_repeated_commit_hook_failure(
                        task,
                        &fingerprint,
                    );
                if matches!(
                    kind,
                    crate::domain::state_machine::transition_handler::CommitHookFailureKind::EnvironmentFailure
                ) || repeated
                {
                    tracing::info!(
                        task_id = task.id.as_str(),
                        kind = kind.as_str(),
                        repeated,
                        "MergeIncomplete commit-hook failure is blocked, not rerouted"
                    );
                    return match self
                        .transition_service
                        .mark_commit_hook_merge_failure_blocked(
                            &task.id,
                            Some(error.to_string()),
                            "system",
                        )
                        .await
                    {
                        Ok(_) => true,
                        Err(e) => {
                            tracing::warn!(
                                task_id = task.id.as_str(),
                                error = %e,
                                "Failed to mark commit-hook MergeIncomplete as blocked"
                            );
                            false
                        }
                    };
                }
            }

            tracing::info!(
                task_id = task.id.as_str(),
                "MergeIncomplete commit-hook failure detected — rerouting to revision flow"
            );
            return match self
                .transition_service
                .reroute_commit_hook_merge_failure(&task.id, None, true, "system")
                .await
            {
                Ok(_) => true,
                Err(e) => {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to reroute commit-hook MergeIncomplete during reconciliation"
                    );
                    false
                }
            };
        }

        // Circuit breaker active guard — fires before all other checks.
        // Prevents auto-retry when the circuit breaker has been triggered.
        // Cleared on user-initiated manual retry.
        if Self::is_circuit_breaker_active(task) {
            tracing::info!(
                task_id = task.id.as_str(),
                "Circuit breaker active — skipping auto-retry until manual retry"
            );
            return false;
        }

        // Validation-in-progress guard: same as PendingMerge guard — validation may
        // still be running when the task transitions to MergeIncomplete (e.g., revert
        // completes but subprocess lingers). Skip until validation flag expires.
        if Self::has_validation_in_progress(task) {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Skipping MergeIncomplete reconciliation — validation in progress"
            );
            return true;
        }

        // User-initiated retry guard: if retry_merge set the in-flight flag, the background
        // task will handle the transition — skip reconciliation to prevent stale-data races.
        if Self::has_merge_retry_in_progress(task) {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Skipping MergeIncomplete reconciliation — user retry in progress"
            );
            return true;
        }

        // Rate limit guard: if a provider rate limit is active, skip retry until it expires.
        // Rate-limited skips do NOT count toward max retries — the retry budget is preserved.
        //
        // When the rate limit expires, we clear it from DB via clear_rate_limit_retry_after.
        // To prevent subsequent metadata writes (record_retry_metadata, record_merge_auto_retry_event)
        // from overwriting the cleared field using the stale `task` reference, we refresh the
        // task's metadata in-memory. We preserve the original updated_at to avoid resetting
        // the age used by the retry delay check.
        let rate_limit_cleared;
        let refreshed_task;
        let task = if let Some(retry_after) = Self::get_rate_limit_retry_after(task) {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&retry_after) {
                if chrono::Utc::now() < dt {
                    tracing::debug!(
                        task_id = task.id.as_str(),
                        retry_after = %retry_after,
                        "Skipping MergeIncomplete retry — provider rate limit active"
                    );
                    return false; // Don't retry, don't count toward budget
                }
                // Rate limit expired — clear it and proceed with normal retry logic
                if let Err(e) = self.clear_rate_limit_retry_after(task).await {
                    warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to clear expired rate_limit_retry_after"
                    );
                }
                // Refresh metadata from DB so subsequent writes don't re-introduce the field,
                // but keep the original updated_at so age calculations remain stable.
                rate_limit_cleared = match self.task_repo.get_by_id(&task.id).await {
                    Ok(Some(mut refreshed)) => {
                        refreshed.updated_at = task.updated_at;
                        refreshed
                    }
                    _ => task.clone(),
                };
                &rate_limit_cleared
            } else {
                refreshed_task = task.clone();
                &refreshed_task
            }
        } else {
            task
        };

        // Smart retry guard: if the incomplete was explicitly reported by the agent,
        // it was a deliberate decision — do NOT auto-retry without human intervention.
        if Self::is_agent_reported_failure(task) {
            debug!(
                task_id = task.id.as_str(),
                "Skipping auto-retry of MergeIncomplete — agent explicitly reported this failure (AgentReported)"
            );
            return false;
        }

        // Loop-breaking guard: if validation has reverted the merge more than the configured max
        // times, stop auto-retrying and surface to user — the code changes must fix the failures.
        let revert_count = Self::validation_revert_count(task);
        if revert_count >= default_reconciliation_validation_revert_max_count() {
            debug!(
                task_id = task.id.as_str(),
                revert_count = revert_count,
                max = default_reconciliation_validation_revert_max_count(),
                "Stopping auto-retry of MergeIncomplete — validation revert loop detected (ValidationFailed)"
            );
            return false;
        }

        let is_validation = Self::is_validation_failure(task);

        // RC#4: Validation failure circuit breaker — after N consecutive validation failures,
        // stop auto-retrying entirely and leave for human intervention.
        if is_validation {
            let consecutive = Self::consecutive_validation_failures(task);
            if consecutive >= default_reconciliation_validation_failure_circuit_breaker_count() {
                debug!(
                    task_id = task.id.as_str(),
                    consecutive = consecutive,
                    max = default_reconciliation_validation_failure_circuit_breaker_count(),
                    "Circuit breaker: stopping auto-retry after consecutive validation failures"
                );
                return false;
            }
        }

        let age = match self.latest_status_transition_age(task, status).await {
            Some(age) => age,
            None => return false,
        };

        // RC#4: Validation failure cooldown — enforce minimum wait before retrying
        // after a validation failure to prevent rapid retry loops.
        if is_validation {
            let cooldown = chrono::Duration::seconds(
                default_reconciliation_validation_retry_min_cooldown_secs() as i64,
            );
            if age < cooldown {
                debug!(
                    task_id = task.id.as_str(),
                    age_secs = age.num_seconds(),
                    cooldown_secs = default_reconciliation_validation_retry_min_cooldown_secs(),
                    "Skipping MergeIncomplete retry — validation failure cooldown not elapsed"
                );
                return false;
            }
        }

        // RC#5: Starvation guard — if this task was retried recently, skip it this cycle
        // to give other MergeIncomplete tasks a turn.
        if let Some(last_retry) = Self::last_retried_at(task) {
            let since_retry = chrono::Utc::now() - last_retry;
            let guard_secs = default_reconciliation_merge_starvation_guard_secs() as i64;
            if since_retry < chrono::Duration::seconds(guard_secs) {
                debug!(
                    task_id = task.id.as_str(),
                    since_retry_secs = since_retry.num_seconds(),
                    guard_secs = guard_secs,
                    "Skipping MergeIncomplete retry — starvation guard (recently retried)"
                );
                return false;
            }
        }

        // ORDERING: must read last event AFTER rate-limit refresh and BEFORE record_retry_metadata()
        // Classify the failure source from the most recent event so auto-retries after a deferral
        // are recorded as TargetBranchBusy rather than TransientGit, preventing false circuit
        // breaker activation when tasks are repeatedly deferred for a concurrent merge.
        let failure_source = Self::latest_merge_incomplete_failure_source(task);
        if matches!(
            failure_source.retry_strategy(),
            RetryStrategy::NoAutomaticRetry
        ) {
            debug!(
                task_id = task.id.as_str(),
                failure_source = ?failure_source,
                "Skipping MergeIncomplete auto-retry — failure source is not automatically retryable"
            );
            return false;
        }

        // Circuit breaker — detect repeated identical failures before proceeding.
        // Fires when threshold+ of the last window failure events share the same source.
        let threshold = default_reconciliation_merge_circuit_breaker_threshold();
        let window = default_reconciliation_merge_circuit_breaker_window();
        if let Some(reason) = Self::should_circuit_break(task, threshold, window) {
            warn!(
                task_id = task.id.as_str(),
                reason = %reason,
                "Circuit breaker triggered — stopping auto-retry due to repeated identical failures"
            );
            if let Err(e) = self.update_circuit_breaker_metadata(task, &reason).await {
                warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "Failed to persist circuit breaker metadata"
                );
            }
            return false;
        }

        let retry_count = Self::merge_incomplete_auto_retry_count(task);
        if retry_count >= default_reconciliation_merge_incomplete_max_retries() {
            return false;
        }

        // Use total retry count (including TargetBranchBusy deferrals) for backoff so that
        // repeated deferrals still space out, even though they don't count toward the circuit breaker.
        let total_retry_count = Self::merge_incomplete_total_retry_count(task);
        let retry_delay = Self::merge_incomplete_retry_delay(total_retry_count);
        if age < retry_delay {
            return false;
        }

        let is_pr_branch_publication_failure =
            crate::domain::state_machine::transition_handler::task_has_pr_branch_publication_failure(
                task,
            );
        let reason_code = if failure_source == MergeFailureSource::TargetBranchBusy {
            Some(MergeRecoveryReasonCode::TargetBranchBusy)
        } else {
            None
        };
        let retry_reason = if failure_source == MergeFailureSource::TargetBranchBusy {
            "MergeIncomplete auto-retry — target branch busy (deferred)"
        } else if is_pr_branch_publication_failure {
            "MergeIncomplete auto-retry — PR branch publication failed"
        } else {
            "MergeIncomplete auto-retry — retryable merge failure"
        };
        let failure_source_str = serde_json::to_value(failure_source)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{failure_source:?}"));

        // Record retry metadata (last_retried_at + consecutive_validation_failures tracking)
        if let Err(e) = self.record_retry_metadata(task, is_validation).await {
            warn!(
                task_id = task.id.as_str(),
                error = %e,
                "Failed to record retry metadata (last_retried_at)"
            );
        }

        let attempt = retry_count + 1;
        if let Err(e) = self
            .record_merge_auto_retry_event(task, attempt, failure_source, retry_reason, reason_code)
            .await
        {
            warn!(
                task_id = task.id.as_str(),
                attempt = attempt,
                error = %e,
                "Failed to record merge auto-retry metadata"
            );
        }

        warn!(
            task_id = task.id.as_str(),
            attempt = attempt,
            failure_source = failure_source_str,
            "Auto-retrying MergeIncomplete — transitioning to PendingMerge"
        );

        match self
            .transition_service
            .transition_task(&task.id, InternalStatus::PendingMerge)
            .await
        {
            Ok(_) => true,
            Err(e) => {
                warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "Failed to transition MergeIncomplete -> PendingMerge during recovery"
                );
                false
            }
        }
    }

    #[doc(hidden)]
    pub async fn reconcile_merge_conflict_task(
        &self,
        task: &crate::domain::entities::Task,
        status: InternalStatus,
    ) -> bool {
        if status != InternalStatus::MergeConflict {
            return false;
        }

        // Skip retry when branch_missing flag is set - surface to user instead
        if has_branch_missing_metadata(task) {
            return false;
        }

        // User-initiated retry guard: skip reconciliation while background retry is running.
        if Self::has_merge_retry_in_progress(task) {
            tracing::debug!(
                task_id = task.id.as_str(),
                "Skipping MergeConflict reconciliation — user retry in progress"
            );
            return true;
        }

        // Smart retry guard: if the conflict was explicitly reported by the agent,
        // the agent made a deliberate decision — do NOT auto-retry without human intervention.
        if Self::is_agent_reported_failure(task) {
            warn!(
                task_id = task.id.as_str(),
                "Skipping auto-retry of MergeConflict — agent explicitly reported this conflict (AgentReported)"
            );
            return false;
        }

        let age = match self.latest_status_transition_age(task, status).await {
            Some(age) => age,
            None => return false,
        };

        let retry_count = Self::merge_conflict_auto_retry_count(task);
        if retry_count >= default_reconciliation_merge_conflict_max_retries() {
            return false;
        }

        let retry_delay = Self::merge_conflict_retry_delay(retry_count);
        if age < retry_delay {
            return false;
        }

        // SHA comparison guard: if source branch hasn't changed since last failure,
        // retrying the same commit will produce the same conflict — skip.
        let sha_changed = self.check_source_sha_changed(task).await;
        match sha_changed {
            ShaComparisonResult::Unchanged(sha) => {
                warn!(
                    task_id = task.id.as_str(),
                    source_sha = %sha,
                    "Skipping MergeConflict auto-retry — source branch SHA unchanged since last failure"
                );
                return false;
            }
            ShaComparisonResult::Changed { old_sha, new_sha } => {
                warn!(
                    task_id = task.id.as_str(),
                    old_sha = %old_sha,
                    new_sha = %new_sha,
                    "Source branch SHA changed since last failure — proceeding with retry"
                );
            }
            ShaComparisonResult::NoStoredSha | ShaComparisonResult::GitError => {
                // No SHA stored (first attempt) or git error — allow retry
            }
        }

        // Get current source SHA to record with the retry event
        let current_sha = self.get_current_source_sha(task).await;

        let attempt = retry_count + 1;
        let failure_source = MergeFailureSource::SystemDetected;
        if let Err(e) = self
            .record_merge_auto_retry_event_with_sha(
                task,
                attempt,
                failure_source,
                "MergeConflict auto-retry — system-detected conflict",
                current_sha.as_deref(),
                None,
            )
            .await
        {
            warn!(
                task_id = task.id.as_str(),
                attempt = attempt,
                error = %e,
                "Failed to record merge auto-retry metadata"
            );
        }

        warn!(
            task_id = task.id.as_str(),
            attempt = attempt,
            failure_source = "system_detected",
            source_sha = current_sha.as_deref().unwrap_or("unknown"),
            "Auto-retrying MergeConflict — system-detected conflict, transitioning to PendingMerge"
        );

        match self
            .transition_service
            .transition_task(&task.id, InternalStatus::PendingMerge)
            .await
        {
            Ok(_) => true,
            Err(e) => {
                warn!(
                    task_id = task.id.as_str(),
                    error = %e,
                    "Failed to transition MergeConflict -> PendingMerge during recovery"
                );
                false
            }
        }
    }

    async fn restart_pr_merge_poller(
        &self,
        task: &crate::domain::entities::Task,
        plan_branch: &PlanBranch,
        pr_number: i64,
        reason: &'static str,
    ) -> bool {
        let Some(ref registry) = self.pr_poller_registry else {
            warn!(
                task_id = task.id.as_str(),
                pr_number = pr_number,
                reason = reason,
                "Cannot restart PR poller because registry is unavailable"
            );
            return false;
        };
        let project = match self.project_repo.get_by_id(&plan_branch.project_id).await {
            Ok(Some(project)) => project,
            Ok(None) => {
                warn!(
                    task_id = task.id.as_str(),
                    project_id = plan_branch.project_id.as_str(),
                    reason = reason,
                    "Cannot restart PR poller because project was not found"
                );
                return false;
            }
            Err(e) => {
                warn!(
                    task_id = task.id.as_str(),
                    project_id = plan_branch.project_id.as_str(),
                    reason = reason,
                    error = %e,
                    "Cannot restart PR poller because project lookup failed"
                );
                return false;
            }
        };
        registry.start_polling(
            task.id.clone(),
            plan_branch.id.clone(),
            pr_number,
            std::path::PathBuf::from(&project.working_directory),
            plan_branch.source_branch.clone(),
            Arc::clone(&self.transition_service),
        );
        true
    }

    /// Returns true if `merge_pipeline_active` is set but has EXPIRED (age >= deadline).
    /// Distinguishes the "expired crash" case from "never set" (both return false from has_merge_pipeline_active).
    #[doc(hidden)]
    pub fn is_merge_pipeline_active_expired(task: &crate::domain::entities::Task) -> bool {
        task.merge_pipeline_active
            .as_deref()
            .and_then(|ts| {
                let started = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
                let age = chrono::Utc::now() - started.with_timezone(&chrono::Utc);
                let deadline_secs = default_reconciliation_attempt_merge_deadline_secs();
                Some(age >= chrono::Duration::seconds(deadline_secs as i64))
            })
            .unwrap_or(false)
    }
}
