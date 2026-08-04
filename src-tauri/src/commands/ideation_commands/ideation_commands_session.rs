// Session management commands

use std::path::PathBuf;
use std::sync::Arc;

use ralphx_events::emit_serialized;
use tauri::State;

use ralphx_domain::entities::EventType;

use crate::application::agent_planning_session_titles::{
    hydrate_agent_conversation_planning_session_title,
    hydrate_agent_conversation_planning_session_titles,
};
use crate::application::git_service::GitService;
use crate::application::ideation_workspace::{
    prepare_ideation_analysis_state, IdeationAnalysisBaseSelection,
};
use crate::application::session_namer_agent::{spawn_session_namer_agent, SessionNamerTarget};
use crate::application::AppState;
use crate::application::{StopMode, TaskCleanupService};
use crate::commands::execution_task_navigation::resolve_agent_workspace_target_for_ideation_session;
use crate::domain::entities::plan_branch::PlanBranchStatus;
use crate::domain::entities::{
    IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId, SessionPurpose, TaskId,
};
use crate::domain::execution::ExecutionTaskAgentWorkspace;

use super::ideation_commands_types::{
    ChatMessageResponse, CreateSessionInput, IdeationSessionResponse,
    IdeationSessionWithProgressResponse, LatestChildSessionIdResponse, SessionGroupCountsResponse,
    SessionListResponse, SessionWithDataResponse, TaskProposalResponse,
};

// ============================================================================
// Session Management Commands
// ============================================================================

/// Resolve the active Agent workspace that owns a linked ideation session.
#[tauri::command]
pub async fn get_ideation_agent_workspace(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<Option<ExecutionTaskAgentWorkspace>, String> {
    get_ideation_agent_workspace_for_app_state(session_id, state.inner()).await
}

#[doc(hidden)]
pub(crate) async fn get_ideation_agent_workspace_for_app_state(
    session_id: String,
    state: &AppState,
) -> Result<Option<ExecutionTaskAgentWorkspace>, String> {
    resolve_agent_workspace_target_for_ideation_session(
        state,
        &IdeationSessionId::from_string(session_id),
    )
    .await
}

/// Core implementation for creating an ideation session.
/// Generic over Runtime to enable unit testing with MockRuntime.
#[doc(hidden)]
pub async fn create_ideation_session_impl<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    state: &AppState,
    input: CreateSessionInput,
) -> Result<IdeationSessionResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id.as_str()))?;
    let seed_task_id = input.seed_task_id.map(TaskId::from_string);
    let session_id = IdeationSessionId::new();
    let analysis = prepare_ideation_analysis_state(
        &project,
        &session_id,
        IdeationAnalysisBaseSelection {
            kind: input.analysis_base_ref_kind,
            base_ref: input.analysis_base_ref,
            display_name: input.analysis_base_display_name,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut builder = IdeationSession::builder()
        .id(session_id)
        .project_id(project_id)
        .analysis(analysis);

    if let Some(title) = input.title {
        builder = builder.title(title);
    }

    if let Some(task_id) = seed_task_id {
        builder = builder.seed_task_id(task_id);
    }

    let session = builder.build();

    let created = state
        .ideation_session_repo
        .create(session)
        .await
        .map_err(|e| e.to_string())?;

    let payload_json = serde_json::json!({
        "sessionId": created.id.to_string(),
        "projectId": created.project_id.to_string(),
    });
    let _ = emit_serialized(
        state.events.as_ref(),
        "ideation:session_created",
        &payload_json,
    );

    // Layer 2: persist to external_events table (non-fatal)
    if let Err(e) = state
        .external_events_repo
        .insert_event(
            "ideation:session_created",
            &created.project_id.to_string(),
            &payload_json.to_string(),
        )
        .await
    {
        tracing::warn!(error = %e, "Failed to persist IdeationSessionCreated event");
    }

    // Layer 3: webhook push (fire-and-forget, non-fatal)
    if let Some(ref publisher) = state.webhook_publisher {
        let _ = publisher
            .publish(
                EventType::IdeationSessionCreated,
                &created.project_id.to_string(),
                payload_json,
            )
            .await;
    }

    Ok(IdeationSessionResponse::from(created))
}

/// Create a new ideation session
#[tauri::command]
pub async fn create_ideation_session(
    input: CreateSessionInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<IdeationSessionResponse, String> {
    create_ideation_session_impl(&app, &state, input).await
}

/// Get a single ideation session by ID
#[tauri::command]
pub async fn get_ideation_session(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<IdeationSessionResponse>, String> {
    let session_id = IdeationSessionId::from_string(id);
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    match session {
        Some(session) => hydrate_agent_conversation_planning_session_title(&state, session)
            .await
            .map(|session| Some(IdeationSessionResponse::from(session)))
            .map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// Get session with proposals and messages
#[tauri::command]
pub async fn get_ideation_session_with_data(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<SessionWithDataResponse>, String> {
    let session_id = IdeationSessionId::from_string(id);

    // Get session
    let session = match state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(s) => s,
        None => return Ok(None),
    };
    let session = hydrate_agent_conversation_planning_session_title(&state, session)
        .await
        .map_err(|e| e.to_string())?;

    // Get proposals
    let proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;

    // Get messages
    let messages = state
        .chat_message_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(Some(SessionWithDataResponse {
        session: IdeationSessionResponse::from(session),
        proposals: proposals
            .into_iter()
            .map(TaskProposalResponse::from)
            .collect(),
        messages: messages
            .into_iter()
            .map(ChatMessageResponse::from)
            .collect(),
    }))
}

/// List all ideation sessions for a project, optionally filtered by purpose
#[tauri::command]
pub async fn list_ideation_sessions(
    project_id: String,
    purpose: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<IdeationSessionResponse>, String> {
    let project_id = ProjectId::from_string(project_id);
    let sessions = state
        .ideation_session_repo
        .get_by_project(&project_id)
        .await
        .map_err(|e| e.to_string())?;
    let sessions = sessions
        .into_iter()
        .filter(|s| {
            if let Some(ref p) = purpose {
                s.session_purpose.to_string() == p.as_str()
            } else {
                true
            }
        })
        .collect();
    hydrate_agent_conversation_planning_session_titles(&state, sessions)
        .await
        .map(|sessions| {
            sessions
                .into_iter()
                .map(IdeationSessionResponse::from)
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// Archive an ideation session
#[tauri::command]
pub async fn archive_ideation_session(
    id: String,
    state: State<'_, AppState>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    let session_id = IdeationSessionId::from_string(id.clone());

    // Get session to retrieve project_id
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {}", id))?;

    // 1. Get all tasks for this session
    let tasks = state
        .task_repo
        .get_by_ideation_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;

    // 2. Stop ideation agent and clean up tasks (archive them, stop agents, clean git)
    let cleanup_service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.events),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));

    // Stop the ideation session agent first
    let found = cleanup_service.stop_ideation_session_agent(&id).await;
    if !found {
        tracing::debug!(
            session_id = id.as_str(),
            "archive_ideation_session: no running agent found in IPR (expected for sessions without active agents)"
        );
    }

    // Archive all session tasks (stop agents, clean git branches/worktrees, archive in DB)
    let report = cleanup_service
        .cleanup_tasks(&tasks, StopMode::DirectStop, true)
        .await;
    if !report.errors.is_empty() {
        tracing::warn!(
            session_id = id.as_str(),
            errors = ?report.errors,
            "Some tasks failed during session archive cleanup"
        );
    }

    // 3. Clean up plan branch (best-effort)
    if let Ok(Some(plan_branch)) = state.plan_branch_repo.get_by_session_id(&session_id).await {
        // Best-effort delete the git feature branch
        let project = state
            .project_repo
            .get_by_id(&session.project_id)
            .await
            .ok()
            .flatten();

        if let Some(project) = project {
            let repo_path = PathBuf::from(&project.working_directory);
            if let Err(e) =
                GitService::delete_feature_branch(&repo_path, &plan_branch.branch_name).await
            {
                tracing::warn!(
                    branch = plan_branch.branch_name.as_str(),
                    error = %e,
                    "Failed to delete git feature branch during session archive (best-effort)"
                );
            }
        }

        // Mark plan branch as Abandoned
        if let Err(e) = state
            .plan_branch_repo
            .update_status(&plan_branch.id, PlanBranchStatus::Abandoned)
            .await
        {
            tracing::warn!(
                plan_branch_id = plan_branch.id.as_str(),
                error = %e,
                "Failed to mark plan branch as Abandoned during session archive"
            );
        }
    }

    // 4. Archive the session
    state
        .ideation_session_repo
        .update_status(&session_id, IdeationSessionStatus::Archived)
        .await
        .map_err(|e| e.to_string())
}

/// Delete an ideation session with cascade: stop active agents, delete tasks, clean up plan branch
#[tauri::command]
pub async fn delete_ideation_session(
    id: String,
    state: State<'_, AppState>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    let session_id = IdeationSessionId::from_string(id.clone());

    // Get session to retrieve project_id for events
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {}", id))?;

    // 1. Get all tasks for this session
    let tasks = state
        .task_repo
        .get_by_ideation_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;

    // 2. Clean up tasks: stop agents, clean git branches/worktrees, delete from DB, emit events
    let cleanup_service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.events),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));
    let report = cleanup_service
        .cleanup_tasks(&tasks, StopMode::DirectStop, true)
        .await;
    if !report.errors.is_empty() {
        tracing::warn!(
            session_id = id.as_str(),
            errors = ?report.errors,
            "Some tasks failed during session deletion cleanup"
        );
    }

    // 3. Clean up plan branch (best-effort)
    if let Ok(Some(plan_branch)) = state.plan_branch_repo.get_by_session_id(&session_id).await {
        // Best-effort delete the git feature branch
        let project = state
            .project_repo
            .get_by_id(&session.project_id)
            .await
            .ok()
            .flatten();

        if let Some(project) = project {
            let repo_path = PathBuf::from(&project.working_directory);
            if let Err(e) =
                GitService::delete_feature_branch(&repo_path, &plan_branch.branch_name).await
            {
                tracing::warn!(
                    branch = plan_branch.branch_name.as_str(),
                    error = %e,
                    "Failed to delete git feature branch during session deletion (best-effort)"
                );
            }
        }

        // Mark plan branch as Abandoned
        if let Err(e) = state
            .plan_branch_repo
            .update_status(&plan_branch.id, PlanBranchStatus::Abandoned)
            .await
        {
            tracing::warn!(
                plan_branch_id = plan_branch.id.as_str(),
                error = %e,
                "Failed to mark plan branch as Abandoned during session deletion"
            );
        }
    }

    // 4. Delete the session (existing CASCADE handles proposals/messages)
    state
        .ideation_session_repo
        .delete(&session_id)
        .await
        .map_err(|e| e.to_string())
}

/// Reopen an accepted/archived ideation session back to Active.
///
/// Cleanup: stops running agents, deletes tasks, cleans git branches/worktrees,
/// clears proposal task links, resets session to Active.
/// Emits ideation:session_reopened and task:list_changed events.
#[tauri::command]
pub async fn reopen_ideation_session(
    id: String,
    state: State<'_, AppState>,
    _app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::application::session_reopen_service::SessionReopenService;
    use crate::application::task_cleanup_service::TaskCleanupService;

    let session_id = IdeationSessionId::from_string(id.clone());

    // Get project_id for events before reopening
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found: {}", id))?;
    let project_id_str = session.project_id.as_str().to_string();

    // Stop any running ideation agent before reopening (separate instance; task_cleanup is consumed
    // by SessionReopenService::new() and all deps are Arc<> clones so two instances are cheap)
    let stop_cleanup = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.events),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));
    let found = stop_cleanup.stop_ideation_session_agent(&id).await;
    if !found {
        tracing::debug!(
            session_id = id.as_str(),
            "reopen_ideation_session: no running agent found in IPR (expected for sessions without active agents)"
        );
    }

    let task_cleanup = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.events),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));

    let service = SessionReopenService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.task_proposal_repo),
        Arc::clone(&state.ideation_session_repo),
        Arc::clone(&state.plan_branch_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.execution_plan_repo),
        task_cleanup,
    );

    service
        .reopen(&session_id, &state)
        .await
        .map_err(|e| e.to_string())?;

    // Emit events for real-time UI updates
    let _ = emit_serialized(
        state.events.as_ref(),
        "ideation:session_reopened",
        &serde_json::json!({
            "sessionId": id,
            "projectId": project_id_str,
        }),
    );
    let _ = emit_serialized(
        state.events.as_ref(),
        "task:list_changed",
        &serde_json::json!({
            "projectId": project_id_str,
        }),
    );

    Ok(())
}

/// Update the title of an ideation session
///
/// Sets or clears the session title and emits a real-time event for UI updates.
/// This is used by the ralphx-utility-session-namer agent for auto-generated titles and
/// by the frontend for manual renames.
#[tauri::command]
pub async fn update_ideation_session_title(
    id: String,
    title: Option<String>,
    state: State<'_, AppState>,
    _app: tauri::AppHandle,
) -> Result<IdeationSessionResponse, String> {
    let session_id = IdeationSessionId::from_string(id.clone());

    // Update the title in the database (UI rename = user-set title)
    state
        .ideation_session_repo
        .update_title(&session_id, title.clone(), "user")
        .await
        .map_err(|e| e.to_string())?;

    // Get the updated session to return
    let session = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session not found after update: {}", id))?;

    // Emit event for real-time UI updates
    let _ = emit_serialized(
        state.events.as_ref(),
        "ideation:session_title_updated",
        &serde_json::json!({
            "sessionId": id,
            "title": title,
        }),
    );

    Ok(IdeationSessionResponse::from(session))
}

/// Spawn the ralphx-utility-session-namer agent to auto-generate a title for the session
///
/// This is a fire-and-forget operation that spawns a background agent.
/// The agent will call the update_session_title MCP tool when complete,
/// which will emit an event for real-time UI updates.
#[tauri::command]
pub async fn spawn_session_namer(
    session_id: Option<String>,
    conversation_id: Option<String>,
    first_message: String,
    provider_harness: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let requested_harness = provider_harness
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse)
        .transpose()
        .map_err(|error| format!("Invalid provider harness for session namer: {error}"))?;
    let target = SessionNamerTarget::from_initial_request(
        session_id,
        conversation_id,
        first_message,
        requested_harness,
        None,
    )
    .map_err(str::to_string)?;
    spawn_session_namer_agent(&state, target)
        .await
        .map_err(|error| error.to_string())
}

/// Get child sessions for a parent session, with optional purpose filter.
/// Returns all non-archived children. When purpose is provided, filters by session_purpose.
#[tauri::command]
pub async fn get_child_sessions(
    session_id: String,
    purpose: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<IdeationSessionResponse>, String> {
    let parent_id = IdeationSessionId::from_string(session_id);
    state
        .ideation_session_repo
        .get_children(&parent_id)
        .await
        .map(|mut sessions| {
            // Exclude archived children
            sessions.retain(|s| {
                use crate::domain::entities::ideation::IdeationSessionStatus;
                s.status != IdeationSessionStatus::Archived
            });
            // Optionally filter by purpose
            if let Some(ref p) = purpose {
                sessions.retain(|s| &s.session_purpose.to_string() == p);
            }
            sessions
                .into_iter()
                .map(IdeationSessionResponse::from)
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// Get the latest child session ID for a parent session, with optional purpose filter.
/// This is the lightweight alternative to `get_child_sessions` for UI chrome that only
/// needs a target session ID.
#[tauri::command]
pub async fn get_latest_child_session_id(
    session_id: String,
    purpose: Option<String>,
    include_archived: Option<bool>,
    state: State<'_, AppState>,
) -> Result<LatestChildSessionIdResponse, String> {
    let parent_id = IdeationSessionId::from_string(session_id);
    let parsed_purpose = purpose
        .as_deref()
        .map(str::parse::<SessionPurpose>)
        .transpose()?;
    let latest_child_session_id = state
        .ideation_session_repo
        .get_latest_child_session_id(&parent_id, parsed_purpose, include_archived.unwrap_or(true))
        .await
        .map_err(|e| e.to_string())?
        .map(|id| id.as_str().to_string());

    Ok(LatestChildSessionIdResponse {
        session_id: parent_id.as_str().to_string(),
        purpose,
        latest_child_session_id,
    })
}

/// Get group counts for all 5 session display groups for a project
///
/// Returns counts for: drafts (active sessions), in_progress (accepted + has active tasks),
/// accepted (accepted + no active tasks), done (accepted + all tasks terminal), archived.
/// Optional `search` filters by case-insensitive title substring match.
#[tauri::command]
pub async fn get_session_group_counts(
    project_id: String,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<SessionGroupCountsResponse, String> {
    let project_id = crate::domain::entities::ProjectId::from_string(project_id);
    let counts = state
        .ideation_session_repo
        .get_group_counts(&project_id, search.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    Ok(SessionGroupCountsResponse {
        drafts: counts.drafts,
        in_progress: counts.in_progress,
        accepted: counts.accepted,
        done: counts.done,
        archived: counts.archived,
    })
}

/// List paginated sessions for a specific group
///
/// Valid groups: "drafts", "in_progress", "accepted", "done", "archived".
/// Returns sessions with server-computed progress data and parent session title.
/// Optional `search` filters by case-insensitive title substring match.
#[tauri::command]
pub async fn list_sessions_by_group(
    project_id: String,
    group: String,
    offset: Option<u32>,
    limit: Option<u32>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<SessionListResponse, String> {
    // Validate group early for a clear error message
    match group.as_str() {
        "drafts" | "in_progress" | "accepted" | "done" | "archived" => {}
        _ => {
            return Err(format!(
                "Unknown session group: '{}'. Valid groups: drafts, in_progress, accepted, done, archived",
                group
            ))
        }
    }

    let project_id = crate::domain::entities::ProjectId::from_string(project_id);
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(20);

    let (sessions, total) = state
        .ideation_session_repo
        .list_by_group(&project_id, &group, offset, limit, search.as_deref())
        .await
        .map_err(|e| e.to_string())?;

    let has_more = (offset + sessions.len() as u32) < total;

    let response_sessions = sessions
        .into_iter()
        .map(IdeationSessionWithProgressResponse::from)
        .collect();

    Ok(SessionListResponse {
        sessions: response_sessions,
        total,
        has_more,
        offset,
    })
}
