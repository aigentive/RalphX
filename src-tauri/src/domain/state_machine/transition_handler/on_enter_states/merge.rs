use super::*;
use crate::domain::state_machine::transition_handler::merge_helpers;
use crate::domain::state_machine::transition_handler::set_trigger_origin;
use crate::domain::state_machine::TransitionHandler;

async fn pr_branch_update_conflict_active(
    services: &crate::domain::state_machine::context::TaskServices,
    task_id: &TaskId,
) -> bool {
    let Some(task_repo) = services.task_repo.as_ref() else {
        return false;
    };
    task_repo
        .get_by_id(task_id)
        .await
        .ok()
        .flatten()
        .and_then(|task| task.metadata)
        .and_then(|metadata| serde_json::from_str::<serde_json::Value>(&metadata).ok())
        .and_then(|metadata| metadata.get("pr_branch_update_conflict")?.as_bool())
        .unwrap_or(false)
}

impl<'a> TransitionHandler<'a> {
    async fn load_merge_prompt_context(&self, task_id: &str) -> MergePromptContext {
        let Some(task_repo) = &self.machine.context.services.task_repo else {
            return MergePromptContext::default();
        };

        let tid = TaskId::from_string(task_id.to_string());
        let Ok(Some(task)) = task_repo.get_by_id(&tid).await else {
            return MergePromptContext::default();
        };

        let meta = task
            .metadata
            .as_ref()
            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok());

        MergePromptContext {
            is_validation_recovery: meta
                .as_ref()
                .and_then(|v| v.get("validation_recovery")?.as_bool())
                .unwrap_or(false),
            is_plan_update_conflict: meta
                .as_ref()
                .and_then(|v| v.get("plan_update_conflict")?.as_bool())
                .unwrap_or(false),
            is_source_update_conflict: meta
                .as_ref()
                .and_then(|v| v.get("source_update_conflict")?.as_bool())
                .unwrap_or(false),
            is_pr_branch_update_conflict: meta
                .as_ref()
                .and_then(|v| v.get("pr_branch_update_conflict")?.as_bool())
                .unwrap_or(false),
            is_pr_branch_publication_conflict: meta
                .as_ref()
                .and_then(|v| v.get("pr_branch_publication_conflict")?.as_bool())
                .unwrap_or(false),
            freshness_conflict_count: meta
                .as_ref()
                .and_then(|v| v.get("freshness_conflict_count")?.as_u64())
                .unwrap_or(0) as u32,
            base_branch: meta
                .as_ref()
                .and_then(|v| v.get("base_branch")?.as_str().map(String::from)),
            source_branch: meta
                .as_ref()
                .and_then(|v| v.get("source_branch")?.as_str().map(String::from)),
            target_branch: meta
                .as_ref()
                .and_then(|v| v.get("target_branch")?.as_str().map(String::from)),
        }
    }

    fn build_merge_prompt(&self, task_id: &str, context: &MergePromptContext) -> String {
        let prompt = if context.is_validation_recovery {
            format!(
                "Fix validation failures for task: {}. The merge succeeded but post-merge \
                 validation commands failed. The failing code is on the target branch. \
                 Read the validation failures from task context, fix the code, run validation \
                 to confirm, then commit your fixes.",
                task_id
            )
        } else if context.is_pr_branch_update_conflict {
            let base_branch = context
                .base_branch
                .clone()
                .unwrap_or_else(|| "origin/main".to_string());
            let plan_branch = context.target_branch.clone().unwrap_or_default();
            if context.is_pr_branch_publication_conflict {
                format!(
                    "Resolve the PR branch publication conflict for task {task_id}.\n\n\
                     The regular task merge is already on the local plan branch ({plan_branch}), \
                     but the remote PR branch ({base_branch}) has commits that must be \
                     incorporated before the branch can be pushed.\n\n\
                     Your working directory is the merge worktree where the plan branch is \
                     already checked out. DO NOT merge this PR into the base branch — GitHub \
                     remains the final merge authority.\n\n\
                     Steps:\n\
                     1. Run `git status` to confirm you are on the plan branch ({plan_branch})\n\
                     2. Run `git merge {base_branch}` to trigger the merge and expose conflicts\n\
                     3. Resolve all conflict markers in the conflicted files\n\
                     4. Stage resolved files: `git add <files>`\n\
                     5. Commit: `git commit --no-edit`\n\
                     6. Run `git rev-parse HEAD` and call `complete_merge` with that full commit SHA\n\
                     7. Do not exit silently — `complete_merge` publishes the PR branch and finalizes the task\n\n\
                     If the conflict is too complex, call report_incomplete with a description.",
                    task_id = task_id,
                    base_branch = base_branch,
                    plan_branch = plan_branch,
                )
            } else {
                format!(
                    "Resolve the GitHub PR branch update conflict for task {task_id}.\n\n\
                     The plan branch ({plan_branch}) needs to incorporate {base_branch} \
                     before PR review can continue, but there are merge conflicts.\n\n\
                     Your working directory is the merge worktree where the plan branch is \
                     already checked out. DO NOT merge this PR into the base branch — GitHub \
                     remains the final merge authority.\n\n\
                     Steps:\n\
                     1. Run `git status` to confirm you are on the plan branch ({plan_branch})\n\
                     2. Run `git merge {base_branch}` to trigger the merge and expose conflicts\n\
                     3. Resolve all conflict markers in the conflicted files\n\
                     4. Stage resolved files: `git add <files>`\n\
                     5. Commit: `git commit --no-edit`\n\
                     6. Run `git rev-parse HEAD` and call `complete_merge` with that full commit SHA\n\
                     7. Do not exit silently — `complete_merge` returns the task to PR waiting\n\n\
                     If the conflict is too complex, call report_incomplete with a description.",
                    task_id = task_id,
                    base_branch = base_branch,
                    plan_branch = plan_branch,
                )
            }
        } else if context.is_plan_update_conflict {
            let base_branch = context
                .base_branch
                .clone()
                .unwrap_or_else(|| "main".to_string());
            let plan_branch = context.target_branch.clone().unwrap_or_default();
            format!(
                "Resolve the plan branch update conflict for task {task_id}.\n\n\
                 The plan branch ({plan_branch}) needs to be updated from {base_branch} \
                 before the task can merge, but there are merge conflicts.\n\n\
                 Your working directory is the merge worktree where the plan branch is \
                 already checked out. DO NOT merge the task branch — the system handles \
                 that automatically after you finish.\n\n\
                 Steps:\n\
                 1. Run `git status` to confirm you are on the plan branch ({plan_branch})\n\
                 2. Run `git merge {base_branch}` to trigger the merge and expose conflicts\n\
                 3. Resolve all conflict markers in the conflicted files\n\
                 4. Stage resolved files: `git add <files>`\n\
                 5. Commit: `git commit --no-edit`\n\
                 6. Run `git rev-parse HEAD` and call `complete_merge` with that full commit SHA\n\
                 7. Do not exit silently — `complete_merge` tells the system how to continue\n\n\
                 If the conflict is too complex, call report_incomplete with a description.",
                task_id = task_id,
                base_branch = base_branch,
                plan_branch = plan_branch,
            )
        } else if context.is_source_update_conflict {
            let source_branch = context.source_branch.clone().unwrap_or_default();
            let target_branch = context.target_branch.clone().unwrap_or_default();
            format!(
                "Resolve the source branch update conflict for task {task_id}.\n\n\
                 The task branch ({source_branch}) needs to incorporate changes from \
                 {target_branch} before it can be merged, but there are conflicts.\n\n\
                 Your working directory is the merge worktree with the task branch checked out.\n\n\
                 Steps:\n\
                 1. Run `git status` to confirm you are on the task branch ({source_branch})\n\
                 2. Run `git merge {target_branch}` to trigger the merge and expose conflicts\n\
                 3. Resolve all conflict markers in the conflicted files\n\
                 4. Stage resolved files: `git add <files>`\n\
                 5. Commit: `git commit --no-edit`\n\
                 6. Run `git rev-parse HEAD` and call `complete_merge` with that full commit SHA\n\
                 7. Do not exit silently — `complete_merge` tells the system how to continue\n\n\
                 If the conflict is too complex, call report_incomplete with a description.",
                task_id = task_id,
                source_branch = source_branch,
                target_branch = target_branch,
            )
        } else {
            format!("Resolve merge conflicts for task: {}", task_id)
        };

        if context.freshness_conflict_count > 1
            && (context.is_plan_update_conflict || context.is_source_update_conflict)
        {
            let config = reconciliation_config();
            format!(
                "{}\n\nIMPORTANT: This is retry {} of {}. Previous resolution \
                 attempts did not fully resolve the staleness. Take extra care to \
                 resolve ALL conflicts completely. If you cannot resolve cleanly, \
                 call report_incomplete rather than committing a partial resolution.",
                prompt, context.freshness_conflict_count, config.freshness_max_conflict_retries
            )
        } else {
            prompt
        }
    }

    pub(super) async fn maybe_start_pr_mode_merge_poller(&self, task_id: &str) -> bool {
        if let (Some(ref plan_branch_repo), Some(ref project_repo)) = (
            &self.machine.context.services.plan_branch_repo,
            &self.machine.context.services.project_repo,
        ) {
            let tid = TaskId::from_string(task_id.to_string());
            let project_id = ProjectId::from_string(self.machine.context.project_id.clone());
            if let (Ok(Some(plan_branch)), Ok(Some(_project))) = (
                plan_branch_repo.get_by_merge_task_id(&tid).await,
                project_repo.get_by_id(&project_id).await,
            ) {
                if pr_branch_update_conflict_active(&self.machine.context.services, &tid).await {
                    tracing::info!(
                        task_id = task_id,
                        "PR mode: bypassing PR poller because merger agent must resolve PR branch update conflict"
                    );
                    return false;
                }
                if let (true, Some(pr_number)) = (plan_branch.pr_eligible, plan_branch.pr_number) {
                    tracing::info!(
                        task_id = task_id,
                        pr_number = pr_number,
                        "PR mode: starting PR merge poller"
                    );

                    let already_polling = self
                        .machine
                        .context
                        .services
                        .pr_poller_registry
                        .as_ref()
                        .map(|r| r.is_polling(&tid))
                        .unwrap_or(false);

                    if already_polling {
                        tracing::debug!(
                            task_id = task_id,
                            pr_number = pr_number,
                            "PR mode: poller already running"
                        );
                        return true;
                    }

                    if let Some(ref registry) = self.machine.context.services.pr_poller_registry {
                        if let Ok(Some(project_for_poller)) =
                            project_repo.get_by_id(&project_id).await
                        {
                            let working_dir =
                                std::path::PathBuf::from(&project_for_poller.working_directory);
                            let base_branch = plan_branch.source_branch.clone();
                            if let Some(ref ts) = self.machine.context.services.transition_service {
                                registry.start_polling(
                                    tid.clone(),
                                    plan_branch.id.clone(),
                                    pr_number,
                                    working_dir,
                                    base_branch,
                                    Arc::clone(ts),
                                );
                                tracing::info!(
                                    task_id = task_id,
                                    pr_number = pr_number,
                                    "PR mode: started PR merge poller"
                                );
                            } else {
                                tracing::warn!(
                                    task_id = task_id,
                                    pr_number = pr_number,
                                    "PR mode: transition_service not wired — poller not started"
                                );
                            }
                        }
                    }

                    return true;
                }
            }
        }

        false
    }

    async fn prepare_merge_worktree_for_entry(&self, task_id: &str) {
        if let (Some(ref task_repo), Some(ref project_repo)) = (
            &self.machine.context.services.task_repo,
            &self.machine.context.services.project_repo,
        ) {
            let project_id = ProjectId::from_string(self.machine.context.project_id.clone());
            if let Ok(Some(project)) = project_repo.get_by_id(&project_id).await {
                let wt_path =
                    std::path::PathBuf::from(compute_merge_worktree_path(&project, task_id));
                let repo_path = std::path::Path::new(&project.working_directory);

                let wt_exists =
                    crate::utils::path_safety::checked_exists(&wt_path, "merge worktree")
                        .unwrap_or(false);
                let repo_exists =
                    crate::utils::path_safety::checked_exists(repo_path, "project repository")
                        .unwrap_or(false);

                if !wt_exists && repo_exists {
                    let tid = TaskId::from_string(task_id.to_string());
                    if let Ok(Some(task)) = task_repo.get_by_id(&tid).await {
                        let meta = task
                            .metadata
                            .as_ref()
                            .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok());
                        let is_plan_conflict = meta
                            .as_ref()
                            .and_then(|v| v.get("plan_update_conflict")?.as_bool())
                            .unwrap_or(false);
                        let is_source_conflict = meta
                            .as_ref()
                            .and_then(|v| v.get("source_update_conflict")?.as_bool())
                            .unwrap_or(false);
                        let meta_source_branch = meta
                            .as_ref()
                            .and_then(|v| v.get("source_branch")?.as_str().map(String::from));
                        let meta_target_branch = meta
                            .as_ref()
                            .and_then(|v| v.get("target_branch")?.as_str().map(String::from));

                        let checkout_branch: Option<String> = if is_plan_conflict {
                            meta_target_branch.or_else(|| project.base_branch.clone())
                        } else if is_source_conflict {
                            meta_source_branch.or_else(|| task.task_branch.clone())
                        } else {
                            task.task_branch.clone()
                        };

                        if let Some(ref branch) = checkout_branch {
                            let task_wt_str =
                                merge_helpers::compute_task_worktree_path(&project, task_id);
                            let task_wt_path = std::path::PathBuf::from(&task_wt_str);
                            merge_helpers::pre_delete_worktree(repo_path, &task_wt_path, task_id)
                                .await;

                            let plan_update_wt_str =
                                merge_helpers::compute_plan_update_worktree_path(&project, task_id);
                            let plan_update_wt_path = std::path::PathBuf::from(&plan_update_wt_str);
                            merge_helpers::pre_delete_worktree(
                                repo_path,
                                &plan_update_wt_path,
                                task_id,
                            )
                            .await;

                            match GitService::checkout_existing_branch_worktree(
                                repo_path, &wt_path, branch,
                            )
                            .await
                            {
                                Ok(_) => {
                                    tracing::info!(
                                        task_id = task_id,
                                        branch = %branch,
                                        worktree = %wt_path.display(),
                                        is_plan_conflict = is_plan_conflict,
                                        is_source_conflict = is_source_conflict,
                                        "on_enter(Merging): Created merge worktree for freshness-conflict path"
                                    );
                                    if let Ok(Some(mut fresh_task)) =
                                        task_repo.get_by_id(&tid).await
                                    {
                                        fresh_task.worktree_path =
                                            Some(wt_path.to_string_lossy().to_string());
                                        fresh_task.touch();
                                        if let Err(e) = task_repo.update(&fresh_task).await {
                                            tracing::warn!(
                                                task_id = task_id,
                                                error = %e,
                                                "on_enter(Merging): Failed to persist worktree_path — cleaning up orphan"
                                            );
                                            let _ =
                                                GitService::delete_worktree(repo_path, &wt_path)
                                                    .await;
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        task_id = task_id,
                                        branch = %branch,
                                        error = %e,
                                        "on_enter(Merging): Failed to create merge worktree on freshness path"
                                    );
                                }
                            }
                        } else {
                            tracing::warn!(
                                task_id = task_id,
                                "on_enter(Merging): No merge worktree and no branch to checkout from metadata"
                            );
                        }
                    }
                }

                if crate::utils::path_safety::checked_exists(&wt_path, "merge worktree")
                    .unwrap_or(false)
                {
                    merge_helpers::clean_stale_git_state(&wt_path, task_id).await;

                    for rel in &[
                        "node_modules",
                        "src-tauri/target",
                        "plugins/app/ralphx-mcp-server/node_modules",
                    ] {
                        let sym = wt_path.join(rel);
                        if crate::utils::path_safety::checked_is_symlink(
                            &sym,
                            "worktree symlink cleanup",
                        )
                        .unwrap_or(false)
                        {
                            tracing::info!(
                                task_id = task_id,
                                path = %sym.display(),
                                "on_enter(Merging): Removing worktree symlink"
                            );
                            if let Err(e) = crate::utils::path_safety::checked_remove_file(
                                &sym,
                                "worktree symlink cleanup",
                            ) {
                                tracing::warn!(
                                    task_id = task_id,
                                    path = %sym.display(),
                                    error = %e,
                                    "Failed to remove worktree symlink"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Execute on-enter dispatch for all state arms.
    ///
    /// Called by `on_enter` in side_effects.rs.
    pub(super) async fn enter_merging_state(&self) -> AppResult<()> {
        let task_id = &self.machine.context.task_id;

        let prompt_context = self.load_merge_prompt_context(task_id).await;
        if !prompt_context.is_pr_branch_update_conflict
            && self.maybe_start_pr_mode_merge_poller(task_id).await
        {
            return Ok(());
        }
        self.prepare_merge_worktree_for_entry(task_id).await;

        let prompt = self.build_merge_prompt(task_id, &prompt_context);

        tracing::info!(
            task_id = task_id,
            is_validation_recovery = prompt_context.is_validation_recovery,
            is_plan_update_conflict = prompt_context.is_plan_update_conflict,
            is_source_update_conflict = prompt_context.is_source_update_conflict,
            freshness_conflict_count = prompt_context.freshness_conflict_count,
            "on_enter(Merging): Spawning merger agent via ChatService"
        );

        let result = self
            .machine
            .context
            .services
            .chat_service
            .send_message(
                crate::domain::entities::ChatContextType::Merge,
                task_id,
                &prompt,
                Default::default(),
            )
            .await;

        match result {
            Ok(result) if result.was_queued => {
                tracing::info!(
                    task_id = task_id,
                    "Agent already running for this task — treating on_enter(Merging) as no-op"
                );
                Ok(())
            }
            Ok(_) => {
                tracing::info!(task_id = task_id, "Merger agent spawned successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(task_id = task_id, error = %e, "Failed to spawn merger agent");
                record_merger_spawn_failure(
                    &self.machine.context.services.task_repo,
                    task_id,
                    &e.to_string(),
                )
                .await;
                Ok(())
            }
        }
    }
}

/// Record a merger agent spawn failure as an `AttemptFailed` event in task metadata.
///
/// Each spawn failure consumes one slot of the reconciler's retry budget
/// (`merging_max_retries`). Once the budget is exhausted the reconciler
/// transitions the task to `MergeIncomplete` on the next cycle (≤30 s).
async fn record_merger_spawn_failure(
    task_repo: &Option<std::sync::Arc<dyn crate::domain::repositories::TaskRepository>>,
    task_id: &str,
    error: &str,
) {
    let Some(repo) = task_repo else { return };
    let tid = TaskId::from_string(task_id.to_string());
    let Ok(Some(mut task)) = repo.get_by_id(&tid).await else {
        return;
    };

    let mut recovery = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .unwrap_or(None)
        .unwrap_or_default();

    let spawn_failure_count = recovery
        .events
        .iter()
        .filter(|ev| {
            ev.kind == MergeRecoveryEventKind::AttemptFailed
                && ev.message.contains("failed to spawn")
        })
        .count() as u32
        + 1;

    let error_lower = error.to_lowercase();
    let spawn_failure_source =
        if error_lower.contains(ENOENT_MARKER) || error_lower.contains("no such file") {
            MergeFailureSource::SpawnFailure
        } else {
            MergeFailureSource::TransientGit
        };
    let event = MergeRecoveryEvent::new(
        MergeRecoveryEventKind::AttemptFailed,
        MergeRecoverySource::System,
        MergeRecoveryReasonCode::GitError,
        format!("Merger agent failed to spawn: {}", error),
    )
    .with_failure_source(spawn_failure_source);

    recovery.append_event_with_state(event, MergeRecoveryState::Failed);

    let max_retries = reconciliation_config().merging_max_retries as u32;
    if let Ok(updated_meta) = recovery.update_task_metadata(task.metadata.as_deref()) {
        task.metadata = Some(updated_meta);
        set_trigger_origin(&mut task, "recovery");
        task.touch();
        if let Err(e) = repo.update(&task).await {
            tracing::warn!(
                task_id = task_id,
                error = %e,
                "Failed to persist merger spawn failure metadata"
            );
        } else {
            tracing::warn!(
                task_id = task_id,
                spawn_failure_count = spawn_failure_count,
                max_retries = max_retries,
                "Recorded merger spawn failure ({}/{}); reconciler will transition to \
                 MergeIncomplete when retry budget is exhausted",
                spawn_failure_count,
                max_retries,
            );
        }
    }
}
