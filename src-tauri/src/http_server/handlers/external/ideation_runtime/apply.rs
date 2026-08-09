use super::*;
use crate::application::spawn_ready_task_scheduler_if_needed;
use crate::http_server::handlers::ideation::stop_verification_children;

/// Request body for `POST /api/external/apply_proposals`.
///
/// Maps to [`ApplyProposalsInput`] used by the Tauri IPC path. The `target_column`
/// defaults to `"auto"` so task status is determined from dependency graph automatically.
#[derive(Debug, Deserialize)]
pub struct ExternalApplyProposalsRequest {
    pub session_id: String,
    pub proposal_ids: Vec<String>,
    /// Controls initial task placement. Use `"auto"` (default) to derive status from
    /// the dependency graph: tasks with no blockers → Ready, with blockers → Blocked.
    #[serde(default = "external_apply_default_column")]
    pub target_column: String,
    /// Per-plan override for the base branch. External callers can specify a custom branch;
    /// the backend validates it exists locally (see apply_proposals_core).
    #[serde(default)]
    pub base_branch_override: Option<String>,
}

fn external_apply_default_column() -> String {
    "auto".to_string()
}

impl From<ExternalApplyProposalsRequest> for ApplyProposalsInput {
    fn from(req: ExternalApplyProposalsRequest) -> Self {
        Self {
            session_id: req.session_id,
            proposal_ids: req.proposal_ids,
            target_column: req.target_column,
            base_branch_override: req.base_branch_override,
        }
    }
}

/// Response body for `POST /api/external/apply_proposals`.
#[derive(Debug, Serialize)]
pub struct ExternalApplyProposalsResponse {
    pub created_task_ids: Vec<String>,
    /// Number of proposal-to-proposal dependency edges created (excludes merge task edges).
    pub dependencies_created: usize,
    /// Number of plan tasks created (excludes the auto-generated merge task).
    pub tasks_created: usize,
    /// Human-readable summary of the finalization result.
    pub message: Option<String>,
    pub warnings: Vec<String>,
    pub session_converted: bool,
    pub execution_plan_id: Option<String>,
}

/// POST /api/external/apply_proposals
///
/// Apply accepted proposals to the Kanban board from the external MCP path.
///
/// Enforces:
/// 1. **Project scope** — the caller's API key must have access to the session's project.
/// 2. **Verification gate** — the plan must pass `check_verification_gate` before
///    proposals are accepted. Full enforcement requires Wave 1 schema migration.
///
/// Like the Tauri IPC path (`apply_proposals_to_kanban`), this endpoint triggers the
/// task scheduler when any tasks are created in Ready status, so execution starts
/// immediately without waiting for the ReadyWatchdog (30-90s delay).
pub async fn external_apply_proposals(
    State(state): State<HttpServerState>,
    scope: ProjectScope,
    Json(req): Json<ExternalApplyProposalsRequest>,
) -> Result<Json<ExternalApplyProposalsResponse>, HttpError> {
    let session_id = IdeationSessionId::from_string(req.session_id.clone());

    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| {
            error!("Failed to get session {}: {}", req.session_id, e);
            HttpError::from(StatusCode::INTERNAL_SERVER_ERROR)
        })?
        .ok_or_else(|| HttpError::from(StatusCode::NOT_FOUND))?;

    session.assert_project_scope(&scope)?;

    // Note: check_verification_gate is NOT called here — it is the canonical gate inside
    // apply_proposals_core (which resolves EffectiveGatePolicy from session.origin).
    // assert_project_scope above handles auth; session fetch above validates ownership.

    let result = apply_proposals_core(&state.app_state, &state.execution_state, req.into())
        .await
        .map_err(|e| {
            error!("apply_proposals_core failed: {}", e);
            HttpError::validation(e.to_string())
        })?;

    if result.session_converted {
        let task_cleanup = TaskCleanupService::new(
            Arc::clone(&state.app_state.task_repo),
            Arc::clone(&state.app_state.project_repo),
            Arc::clone(&state.app_state.running_agent_registry),
            Arc::clone(&state.app_state.events),
        )
        .with_interactive_process_registry(Arc::clone(
            &state.app_state.interactive_process_registry,
        ));

        let stopped = task_cleanup
            .stop_ideation_session_agent(&result.session_id)
            .await;
        if !stopped {
            tracing::warn!(
                session_id = %result.session_id,
                "IPR cleanup: no running process found for accepted session (HTTP path)"
            );
        }

        // Stop and archive any running verification child agents (best-effort).
        stop_verification_children(&result.session_id, &state.app_state)
            .await
            .ok();
    }

    tracing::info!(
        session_id = %session_id.as_str(),
        created = result.created_task_ids.len(),
        "External apply_proposals completed"
    );

    // Trigger scheduler to pick up newly Ready tasks (ready_settle_ms delay)
    // This is necessary because tasks are set via direct repo update, bypassing TransitionHandler
    spawn_ready_task_scheduler_if_needed(
        &state.app_state,
        Arc::clone(&state.execution_state),
        None,
        result.any_ready_tasks,
    );

    Ok(Json(ExternalApplyProposalsResponse {
        created_task_ids: result.created_task_ids,
        dependencies_created: result.dependencies_created,
        tasks_created: result.tasks_created,
        message: result.message,
        warnings: result.warnings,
        session_converted: result.session_converted,
        execution_plan_id: result.execution_plan_id,
    }))
}
