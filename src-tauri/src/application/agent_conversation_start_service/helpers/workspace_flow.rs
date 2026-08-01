use super::*;

pub(crate) fn agent_mode_requires_workspace(mode: AgentConversationWorkspaceMode) -> bool {
    matches!(
        mode,
        AgentConversationWorkspaceMode::Edit
            | AgentConversationWorkspaceMode::Plan
            | AgentConversationWorkspaceMode::Tasks
            | AgentConversationWorkspaceMode::Autopilot
            | AgentConversationWorkspaceMode::Ideation
            | AgentConversationWorkspaceMode::ReviewPr
    )
}

pub(crate) fn agent_mode_should_create_workspace(
    mode: AgentConversationWorkspaceMode,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
    has_plan_reference: bool,
) -> bool {
    if matches!(
        mode,
        AgentConversationWorkspaceMode::Automation | AgentConversationWorkspaceMode::PersonaBuilder
    ) {
        return false;
    }
    agent_mode_requires_workspace(mode)
        || (mode == AgentConversationWorkspaceMode::Chat
            && (source_pull_request.is_some() || has_plan_reference))
}

pub(crate) fn review_pr_monitor_for_workspace(
    workspace: &AgentConversationWorkspace,
) -> Result<Option<AgentWorkspacePrReviewMonitor>, String> {
    if workspace.mode != AgentConversationWorkspaceMode::ReviewPr {
        return Ok(None);
    }

    let pr_number = workspace
        .source_pull_request
        .as_ref()
        .map(|pull_request| pull_request.number)
        .or(workspace.publication_pr_number)
        .ok_or_else(|| "Review PR workspace requires a linked pull request".to_string())?;
    let head_sha = workspace
        .source_pull_request
        .as_ref()
        .and_then(|pull_request| pull_request.head_ref_oid.clone());
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        workspace.conversation_id.clone(),
        workspace.project_id.clone(),
        pr_number,
        head_sha,
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    Ok(Some(monitor))
}

pub(crate) async fn ensure_review_pr_monitor_for_workspace(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    workspace: Option<&AgentConversationWorkspace>,
) -> Result<Option<AgentWorkspacePrReviewMonitor>, String> {
    let Some(monitor) = workspace
        .map(review_pr_monitor_for_workspace)
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };

    let monitor = if let Some(existing) = workspace_repo
        .get_pr_review_monitor(&monitor.conversation_id)
        .await
        .map_err(|error| error.to_string())?
    {
        existing
    } else {
        workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .map_err(|error| error.to_string())?
    };
    Ok(Some(monitor))
}

pub(crate) async fn ensure_linked_branch_workspace_available(
    state: &AppState,
    project_id: &ProjectId,
    current_conversation_id: Option<&ChatConversationId>,
    branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    base_ref: Option<&str>,
    source_pull_request: Option<&AgentWorkspaceSourcePullRequest>,
) -> Result<(), String> {
    if branch_mode != Some(AgentConversationWorkspaceBranchMode::Linked) {
        return Ok(());
    }
    let branch_name = source_pull_request
        .map(|pull_request| pull_request.head_ref_name.as_str())
        .or(base_ref)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(branch_name) = branch_name else {
        return Ok(());
    };

    let active_workspaces = state
        .agent_conversation_workspace_repo
        .find_active_by_project_and_branch_name(project_id, branch_name)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(conflict) = active_workspaces
        .into_iter()
        .find(|workspace| current_conversation_id != Some(&workspace.conversation_id))
    {
        return Err(format!(
            "Selected branch '{}' is already linked to active conversation {}; choose isolated branch mode or continue in that conversation",
            branch_name, conflict.conversation_id
        ));
    }

    Ok(())
}

pub(crate) const LINKED_SETUP_FAILURE_MARKER: &str = "[ralphx:linked_setup_failure]";

pub(crate) fn linked_setup_failure_error(message: impl AsRef<str>) -> String {
    let message = message.as_ref().trim();
    if message.contains(LINKED_SETUP_FAILURE_MARKER) {
        return message.to_string();
    }
    if message.is_empty() {
        LINKED_SETUP_FAILURE_MARKER.to_string()
    } else {
        format!("{LINKED_SETUP_FAILURE_MARKER} {message}")
    }
}

pub(crate) async fn archive_empty_seeded_draft_after_setup_failure(
    state: &AppState,
    conversation: &ChatConversation,
) -> Result<bool, String> {
    if conversation.context_type != ChatContextType::Project
        || conversation.message_count != 0
        || conversation.archived_at.is_some()
        || conversation.provider_session_ref().is_some()
    {
        return Ok(false);
    }

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .map_err(|error| error.to_string())?;
    if workspace.is_some() {
        return Ok(false);
    }

    state
        .chat_conversation_repo
        .archive(&conversation.id)
        .await
        .map_err(|error| error.to_string())?;
    Ok(true)
}

pub(crate) async fn archive_supplied_seeded_draft_after_setup_failure(
    state: &AppState,
    project_id: &str,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    let lookup = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| error.to_string());

    archive_seeded_draft_lookup_after_setup_failure(state, project_id, lookup).await
}

pub(crate) async fn archive_seeded_draft_lookup_after_setup_failure(
    state: &AppState,
    project_id: &str,
    conversation: Result<Option<ChatConversation>, String>,
) -> Result<bool, String> {
    let Some(conversation) = conversation? else {
        return Ok(false);
    };
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != project_id
    {
        return Ok(false);
    }

    archive_empty_seeded_draft_after_setup_failure(state, &conversation).await
}

pub(crate) async fn hydrate_linked_branch_source_pull_request(
    state: &AppState,
    project: &Project,
    branch_mode: Option<AgentConversationWorkspaceBranchMode>,
    base_ref: Option<&str>,
    source_pull_request: Option<AgentWorkspaceSourcePullRequest>,
) -> Result<Option<AgentWorkspaceSourcePullRequest>, String> {
    if source_pull_request.is_some()
        || branch_mode != Some(AgentConversationWorkspaceBranchMode::Linked)
    {
        return Ok(source_pull_request);
    }
    let Some(branch_name) = base_ref.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some(github) = state.github_service.as_ref() else {
        return Ok(None);
    };

    let matches = match github
        .search_pull_requests(Path::new(&project.working_directory), Some(branch_name), 20)
        .await
    {
        Ok(matches) => matches,
        Err(error) => {
            tracing::warn!(
                project_id = %project.id,
                branch_name,
                error = %error,
                "Linked branch PR lookup failed; continuing without PR linkage"
            );
            return Ok(None);
        }
    };

    Ok(matches
        .into_iter()
        .find(|pull_request| {
            pull_request.is_open()
                && !pull_request.is_cross_repository
                && pull_request.head_ref_name == branch_name
        })
        .map(|pull_request| AgentWorkspaceSourcePullRequest {
            number: pull_request.number,
            url: Some(pull_request.url),
            title: Some(pull_request.title),
            head_ref_name: pull_request.head_ref_name,
            base_ref_name: Some(pull_request.base_ref_name),
            head_ref_oid: pull_request.head_ref_oid,
        }))
}

pub(crate) async fn ensure_plan_workspace_planning_session_link(
    state: &AppState,
    project: &Project,
    workspace: &mut AgentConversationWorkspace,
) -> Result<bool, String> {
    if workspace.mode != AgentConversationWorkspaceMode::Plan {
        return Ok(false);
    }

    if crate::application::agent_plan_context::linked_workspace_planning_session_is_reusable(
        state, workspace,
    )
    .await?
    {
        return Ok(false);
    }

    let analysis = prepare_ideation_analysis_state_from_agent_workspace(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    let session = IdeationSession::builder()
        .project_id(workspace.project_id.clone())
        .session_flow(IdeationSessionFlow::Planning)
        .source_context_type("agent_conversation")
        .source_context_id(workspace.conversation_id.as_str())
        .spawn_reason("agent_plan_mode")
        .analysis(analysis)
        .build();
    let session = hydrate_agent_conversation_planning_session_title(state, session)
        .await
        .map_err(|error| error.to_string())?;
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .map_err(|error| error.to_string())?;

    workspace.linked_ideation_session_id = Some(session.id);
    workspace.linked_plan_branch_id = None;
    workspace.updated_at = chrono::Utc::now();
    Ok(true)
}

pub(crate) async fn agent_workspace_pr_automation_defaults_for_project(
    state: &AppState,
    project_id: &ProjectId,
) -> Result<AgentConversationWorkspacePrAutomationDefaults, String> {
    let settings = state
        .execution_settings_repo
        .get_settings(Some(project_id))
        .await
        .map_err(|error| error.to_string())?;
    Ok(AgentConversationWorkspacePrAutomationDefaults::from(
        &settings,
    ))
}
