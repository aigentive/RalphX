use super::*;
use crate::application::interactive_notification_producer::InteractiveNotificationProducer;
use crate::application::NotificationContextResolver;

// ============================================================================
// POST /api/team/plan/request — Phase 1: validate + store + emit (non-blocking)
// ============================================================================

/// Phase 1 of the two-phase team plan flow.
///
/// Validates the team plan, stores it as pending, registers a watch channel,
/// emits the `team:plan_requested` event for the frontend, and returns the
/// `plan_id` immediately without blocking.
///
/// The MCP caller should then follow up with `GET /api/team/plan/await/:plan_id`
/// to long-poll for the user's decision.
pub async fn request_team_plan_register(
    State(state): State<HttpServerState>,
    Json(req): Json<RequestTeamPlanRequest>,
) -> Result<Json<TeamPlanRegisterResponse>, (StatusCode, String)> {
    ensure_team_mode_supported_for_context(&state, &req.context_type, &req.context_id).await?;

    info!(
        process = %req.process,
        teammate_count = req.teammates.len(),
        "Team plan requested — validating constraints"
    );

    // Load constraints from ralphx.yaml
    let config = team_constraints_config();
    let constraints = get_team_constraints(config, &req.process);

    // Convert HTTP request teammates to validation type
    let spawn_requests: Vec<TeammateSpawnRequest> = req
        .teammates
        .iter()
        .map(|t| TeammateSpawnRequest {
            role: t.role.clone(),
            prompt: None,
            preset: t.preset.clone(),
            tools: t.tools.clone(),
            mcp_tools: t.mcp_tools.clone(),
            model: t.model.clone(),
            prompt_summary: Some(t.prompt_summary.clone()),
        })
        .collect();

    // Validate the plan against constraints
    let plan = validate_team_plan(&constraints, &req.process, &spawn_requests).map_err(|e| {
        warn!(process = %req.process, error = %e, "Team plan validation failed");
        (
            StatusCode::BAD_REQUEST,
            format!("Team plan validation failed: {e}"),
        )
    })?;

    let plan_id = plan.plan_id.clone();

    info!(
        plan_id = %plan_id,
        process = %req.process,
        approved_teammates = plan.teammates.len(),
        "Team plan validated — storing and emitting for user approval"
    );

    // Store the plan with full prompts for batch-spawn on approval.
    // store_pending_plan enforces single-plan-per-context and GCs stale entries.
    let pending_teammates: Vec<PendingTeammate> = req
        .teammates
        .iter()
        .zip(plan.teammates.iter())
        .map(|(req_t, approved_t)| PendingTeammate {
            role: approved_t.role.clone(),
            prompt: req_t
                .prompt
                .clone()
                .unwrap_or_else(|| req_t.prompt_summary.clone()),
            tools: approved_t.approved_tools.clone(),
            mcp_tools: approved_t.approved_mcp_tools.clone(),
            model: approved_t.approved_model.clone(),
            preset: req_t.preset.clone(),
        })
        .collect();

    state
        .team_tracker
        .store_pending_plan(PendingTeamPlan {
            plan_id: plan_id.clone(),
            context_type: req.context_type.clone(),
            context_id: req.context_id.clone(),
            process: req.process.clone(),
            teammates: pending_teammates,
            created_at: Utc::now(),
            team_name: req.team_name.clone(),
            lead_session_id: req.lead_session_id.clone(),
        })
        .await;

    // Register watch channel AFTER storing (before emitting to avoid race)
    state.team_tracker.register_plan_channel(&plan_id).await;

    let auto_approve = constraints.auto_approve.unwrap_or(true);

    if auto_approve {
        // Auto-approve: take the pending plan and spawn teammates immediately.
        // Channel cleanup on failure is handled by execute_team_spawn's internal rollback.
        let plan = state
            .team_tracker
            .take_pending_plan(&plan_id)
            .await
            .ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to retrieve pending plan after storage".to_string(),
            ))?;

        match execute_team_spawn(&state, plan, &plan_id).await {
            Ok(spawn_result) => {
                crate::http_server::emit_http_event(
                    &state,
                    "team:plan_auto_approved",
                    serde_json::json!({
                        "plan_id": plan_id,
                        "context_type": req.context_type,
                        "context_id": req.context_id,
                        "process": req.process,
                        "team_name": spawn_result.team_name,
                        "teammates_spawned": spawn_result.spawned_teammates,
                        "message": spawn_result.message,
                    }),
                );
                info!(plan_id = %plan_id, "Emitted team:plan_auto_approved event");
                state.team_tracker.remove_plan_channel(&plan_id).await;
                Ok(Json(TeamPlanRegisterResponse {
                    success: true,
                    plan_id: plan_id.clone(),
                    message: format!("Plan auto-approved: {}", spawn_result.message),
                    auto_approved: true,
                    teammates_spawned: spawn_result.spawned_teammates.clone(),
                }))
            }
            Err(e) => {
                // Channel already cleaned up by execute_team_spawn's internal rollback
                Err(e)
            }
        }
    } else {
        record_team_plan_requested_notification(&state, &plan_id, &req).await;

        // Manual flow: emit team:plan_requested for frontend approval dialog
        crate::http_server::emit_http_event(
            &state,
            "team:plan_requested",
            serde_json::json!({
                "plan_id": plan_id,
                "context_type": req.context_type,
                "context_id": req.context_id,
                "process": req.process,
                "teammates": req.teammates,
                "validated": true,
                "created_at": Utc::now().to_rfc3339(),
            }),
        );
        info!(plan_id = %plan_id, "Emitted team:plan_requested event");

        Ok(Json(TeamPlanRegisterResponse {
            success: true,
            plan_id,
            message: "Team plan submitted for approval".to_string(),
            auto_approved: false,
            teammates_spawned: vec![],
        }))
    }
}

async fn record_team_plan_requested_notification(
    state: &HttpServerState,
    plan_id: &str,
    request: &RequestTeamPlanRequest,
) {
    let notification_context = NotificationContextResolver::from_app_state(&state.app_state);
    match notification_context
        .resolve_context_target(&request.context_type, &request.context_id)
        .await
    {
        Ok(resolved) => {
            state
                .app_state
                .notification_service()
                .record(InteractiveNotificationProducer::team_plan_approval(
                    plan_id,
                    &request.process,
                    resolved,
                ))
                .await;
        }
        Err(error) => {
            tracing::warn!(error = %error, plan_id, "Failed to resolve team plan notification context");
        }
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;

// ============================================================================
// GET /api/team/plan/await/:plan_id — Phase 2: long-poll until decision (840s)
// ============================================================================

/// Phase 2 of the two-phase team plan flow.
///
/// Long-polls the watch channel for up to 840s (14 min) waiting for the user
/// to approve or reject the plan. Checks for an already-resolved decision before
/// entering the wait loop to handle the race where approval arrives between
/// Phase 1 and Phase 2.
///
/// Returns 408 on timeout (backend always responds before the MCP 900s AbortController).
pub async fn await_team_plan(
    State(state): State<HttpServerState>,
    Path(plan_id): Path<String>,
) -> Result<Json<RequestTeamPlanResponse>, (StatusCode, String)> {
    // Get receiver for this plan_id — 404 if not registered
    let mut rx = match state.team_tracker.subscribe_plan_channel(&plan_id).await {
        Some(rx) => rx,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("No pending plan found with id '{plan_id}'"),
            ));
        }
    };

    // ── Long-poll until decision (840s = 14 min) ────────────────────────
    // Backend fires at 840s so MCP AbortController at 900s is never the first to fire.
    let timeout = tokio::time::Duration::from_secs(840);
    let start = tokio::time::Instant::now();

    let decision = loop {
        // Check immediately for already-resolved decision (race guard).
        let maybe_decision: Option<PlanDecision> = {
            let current = rx.borrow();
            current.clone()
        };

        if let Some(decision) = maybe_decision {
            break decision;
        }

        // Check timeout
        if start.elapsed() >= timeout {
            state.team_tracker.remove_plan_channel(&plan_id).await;
            return Ok(Json(RequestTeamPlanResponse {
                success: false,
                plan_id,
                team_name: None,
                teammates_spawned: vec![],
                message: "Team plan timed out waiting for user approval (14 min). Plan is still pending — user can still approve in the UI.".to_string(),
            }));
        }

        // Wait for change signal with remaining timeout
        let remaining = timeout.saturating_sub(start.elapsed());
        match tokio::time::timeout(remaining, rx.changed()).await {
            Ok(Ok(())) => continue,
            Ok(Err(_)) => {
                // Channel closed (superseded by a newer plan for this context)
                return Ok(Json(RequestTeamPlanResponse {
                    success: false,
                    plan_id,
                    team_name: None,
                    teammates_spawned: vec![],
                    message: "Plan was superseded by a newer plan for this context".to_string(),
                }));
            }
            Err(_) => {
                // Tokio timeout — remove channel, keep plan alive for UI
                state.team_tracker.remove_plan_channel(&plan_id).await;
                return Ok(Json(RequestTeamPlanResponse {
                    success: false,
                    plan_id,
                    team_name: None,
                    teammates_spawned: vec![],
                    message: "Team plan timed out waiting for user approval (14 min). Plan is still pending — user can still approve in the UI.".to_string(),
                }));
            }
        }
    };

    // Cleanup channel
    state.team_tracker.remove_plan_channel(&plan_id).await;

    // Build response from the decision
    let teammates_spawned: Vec<SpawnedTeammateInfo> = decision
        .teammates_spawned
        .iter()
        .map(|t| SpawnedTeammateInfo {
            name: t.name.clone(),
            role: t.role.clone(),
            model: t.model.clone(),
            color: t.color.clone(),
        })
        .collect();

    Ok(Json(RequestTeamPlanResponse {
        success: decision.approved,
        plan_id,
        team_name: decision.team_name,
        teammates_spawned,
        message: decision.message,
    }))
}

// ============================================================================
// GET /api/team/plan/pending/:context_id — Frontend reconciliation
// ============================================================================

/// Return the pending plan for a context_id (if any).
///
/// Used by the frontend on mount to detect stale plans that arrived while
/// the component was unmounted (e.g., page reload between Phase 1 and Phase 2).
pub async fn get_pending_plan(
    State(state): State<HttpServerState>,
    Path(context_id): Path<String>,
) -> Json<GetPendingPlanResponse> {
    match state
        .team_tracker
        .get_pending_plan_for_context(&context_id)
        .await
    {
        Some(plan) => Json(GetPendingPlanResponse {
            has_pending: true,
            plan_id: Some(plan.plan_id),
            context_id: plan.context_id,
            process: Some(plan.process),
            teammate_count: Some(plan.teammates.len()),
            created_at: Some(plan.created_at.to_rfc3339()),
        }),
        None => Json(GetPendingPlanResponse {
            has_pending: false,
            plan_id: None,
            context_id,
            process: None,
            teammate_count: None,
            created_at: None,
        }),
    }
}

// ============================================================================
// POST /api/team/plan/approve — Approve a team plan and batch-spawn teammates
// ============================================================================

/// Thin wrapper: validates the channel is still alive, takes the pending plan,
/// and delegates all spawn logic to `execute_team_spawn`.
///
/// The approve-at-timeout guard is manual-path-only — auto-approve runs within
/// Phase 1 where the channel was just registered and cannot be stale.
pub async fn approve_team_plan(
    State(state): State<HttpServerState>,
    Json(req): Json<ApproveTeamPlanRequest>,
) -> Result<Json<ApproveTeamPlanResponse>, (StatusCode, String)> {
    info!(
        plan_id = %req.plan_id,
        context_type = %req.context_type,
        context_id = %req.context_id,
        "Team plan approval requested — batch-spawning teammates"
    );

    // Approve-at-timeout guard: refuse approval if watch channel is gone.
    // When the 840s backend timeout fires in `await_team_plan`, it removes the channel.
    // If the channel is gone, the agent already received a timeout response — spawning
    // teammates now would create orphaned processes that nobody is waiting for.
    if !state.team_tracker.plan_channel_exists(&req.plan_id).await {
        warn!(plan_id = %req.plan_id, "Approve rejected: plan channel gone (agent already received timeout)");
        return Err((
            StatusCode::CONFLICT,
            "Plan expired — agent already received timeout. Cannot approve expired plan."
                .to_string(),
        ));
    }

    // 1. Take the pending plan (removes it from store)
    let plan = state
        .team_tracker
        .take_pending_plan(&req.plan_id)
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("No pending plan found with id '{}'", req.plan_id),
            )
        })?;

    let result = execute_team_spawn(&state, plan, &req.plan_id).await?;
    Ok(Json(ApproveTeamPlanResponse {
        success: result.spawned_count > 0,
        team_name: result.team_name,
        teammates_spawned: result.spawned_teammates,
        message: result.message,
    }))
}

pub async fn reject_team_plan(
    State(state): State<HttpServerState>,
    Json(req): Json<RejectTeamPlanRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    info!(plan_id = %req.plan_id, "Team plan rejected by user");

    // Remove the pending plan
    state.team_tracker.take_pending_plan(&req.plan_id).await;

    // Signal the blocking handler with rejection
    state
        .team_tracker
        .resolve_plan(
            &req.plan_id,
            PlanDecision {
                approved: false,
                team_name: None,
                teammates_spawned: vec![],
                message: "Team plan rejected by user".to_string(),
            },
        )
        .await;

    Ok(StatusCode::OK)
}
