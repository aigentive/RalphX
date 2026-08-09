use super::*;
use crate::application::task_diff_base::task_allows_empty_captured_diff;

pub async fn ensure_task_still_reviewing_before_transition(
    state: &HttpServerState,
    task_id: &TaskId,
    decision: &str,
) -> Result<(), (StatusCode, String)> {
    let current_task = state
        .app_state
        .task_repo
        .get_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    if current_task.internal_status != InternalStatus::Reviewing {
        tracing::warn!(
            task_id = %task_id.as_str(),
            decision = %decision,
            rejection_reason = %current_task.internal_status.as_str(),
            "complete_review rejected: task no longer in reviewing state before transition"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Task not in reviewing state. Current state: {}",
                current_task.internal_status.as_str()
            ),
        ));
    }

    Ok(())
}

pub async fn complete_review(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<CompleteReviewRequest>,
) -> Result<Json<CompleteReviewResponse>, (StatusCode, String)> {
    let task_id = TaskId::from_string(req.task_id);

    // 1. Get task and validate state is Reviewing
    let mut task = state
        .app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Task not found".to_string()))?;

    // Enforce project scope (no-op for internal requests without the header)
    task.assert_project_scope(&scope)
        .map_err(|e| (e.status, e.message.unwrap_or_default()))?;

    if task.internal_status != InternalStatus::Reviewing {
        tracing::warn!(
            task_id = %task_id.as_str(),
            decision = %req.decision,
            rejection_reason = %task.internal_status.as_str(),
            "complete_review rejected: task not in reviewing state"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Task not in reviewing state. Current state: {}",
                task.internal_status.as_str()
            ),
        ));
    }

    let task_context = get_task_context_impl(&state.app_state, &task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let scope_drift_classification = req
        .scope_drift_classification
        .as_deref()
        .map(str::parse::<ScopeDriftClassification>)
        .transpose()
        .map_err(|e| {
            tracing::warn!(
                task_id = %task_id.as_str(),
                decision = %req.decision,
                rejection_reason = %e,
                "complete_review rejected: invalid scope_drift_classification"
            );
            (StatusCode::BAD_REQUEST, e.to_string())
        })?;
    let prior_review_notes = state
        .app_state
        .review_repo
        .get_notes_by_task_id(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let revision_count = count_revision_cycles(&prior_review_notes);
    let review_settings = state
        .app_state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 2. Parse and validate decision policy
    let outcome = parse_review_decision(&req.decision).map_err(|e| {
        tracing::warn!(
            task_id = %task_id.as_str(),
            decision = %req.decision,
            rejection_reason = %e,
            "complete_review rejected: invalid decision value"
        );
        (StatusCode::BAD_REQUEST, e.to_string())
    })?;
    validate_complete_review_policy(
        task_context.scope_drift_status.clone(),
        &task_context.out_of_scope_files,
        scope_drift_classification,
        outcome,
        revision_count,
        &review_settings,
        req.issues.as_ref().map_or(0, Vec::len),
    )
    .map_err(|e| {
        tracing::warn!(
            task_id = %task_id.as_str(),
            decision = %req.decision,
            rejection_reason = %e,
            "complete_review rejected: policy validation failed"
        );
        (StatusCode::BAD_REQUEST, e.to_string())
    })?;

    if matches!(outcome, ReviewToolOutcome::Approved) {
        let project = state
            .app_state
            .project_repo
            .get_by_id(&task.project_id)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "Project not found".to_string()))?;

        ensure_task_has_non_empty_captured_diff(&task, &project, "complete_review_approved")
            .await
            .map_err(|error| {
                tracing::warn!(
                    task_id = %task_id.as_str(),
                    decision = %req.decision,
                    error = %error,
                    "complete_review rejected: approved code-change task has no captured-base diff"
                );
                (StatusCode::BAD_REQUEST, error.to_string())
            })?;
    }

    // 3. Get feedback - stored separately from issues now
    let feedback = req.feedback.clone();

    // 4. Get or create Review record for this task
    let reviews = state
        .app_state
        .review_repo
        .get_by_task_id(&task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let (is_new_review, mut review) =
        pending_review_or_new(reviews, task.project_id.clone(), task_id.clone());

    // 5. Process the review result based on outcome
    let review_outcome = apply_review_outcome(&mut review, outcome, feedback.clone());

    // Save review
    if is_new_review {
        // New review, create it
        state
            .app_state
            .review_repo
            .create(&review)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        // Existing review, update it
        state
            .app_state
            .review_repo
            .update(&review)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let parsed_issues = req
        .issues
        .as_ref()
        .map(|issues| {
            parse_review_issues(
                &issues
                    .iter()
                    .map(|issue| RawReviewIssueInput {
                        severity: issue.severity.clone(),
                        title: issue.title.clone(),
                        step_id: issue.step_id.clone(),
                        no_step_reason: issue.no_step_reason.clone(),
                        description: issue.description.clone(),
                        category: issue.category.clone(),
                        file_path: issue.file_path.clone(),
                        line_number: issue.line_number,
                        code_snippet: issue.code_snippet.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .transpose()
        .map_err(|msg: String| {
            tracing::warn!(
                task_id = %task_id.as_str(),
                decision = %req.decision,
                rejection_reason = %msg,
                "complete_review rejected: invalid review issues"
            );
            (StatusCode::BAD_REQUEST, msg)
        })?;

    let domain_issues = parsed_issues
        .as_ref()
        .map(|issues| build_review_note_issues(issues));

    // For now, we don't create fix tasks automatically - that can be added later
    let fix_task_id: Option<TaskId> = None;
    let followup_conversation_id = maybe_register_unrelated_drift_issue(
        &state,
        &task,
        &review,
        &task_context,
        outcome,
        scope_drift_classification,
        revision_count,
        &review_settings,
        req.summary.as_deref(),
        req.feedback.as_deref(),
        req.escalation_reason.as_deref(),
    )
    .await;
    let followup_session_id: Option<String> = None;

    // Create review note for history.
    // For escalations, prefer escalation_reason over generic feedback so the
    // frontend EscalatedTaskDetail can display a precise reason.
    let note_content = review_note_content(
        outcome,
        req.feedback.as_deref(),
        req.escalation_reason.as_deref(),
    );
    // Legitimate AI decision via MCP tool — agent deliberately called complete_review. Do NOT change to System.
    let review_note = build_ai_review_note(
        task_id.clone(),
        review_outcome,
        req.summary.clone(),
        note_content,
        domain_issues,
        followup_session_id.clone(),
    );
    state
        .app_state
        .review_repo
        .add_note(&review_note)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let review_note_id = review_note.id.clone();

    persist_review_scope_snapshot(
        &state,
        &mut task,
        &task_context,
        scope_drift_classification,
        req.scope_drift_notes.clone(),
    )
    .await?;

    if matches!(outcome, ReviewToolOutcome::NeedsChanges) {
        if let Some(issues) = parsed_issues {
            if !issues.is_empty() {
                state
                    .app_state
                    .review_issue_repo
                    .bulk_create(build_review_issue_entities(
                        issues,
                        review_note.id.clone(),
                        task_id.clone(),
                    ))
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            }
        }
    }

    // 6. Trigger state transition via TaskTransitionService
    // Create scheduler for auto-scheduling next Ready task when this one exits Reviewing
    let scheduler_concrete = Arc::new(
        state
            .app_state
            .build_task_scheduler_for_runtime(Arc::clone(&state.execution_state), None),
    );
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

    let mut transition_service_builder = state
        .app_state
        .build_transition_service_for_runtime(Arc::clone(&state.execution_state), None)
        .with_task_scheduler(task_scheduler);

    if let Some(ref pub_) = state.app_state.webhook_publisher {
        transition_service_builder =
            transition_service_builder.with_webhook_publisher_for_emitter(Arc::clone(pub_));
    }

    let transition_service = transition_service_builder
        .with_external_events_repo(Arc::clone(&state.app_state.external_events_repo));

    // Early unregister: remove the review agent from running_agent_registry BEFORE triggering
    // the state transition. This prevents pre_merge_cleanup from seeing the review agent as
    // "still running" and stopping it — which would kill this very HTTP connection and cancel
    // the entire inline merge pipeline chain. The registry's unregister is idempotent:
    // process_stream_background's own unregister later becomes a no-op.
    {
        let review_key = RunningAgentKey::new("review", task_id.as_str());
        if let Some(agent_info) = state
            .app_state
            .running_agent_registry
            .get(&review_key)
            .await
        {
            let _ = state
                .app_state
                .running_agent_registry
                .unregister(&review_key, &agent_info.agent_run_id)
                .await;
            tracing::info!(
                task_id = task_id.as_str(),
                agent_run_id = %agent_info.agent_run_id,
                "Early-unregistered review agent before state transition to prevent merge self-sabotage"
            );
        }
    }

    let new_status = match outcome {
        ReviewToolOutcome::Approved => {
            ensure_task_still_reviewing_before_transition(&state, &task_id, &req.decision).await?;

            // Check if human review is required
            let require_human = state
                .app_state
                .review_settings_repo
                .get_settings()
                .await
                .map(|s| s.require_human_review)
                .unwrap_or(false);

            transition_ai_review_approval(&state, &transition_service, &task_id, require_human)
                .await?
        }
        ReviewToolOutcome::NeedsChanges => {
            ensure_task_still_reviewing_before_transition(&state, &task_id, &req.decision).await?;

            // Needs changes: transition to RevisionNeeded (auto re-execute)
            transition_service
                .transition_task(&task_id, InternalStatus::RevisionNeeded)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            InternalStatus::RevisionNeeded
        }
        ReviewToolOutcome::Escalate => {
            ensure_task_still_reviewing_before_transition(&state, &task_id, &req.decision).await?;

            // Escalate: transition to Escalated (requires human decision)
            transition_service
                .transition_task(&task_id, InternalStatus::Escalated)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            InternalStatus::Escalated
        }
        ReviewToolOutcome::ApprovedNoChanges => {
            ensure_task_still_reviewing_before_transition(&state, &task_id, &req.decision).await?;

            // Extract fields BEFORE transition (transition may clear these from task)
            let task_branch = task.task_branch.clone();
            let worktree_path = task.worktree_path.clone();

            let require_human = state
                .app_state
                .review_settings_repo
                .get_settings()
                .await
                .map(|s| s.require_human_review)
                .unwrap_or(false);

            // Fetch project for repo_path and working_directory (needed for git diff + cleanup)
            let project = state
                .app_state
                .project_repo
                .get_by_id(&task.project_id)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .ok_or_else(|| (StatusCode::NOT_FOUND, "Project not found".to_string()))?;

            // Git diff validation safety gate (BEFORE metadata persistence).
            // If the branch has code changes, fall back to standard Approved flow.
            let has_code_changes = match read_captured_task_diff_stats(
                &task,
                &project,
                "complete_review_approved_no_changes",
            )
            .await
            {
                Ok(Some(stats)) => {
                    let has_changes = diff_stats_has_changes(&stats);
                    if has_changes {
                        tracing::warn!(
                            task_id = %task_id.as_str(),
                            base_ref = %task.task_branch_base_ref.as_deref().unwrap_or_default(),
                            base_sha = %task.task_branch_base_sha.as_deref().unwrap_or_default(),
                            files_changed = stats.files_changed,
                            "Reviewer marked approved_no_changes but captured-base diff has code changes — falling back to standard Approved flow"
                        );
                    }
                    has_changes
                }
                Ok(None) => {
                    if let Some(ref branch) = task_branch {
                        let repo_path = std::path::Path::new(&project.working_directory);
                        let base = project.base_branch_or_default();
                        match GitService::branches_have_same_content(repo_path, branch, base).await
                        {
                            Ok(false) => {
                                // Not same content → branch has code changes
                                tracing::warn!(
                                    task_id = %task_id.as_str(),
                                    branch = %branch,
                                    base_branch = %base,
                                    "Reviewer marked approved_no_changes but branch has code changes \
                                     — falling back to standard Approved flow"
                                );
                                true
                            }
                            Ok(true) => false,
                            Err(e) => {
                                tracing::warn!(
                                    task_id = %task_id.as_str(),
                                    error = %e,
                                    "Git diff validation failed for approved_no_changes"
                                );
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    format!(
                                        "Cannot approve as no-changes because git diff validation failed: {e}"
                                    ),
                                ));
                            }
                        }
                    } else {
                        false
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        task_id = %task_id.as_str(),
                        error = %error,
                        "complete_review rejected: approved_no_changes diff check failed"
                    );
                    return Err((StatusCode::BAD_REQUEST, error.to_string()));
                }
            };

            if has_code_changes {
                // Fall back to standard Approved flow (reviewer decision treated as regular Approved)
                transition_ai_review_approval(&state, &transition_service, &task_id, require_human)
                    .await?
            } else {
                if !task_allows_empty_captured_diff(&task) {
                    let base_ref = task
                        .task_branch_base_ref
                        .as_deref()
                        .unwrap_or("<unknown-base-ref>");
                    let base_sha = task
                        .task_branch_base_sha
                        .as_deref()
                        .unwrap_or("<unknown-base-sha>");
                    tracing::warn!(
                        task_id = %task_id.as_str(),
                        base_ref = %base_ref,
                        base_sha = %base_sha,
                        "complete_review rejected: approved_no_changes requires explicit no-code classification"
                    );
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!(
                            "empty_task_diff_against_captured_base: task {} cannot be approved as no-changes because it is not explicitly classified as no-code/no-change",
                            task_id.as_str()
                        ),
                    ));
                }

                // No code changes confirmed — set metadata and skip merge pipeline.
                // Re-fetch task for a fresh mutable copy to avoid borrow conflicts.
                let mut fresh_task = state
                    .app_state
                    .task_repo
                    .get_by_id(&task_id)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                    .ok_or_else(|| {
                        (
                            StatusCode::NOT_FOUND,
                            "Task not found after review bookkeeping".to_string(),
                        )
                    })?;

                set_no_code_changes_metadata(&mut fresh_task);
                set_pending_cleanup_metadata(&mut fresh_task);
                fresh_task.touch();

                state
                    .app_state
                    .task_repo
                    .update(&fresh_task)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                let target_status = transition_ai_review_approval(
                    &state,
                    &transition_service,
                    &task_id,
                    require_human,
                )
                .await?;

                // Direct-to-Merged path: clear merge progress + spawn deferred cleanup
                if !require_human {
                    crate::domain::entities::merge_progress_event::clear_merge_progress(
                        task_id.as_str(),
                    );

                    let project_working_dir = project.working_directory.clone();

                    tokio::spawn(deferred_merge_cleanup(
                        task_id.clone(),
                        Arc::clone(&state.app_state.task_repo),
                        project_working_dir,
                        task_branch,
                        worktree_path,
                        None,
                    ));
                }

                target_status
            }
        }
    };

    persist_followup_activity_event(
        &state,
        &task_id,
        new_status.clone(),
        followup_session_id.as_deref(),
        review_note_id.as_str(),
    )
    .await;

    crate::http_server::emit_http_event(
        &state,
        "review:completed",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "decision": req.decision,
            "new_status": new_status.as_str(),
        }),
    );
    crate::http_server::emit_http_event(
        &state,
        "task:status_changed",
        serde_json::json!({
            "task_id": task_id.as_str(),
            "old_status": task.internal_status.as_str(),
            "new_status": new_status.as_str(),
        }),
    );
    // For direct-to-Merged (approved_no_changes, no human review gate), emit task:merged
    if new_status == InternalStatus::Merged {
        crate::http_server::emit_http_event(
            &state,
            "task:merged",
            serde_json::json!({
                "task_id": task_id.as_str(),
            }),
        );
    }

    // 8. Notify completion signal then close stdin via IPR
    {
        use crate::application::interactive_process_registry::InteractiveProcessKey;
        let key = InteractiveProcessKey::new("review", task_id.as_str());
        if let Some(signal) = state
            .app_state
            .interactive_process_registry
            .get_completion_signal(&key)
            .await
        {
            signal.notify_one();
        }
        if state
            .app_state
            .interactive_process_registry
            .remove(&key)
            .await
            .is_some()
        {
            tracing::info!("IPR removed for reviewer on task {}", task_id.as_str());
        }
    }

    // 9. Return response
    Ok(Json(CompleteReviewResponse {
        success: true,
        message: match followup_conversation_id.as_deref() {
            Some(conversation_id) => format!(
                "Review submitted successfully. Follow-up Agent conversation created: {conversation_id}"
            ),
            None => complete_review_response_message(None),
        },
        new_status: new_status.as_str().to_string(),
        fix_task_id: fix_task_id.map(|id| id.as_str().to_string()),
        followup_session_id,
        followup_conversation_id,
    }))
}

async fn transition_ai_review_approval(
    state: &HttpServerState,
    transition_service: &TaskTransitionService,
    task_id: &TaskId,
    require_human_review: bool,
) -> Result<InternalStatus, (StatusCode, String)> {
    transition_service
        .transition_task(task_id, InternalStatus::ReviewPassed)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if require_human_review {
        return Ok(InternalStatus::ReviewPassed);
    }

    transition_service
        .transition_task(task_id, InternalStatus::Approved)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state
        .app_state
        .task_repo
        .get_by_id(task_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(|task| task.internal_status)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Task not found after approval transition".to_string(),
            )
        })
}

async fn persist_review_scope_snapshot(
    state: &HttpServerState,
    task: &mut crate::domain::entities::Task,
    task_context: &crate::domain::entities::TaskContext,
    scope_drift_classification: Option<ScopeDriftClassification>,
    scope_drift_notes: Option<String>,
) -> Result<(), (StatusCode, String)> {
    task.metadata = update_review_scope_metadata(
        task.metadata.as_deref(),
        task_context,
        scope_drift_classification,
        scope_drift_notes,
    )
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    state
        .app_state
        .task_repo
        .update(task)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(())
}

async fn persist_followup_activity_event(
    state: &HttpServerState,
    task_id: &TaskId,
    new_status: InternalStatus,
    followup_session_id: Option<&str>,
    review_note_id: &str,
) {
    let Some(event) = build_followup_activity_event(
        task_id.clone(),
        new_status,
        followup_session_id,
        review_note_id,
    ) else {
        return;
    };

    if let Err(error) = state.app_state.activity_event_repo.save(event).await {
        tracing::warn!(
            task_id = task_id.as_str(),
            followup_session_id = %followup_session_id.unwrap_or_default(),
            %error,
            "Failed to persist follow-up activity event after review escalation"
        );
    }
}

async fn maybe_register_unrelated_drift_issue(
    state: &HttpServerState,
    task: &crate::domain::entities::Task,
    review: &Review,
    task_context: &crate::domain::entities::TaskContext,
    outcome: ReviewToolOutcome,
    scope_drift_classification: Option<ScopeDriftClassification>,
    revision_count: u32,
    review_settings: &crate::domain::review::ReviewSettings,
    summary: Option<&str>,
    feedback: Option<&str>,
    escalation_reason: Option<&str>,
) -> Option<String> {
    if !should_spawn_unrelated_drift_followup(
        outcome,
        scope_drift_classification,
        revision_count,
        review_settings,
    ) {
        return None;
    }

    let parent_session_id = match task.ideation_session_id.clone() {
        Some(session_id) => session_id,
        None => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                "Cannot register unrelated-drift Agent issue: task has no ideation session"
            );
            return None;
        }
    };

    let origin_workspace = match state
        .app_state
        .agent_conversation_workspace_repo
        .get_by_linked_ideation_session_id(&parent_session_id)
        .await
    {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                "Cannot register unrelated-drift Agent issue: task ideation session is not attached to an Agent conversation"
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                error = %error,
                "Failed to resolve Agent workspace for unrelated-drift issue"
            );
            return None;
        }
    };

    let origin_conversation = match state
        .app_state
        .chat_conversation_repo
        .get_by_id(&origin_workspace.conversation_id)
        .await
    {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                conversation_id = %origin_workspace.conversation_id.as_str(),
                "Cannot register unrelated-drift Agent issue: origin Agent conversation is missing"
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                error = %error,
                "Failed to load origin Agent conversation for unrelated-drift issue"
            );
            return None;
        }
    };

    let draft = build_unrelated_drift_followup_draft(
        task,
        task_context,
        summary,
        feedback,
        escalation_reason,
        revision_count,
        review_settings,
    );

    let mut issue = AgentConversationIssue::new(
        ProjectId::from_string(origin_conversation.context_id.clone()),
        origin_conversation.id.clone(),
        Some(task.id.as_str().to_string()),
        Some("review".to_string()),
        Some(review.id.as_str().to_string()),
        Some("ralphx-execution-reviewer".to_string()),
        "plan_drift".to_string(),
        "high".to_string(),
        "followup_only".to_string(),
        draft.title.clone(),
        draft.description.clone(),
        Some(task_context.out_of_scope_files.join("\n")),
        Some(draft.prompt.clone()),
        draft.blocker_fingerprint.clone(),
        Some(draft.title.clone()),
        Some(draft.prompt.clone()),
        true,
    );
    let canonical_identity =
        canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
            issue_kind: issue.issue_kind.as_str(),
            blocking_scope: issue.blocking_scope.as_str(),
            title: issue.title.as_str(),
            summary: issue.summary.as_str(),
            evidence: issue.evidence.as_deref(),
            recommendation: issue.recommendation.as_deref(),
            blocker_fingerprint: issue.blocker_fingerprint.as_deref(),
            source_task_id: issue.source_task_id.as_deref(),
        });
    issue.apply_canonical_identity(&canonical_identity);
    let mut dedupe_decision = AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED;

    match state
        .app_state
        .agent_conversation_issue_repo
        .find_open_by_canonical_fingerprint(
            &origin_conversation.id,
            &canonical_identity.fingerprint,
        )
        .await
    {
        Ok(Some(mut existing)) => {
            existing.refresh_from(issue);
            issue = existing;
            dedupe_decision = AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED;
        }
        Ok(None) => {
            if let Some(blocker_fingerprint) = issue.blocker_fingerprint.as_deref() {
                match state
                    .app_state
                    .agent_conversation_issue_repo
                    .find_open_by_fingerprint(
                        &origin_conversation.id,
                        Some(task.id.as_str()),
                        "plan_drift",
                        blocker_fingerprint,
                    )
                    .await
                {
                    Ok(Some(mut existing)) => {
                        existing.apply_canonical_identity(&canonical_identity);
                        existing.refresh_from(issue);
                        issue = existing;
                        dedupe_decision = AGENT_CONVERSATION_ISSUE_DEDUPE_EXACT_ATTACHED;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(
                            task_id = %task.id.as_str(),
                            error = %error,
                            "Failed to check for existing unrelated-drift Agent issue"
                        );
                    }
                }
            }
        }
        Err(error) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                error = %error,
                "Failed to check canonical unrelated-drift Agent issue identity"
            );
        }
    }

    let saved_issue = match state
        .app_state
        .agent_conversation_issue_repo
        .save(&issue)
        .await
    {
        Ok(issue) => issue,
        Err(error) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                error = %error,
                "Failed to save unrelated-drift Agent issue"
            );
            return None;
        }
    };
    let occurrence = AgentConversationIssueOccurrence::from_issue(&saved_issue, dedupe_decision);
    if let Err(error) = state
        .app_state
        .agent_conversation_issue_repo
        .append_occurrence(&occurrence)
        .await
    {
        tracing::warn!(
            task_id = %task.id.as_str(),
            issue_id = %saved_issue.id,
            error = %error,
            "Failed to save unrelated-drift Agent issue occurrence"
        );
    }

    if !review_settings.auto_create_followup_agent_conversation {
        return None;
    }

    let request = CreateFollowupAgentConversationRequest {
        origin_conversation_id: Some(origin_conversation.id.as_str()),
        source_task_id: Some(task.id.as_str().to_string()),
        source_context_type: Some("review".to_string()),
        source_context_id: Some(review.id.as_str().to_string()),
        source_agent_name: Some("ralphx-execution-reviewer".to_string()),
        title: draft.title,
        description: Some(draft.description),
        initial_prompt: Some(draft.prompt),
        spawn_reason: Some("out_of_scope_failure".to_string()),
        blocker_fingerprint: draft.blocker_fingerprint,
        provider_harness: None,
        model_override: None,
        logical_effort: None,
    };

    match create_followup_agent_conversation_for_request(state, request).await {
        Ok(response) => {
            let followup_conversation_id =
                ChatConversationId::from_string(response.conversation.id.clone());
            if let Err(error) = state
                .app_state
                .agent_conversation_issue_repo
                .link_followup_conversation(&saved_issue.id, &followup_conversation_id)
                .await
            {
                tracing::warn!(
                    task_id = %task.id.as_str(),
                    issue_id = %saved_issue.id,
                    error = %error,
                    "Failed to link unrelated-drift issue to follow-up Agent conversation"
                );
            }
            Some(response.conversation.id)
        }
        Err((status, body)) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                status = %status,
                body = %body.0,
                "Failed to auto-create follow-up Agent conversation for unrelated scope drift"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "complete_tests.rs"]
mod complete_tests;
