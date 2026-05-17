use super::*;

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
    let followup_session_id = maybe_spawn_unrelated_drift_followup(
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
    let scheduler_concrete = Arc::new(state.app_state.build_task_scheduler_for_runtime(
        Arc::clone(&state.execution_state),
        state.app_state.app_handle.as_ref().cloned(),
    ));
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    let task_scheduler: Arc<dyn TaskScheduler> = scheduler_concrete;

    let mut transition_service_builder = state
        .app_state
        .build_transition_service_with_execution_state(Arc::clone(&state.execution_state))
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
            let project_opt = state
                .app_state
                .project_repo
                .get_by_id(&task.project_id)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            // Git diff validation safety gate (BEFORE metadata persistence).
            // If the branch has code changes, fall back to standard Approved flow.
            let has_code_changes =
                if let (Some(ref project), Some(ref branch)) = (&project_opt, &task_branch) {
                    let repo_path = std::path::Path::new(&project.working_directory);
                    let base = project.base_branch_or_default();
                    match GitService::branches_have_same_content(repo_path, branch, base).await {
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
                        Ok(true) => false, // Same content — no changes, proceed with no-changes path
                        Err(e) => {
                            // Git error — defensive: proceed with no-changes path
                            tracing::warn!(
                                task_id = %task_id.as_str(),
                                error = %e,
                                "Git diff validation failed — proceeding with \
                                 no-changes path (defensive)"
                            );
                            false
                        }
                    }
                } else {
                    // No project or no branch — proceed with no-changes path (defensive)
                    false
                };

            if has_code_changes {
                // Fall back to standard Approved flow (reviewer decision treated as regular Approved)
                transition_ai_review_approval(&state, &transition_service, &task_id, require_human)
                    .await?
            } else {
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

                    let project_working_dir = project_opt
                        .as_ref()
                        .map(|p| p.working_directory.clone())
                        .unwrap_or_default();

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

    // 7. Emit events
    if let Some(app_handle) = &state.app_state.app_handle {
        let _ = app_handle.emit(
            "review:completed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "decision": req.decision,
                "new_status": new_status.as_str(),
            }),
        );
        let _ = app_handle.emit(
            "task:status_changed",
            serde_json::json!({
                "task_id": task_id.as_str(),
                "old_status": task.internal_status.as_str(),
                "new_status": new_status.as_str(),
            }),
        );
        // For direct-to-Merged (approved_no_changes, no human review gate), emit task:merged
        if new_status == InternalStatus::Merged {
            let _ = app_handle.emit(
                "task:merged",
                serde_json::json!({
                    "task_id": task_id.as_str(),
                }),
            );
        }
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
        message: complete_review_response_message(followup_session_id.as_deref()),
        new_status: new_status.as_str().to_string(),
        fix_task_id: fix_task_id.map(|id| id.as_str().to_string()),
        followup_session_id,
    }))
}

async fn transition_ai_review_approval(
    state: &HttpServerState,
    transition_service: &TaskTransitionService<tauri::Wry>,
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

async fn maybe_spawn_unrelated_drift_followup(
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
                "Cannot auto-create follow-up session for unrelated drift: task has no ideation session"
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

    match find_existing_unrelated_drift_followup(
        state,
        &parent_session_id,
        &task.id,
        draft.blocker_fingerprint.as_deref(),
    )
    .await
    {
        Ok(Some(existing_id)) => return Some(existing_id),
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                error = %e,
                "Failed to check for existing unrelated-drift follow-up session"
            );
        }
    }

    let request = CreateChildSessionRequest {
        parent_session_id: parent_session_id.as_str().to_string(),
        title: Some(draft.title),
        description: Some(draft.description),
        inherit_context: true,
        initial_prompt: Some(draft.prompt),
        team_mode: None,
        team_config: None,
        purpose: Some("general".to_string()),
        is_external_trigger: false,
        source_task_id: Some(task.id.as_str().to_string()),
        source_context_type: Some("review".to_string()),
        source_context_id: Some(review.id.as_str().to_string()),
        spawn_reason: Some("out_of_scope_failure".to_string()),
        blocker_fingerprint: draft.blocker_fingerprint,
    };

    match create_child_session_impl(state, request).await {
        Ok(response) => Some(response.session_id),
        Err((status, body)) => {
            tracing::warn!(
                task_id = %task.id.as_str(),
                status = %status,
                body = %body.0,
                "Failed to auto-create follow-up session for unrelated scope drift"
            );
            None
        }
    }
}

async fn find_existing_unrelated_drift_followup(
    state: &HttpServerState,
    parent_session_id: &crate::domain::entities::IdeationSessionId,
    task_id: &TaskId,
    blocker_fingerprint: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let children = state
        .app_state
        .ideation_session_repo
        .get_children(parent_session_id)
        .await?;

    Ok(matching_unrelated_drift_followup_session_id(
        &children,
        task_id,
        blocker_fingerprint,
    ))
}
