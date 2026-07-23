use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ArtifactId, ChatContextType, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind,
    IdeationSession, IdeationSessionId, PlanBranchId, PlanBranchStatus, Project, ProjectId, TaskId,
};
use crate::infrastructure::memory::MemoryChatConversationRepository;
use chrono::Utc;
use std::sync::Arc;

async fn seed_linked_session(
    state: &AppState,
    session_project_id: ProjectId,
    workspace_project_id: ProjectId,
    workspace_status: AgentConversationWorkspaceStatus,
    create_conversation: bool,
) -> (
    IdeationSession,
    ChatConversation,
    AgentConversationWorkspace,
) {
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(session_project_id))
        .await
        .expect("session should persist");
    let mut conversation = ChatConversation::new_ideation(session.id.clone());
    conversation.title = Some("Plan owner".into());
    let workspace = {
        let mut workspace = AgentConversationWorkspace::new(
            conversation.id,
            workspace_project_id,
            AgentConversationWorkspaceMode::Ideation,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".into(),
            Some("main".into()),
            Some("base-sha".into()),
            "ralphx/ideation-navigation".into(),
            "/tmp/ideation-navigation".into(),
        );
        workspace.status = workspace_status;
        workspace.linked_ideation_session_id = Some(session.id.clone());
        workspace
    };
    if create_conversation {
        state
            .chat_conversation_repo
            .create(conversation.clone())
            .await
            .expect("conversation should persist");
    }
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    (session, conversation, workspace)
}

#[tokio::test]
async fn linked_ideation_session_resolves_exact_active_agent_workspace() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Exact ideation navigation project".into(),
            "/tmp/exact-ideation-navigation-project".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let mut conversation = ChatConversation::new_ideation(session.id.clone());
    conversation.title = Some("Exact plan owner".into());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/exact-ideation-navigation".into(),
        "/tmp/exact-ideation-navigation".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("navigation lookup should succeed")
        .expect("active linked workspace should resolve");

    assert_eq!(target.conversation_id, conversation.id.as_str());
    assert_eq!(target.project_id, project.id.as_str());
    assert_eq!(target.title, "Exact plan owner");
}

#[tokio::test]
async fn linked_ideation_session_returns_none_for_archived_conversation() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Archived conversation project".into(),
            "/tmp/archived-conversation-project".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let mut conversation = ChatConversation::new_ideation(session.id.clone());
    conversation.archived_at = Some(Utc::now());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/archived-conversation".into(),
        "/tmp/archived-conversation".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("archived conversation should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_returns_none_for_conversation_from_another_session() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Mismatched conversation project".into(),
            "/tmp/mismatched-conversation-project".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let other_session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("other session should persist");
    let conversation = ChatConversation::new_ideation(other_session.id);
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/mismatched-conversation".into(),
        "/tmp/mismatched-conversation".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("mismatched conversation should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_resolves_active_same_project_project_conversation() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Project conversation compatibility".into(),
            "/tmp/project-conversation-compatibility".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.title = Some("Project-scoped plan owner".into());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/project-conversation-compatibility".into(),
        "/tmp/project-conversation-compatibility".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("project conversation should be a valid navigation target")
        .expect("active same-project project conversation should resolve");

    assert_eq!(target.conversation_id, conversation.id.as_str());
    assert_eq!(target.project_id, project.id.as_str());
    assert_eq!(target.title, "Project-scoped plan owner");
}

#[tokio::test]
async fn linked_ideation_session_rejects_mismatched_project_conversation_context() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Mismatched project conversation context".into(),
            "/tmp/mismatched-project-conversation-context".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.context_id = "different-project".into();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/mismatched-project-conversation-context".into(),
        "/tmp/mismatched-project-conversation-context".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("mismatched project context should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_rejects_unsupported_conversation_context_type() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Unsupported conversation context type".into(),
            "/tmp/unsupported-conversation-context-type".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id.clone()))
        .await
        .expect("session should persist");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.context_type = ChatContextType::Task;
    conversation.context_id = "task-context".into();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project.id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/unsupported-conversation-context-type".into(),
        "/tmp/unsupported-conversation-context-type".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("unsupported context type should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_returns_none_when_session_is_missing() {
    let state = AppState::new_test();

    let target = resolve_agent_workspace_target_for_ideation_session(
        &state,
        &IdeationSessionId::from_string("missing-session"),
    )
    .await
    .expect("missing session should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_returns_none_when_workspace_is_missing() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Missing workspace project".into(),
            "/tmp/missing-workspace-project".into(),
        ))
        .await
        .expect("project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id))
        .await
        .expect("session should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("missing workspace should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_rejects_inactive_workspace() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Inactive workspace project".into(),
            "/tmp/inactive-workspace-project".into(),
        ))
        .await
        .expect("project should persist");
    let (session, _, _) = seed_linked_session(
        &state,
        project.id.clone(),
        project.id,
        AgentConversationWorkspaceStatus::Archived,
        true,
    )
    .await;

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("inactive workspace should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_rejects_workspace_from_another_project() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Project ownership source".into(),
            "/tmp/project-ownership-source".into(),
        ))
        .await
        .expect("source project should persist");
    let other_project = state
        .project_repo
        .create(Project::new(
            "Project ownership mismatch".into(),
            "/tmp/project-ownership-mismatch".into(),
        ))
        .await
        .expect("other project should persist");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id))
        .await
        .expect("session should persist");
    let conversation = ChatConversation::new_ideation(session.id.clone());
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        other_project.id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".into(),
        Some("main".into()),
        Some("base-sha".into()),
        "ralphx/project-mismatch".into(),
        "/tmp/project-mismatch".into(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("project mismatch should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_rejects_missing_trusted_conversation() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Missing conversation project".into(),
            "/tmp/missing-conversation-project".into(),
        ))
        .await
        .expect("project should persist");
    let (session, _, _) = seed_linked_session(
        &state,
        project.id.clone(),
        project.id,
        AgentConversationWorkspaceStatus::Active,
        false,
    )
    .await;

    let target = resolve_agent_workspace_target_for_ideation_session(&state, &session.id)
        .await
        .expect("missing conversation should be a neutral navigation result");

    assert!(target.is_none());
}

#[tokio::test]
async fn linked_ideation_session_propagates_conversation_repository_error() {
    let mut state = AppState::new_test();
    let chat_conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    state.chat_conversation_repo = chat_conversation_repo.clone();
    let project = state
        .project_repo
        .create(Project::new(
            "Conversation repository error project".into(),
            "/tmp/conversation-repository-error-project".into(),
        ))
        .await
        .expect("project should persist");
    let (session, conversation, _) = seed_linked_session(
        &state,
        project.id.clone(),
        project.id,
        AgentConversationWorkspaceStatus::Active,
        true,
    )
    .await;
    chat_conversation_repo
        .fail_get_by_id(conversation.id.clone())
        .await;

    let result = resolve_agent_workspace_target_for_ideation_session(&state, &session.id).await;

    let error = result.expect_err("conversation repository errors must propagate");
    assert!(error.contains(&format!(
        "injected conversation lookup failure for {}",
        conversation.id
    )));
}

fn task(id: &str) -> Task {
    let mut task = Task::new(
        ProjectId::from_string("project-1".to_string()),
        "Task".to_string(),
    );
    task.id = TaskId::from_string(id.to_string());
    task
}

fn plan_branch(id: &str, session_id: &str) -> PlanBranch {
    PlanBranch {
        id: PlanBranchId::from_string(id),
        plan_artifact_id: ArtifactId::from_string("artifact-1"),
        session_id: IdeationSessionId::from_string(session_id),
        project_id: ProjectId::from_string("project-1".to_string()),
        branch_name: "feature/plan".to_string(),
        source_branch: "main".to_string(),
        status: PlanBranchStatus::Active,
        execution_plan_id: None,
        merge_task_id: None,
        created_at: Utc::now(),
        merged_at: None,
        pr_number: None,
        pr_url: None,
        pr_status: None,
        pr_polling_active: false,
        pr_eligible: false,
        last_polled_at: None,
        pr_push_status: Default::default(),
        merge_commit_sha: None,
        pr_draft: None,
        base_branch_override: None,
    }
}

fn workspace(
    conversation_id: &str,
    status: AgentConversationWorkspaceStatus,
    plan_branch_id: Option<PlanBranchId>,
    session_id: Option<IdeationSessionId>,
) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string(conversation_id),
        ProjectId::from_string("project-1".to_string()),
        crate::domain::entities::AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/agent-workspace".to_string(),
        "/tmp/ralphx-agent-workspace".to_string(),
    );
    workspace.status = status;
    workspace.linked_plan_branch_id = plan_branch_id;
    workspace.linked_ideation_session_id = session_id;
    workspace
}

#[test]
fn plan_merge_task_resolves_workspace_by_plan_branch_before_session() {
    let mut merge_task = task("merge-task");
    merge_task.category = TaskCategory::PlanMerge;
    let mut branch = plan_branch("plan-branch-1", "session-1");
    branch.merge_task_id = Some(merge_task.id.clone());

    let plan_workspace = workspace(
        "11111111-1111-1111-1111-111111111111",
        AgentConversationWorkspaceStatus::Active,
        Some(branch.id.clone()),
        Some(branch.session_id.clone()),
    );
    let session_workspace = workspace(
        "22222222-2222-2222-2222-222222222222",
        AgentConversationWorkspaceStatus::Active,
        None,
        Some(branch.session_id.clone()),
    );

    let branches = [branch];
    let workspaces = [session_workspace, plan_workspace];
    let resolved = resolve_agent_workspace_for_task(&merge_task, &branches, &workspaces)
        .expect("workspace should resolve");

    assert_eq!(
        resolved.conversation_id.as_str(),
        "11111111-1111-1111-1111-111111111111"
    );
}

#[test]
fn archived_workspace_is_not_a_navigation_target() {
    let mut linked_task = task("task-1");
    linked_task.ideation_session_id = Some(IdeationSessionId::from_string("session-1"));
    let branch = plan_branch("plan-branch-1", "session-1");
    let archived_workspace = workspace(
        "33333333-3333-3333-3333-333333333333",
        AgentConversationWorkspaceStatus::Archived,
        Some(branch.id.clone()),
        Some(branch.session_id.clone()),
    );

    let branches = [branch];
    let workspaces = [archived_workspace];
    let resolved = resolve_agent_workspace_for_task(&linked_task, &branches, &workspaces);

    assert!(resolved.is_none());
}

#[tokio::test]
async fn task_navigation_target_requires_an_active_workspace() {
    let state = AppState::new_test();
    let session_id = IdeationSessionId::from_string("session-1");
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let conversation_id = conversation.id.as_str();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("conversation should persist");
    let mut linked_task = task("task-1");
    linked_task.ideation_session_id = Some(session_id.clone());
    let branches = [];

    let active_target = resolve_agent_workspace_target_for_task(
        &state,
        &linked_task,
        &branches,
        &[workspace(
            conversation_id.as_str(),
            AgentConversationWorkspaceStatus::Active,
            None,
            Some(session_id.clone()),
        )],
    )
    .await
    .expect("active workspace lookup should succeed");
    assert!(active_target.is_some());

    let missing_target = resolve_agent_workspace_target_for_task(
        &state,
        &linked_task,
        &branches,
        &[workspace(
            conversation_id.as_str(),
            AgentConversationWorkspaceStatus::Missing,
            None,
            Some(session_id.clone()),
        )],
    )
    .await
    .expect("missing workspace lookup should succeed");
    assert!(missing_target.is_none());

    let archived_target = resolve_agent_workspace_target_for_task(
        &state,
        &linked_task,
        &branches,
        &[workspace(
            conversation_id.as_str(),
            AgentConversationWorkspaceStatus::Archived,
            None,
            Some(session_id),
        )],
    )
    .await
    .expect("archived workspace lookup should succeed");
    assert!(archived_target.is_none());
}
