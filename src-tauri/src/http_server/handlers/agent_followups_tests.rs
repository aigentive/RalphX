use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceBranchMode,
    AgentConversationWorkspaceMode, AgentWorkspaceFollowupProvenance,
    AgentWorkspaceSourcePullRequest, ChatConversation, IdeationAnalysisBaseRefKind,
    IdeationSessionId, ProjectId, Task, TaskId,
};
use crate::http_server::types::HttpServerState;
use axum::{extract::State, Json};
use std::sync::Arc;

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState::new_test(app_state)
}

fn followup_request(
    origin_conversation_id: Option<String>,
) -> CreateFollowupAgentConversationRequest {
    CreateFollowupAgentConversationRequest {
        origin_conversation_id,
        source_task_id: Some(" task-1 ".to_string()),
        source_context_type: Some(" review ".to_string()),
        source_context_id: Some(" review-1 ".to_string()),
        source_agent_name: Some(" ralphx-execution-reviewer ".to_string()),
        title: "Investigate drift".to_string(),
        description: Some("Use this description".to_string()),
        initial_prompt: Some("Use this prompt".to_string()),
        spawn_reason: Some(" plan_drift ".to_string()),
        blocker_fingerprint: Some(" drift:task-1 ".to_string()),
        provider_harness: None,
        model_override: None,
        logical_effort: None,
    }
}

fn test_workspace(
    conversation: &ChatConversation,
    project_id: &ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        format!("agent/{}", conversation.id.as_str()),
        "/tmp/ralphx-followup-test".to_string(),
    )
}

async fn seed_project_conversation(app_state: &AppState) -> (ProjectId, ChatConversation) {
    let project_id = ProjectId::new();
    let conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    (project_id, conversation)
}

async fn seed_source_task(
    app_state: &AppState,
    project_id: &ProjectId,
    session_id: Option<IdeationSessionId>,
) -> Task {
    let mut task = Task::new(project_id.clone(), "Source task".to_string());
    task.ideation_session_id = session_id;
    app_state.task_repo.create(task).await.unwrap()
}

#[test]
fn helper_normalizes_origin_and_source_pull_request_input() {
    assert_eq!(trim_optional(Some("  value  ")).as_deref(), Some("value"));
    assert_eq!(trim_optional(Some("   ")), None);

    let mut req = followup_request(None);
    req.source_context_type = Some("agent_conversation".to_string());
    req.source_context_id = Some(" conversation-1 ".to_string());
    assert_eq!(
        request_origin_conversation_id(&req).as_deref(),
        Some("conversation-1")
    );

    let input = source_pull_request_input(Some(AgentWorkspaceSourcePullRequest {
        number: 489,
        url: Some("https://github.test/pr/489".to_string()),
        title: Some("Patch coverage".to_string()),
        head_ref_name: "feature".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("abc123".to_string()),
    }))
    .unwrap();
    assert_eq!(input.number, 489);
    assert_eq!(input.head_ref_name, "feature");
    assert_eq!(input.base_ref_name.as_deref(), Some("main"));
}

#[test]
fn followup_prompt_records_origin_and_branch_context() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id);
    let prompt = followup_prompt(
        &followup_request(Some(origin.id.as_str())),
        &origin,
        Some("drift:task-1"),
    );

    assert!(prompt.contains("Create a visible follow-up Agent conversation in Ideation mode."));
    assert!(prompt.contains(&format!("Origin Agent conversation: {}", origin.id)));
    assert!(prompt.contains("Source agent: ralphx-execution-reviewer"));
    assert!(prompt.contains("Source task: task-1"));
    assert!(prompt.contains("Source context type: review"));
    assert!(prompt.contains("Source context ID: review-1"));
    assert!(prompt.contains("Reason: plan_drift"));
    assert!(prompt.contains("Blocker fingerprint: drift:task-1"));
    assert!(prompt.contains("Use this prompt"));
}

#[test]
fn followup_provenance_trims_optional_fields() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id);
    let provenance = followup_provenance(
        &followup_request(Some(origin.id.as_str())),
        &origin,
        Some("drift:task-1".to_string()),
    );

    assert_eq!(provenance.origin_conversation_id, origin.id);
    assert_eq!(provenance.source_task_id.as_deref(), Some("task-1"));
    assert_eq!(provenance.source_context_type.as_deref(), Some("review"));
    assert_eq!(provenance.source_context_id.as_deref(), Some("review-1"));
    assert_eq!(
        provenance.source_agent_name.as_deref(),
        Some("ralphx-execution-reviewer")
    );
    assert_eq!(provenance.spawn_reason.as_deref(), Some("plan_drift"));
    assert_eq!(
        provenance.blocker_fingerprint.as_deref(),
        Some("drift:task-1")
    );
}

#[test]
fn followup_base_branch_mode_preserves_parent_workspace_policy() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id.clone());
    let mut workspace = test_workspace(&origin, &project_id);

    assert_eq!(
        followup_base_selection(Some(&workspace)).branch_mode,
        Some(AgentConversationWorkspaceBranchMode::Isolated)
    );

    workspace.branch_mode = AgentConversationWorkspaceBranchMode::Linked;
    assert_eq!(
        followup_base_selection(Some(&workspace)).branch_mode,
        Some(AgentConversationWorkspaceBranchMode::Linked)
    );
    assert_eq!(followup_base_selection(None).branch_mode, None);
}

#[test]
fn followup_base_selection_uses_pr_head_for_pr_backed_linked_workspace() {
    let project_id = ProjectId::new();
    let origin = ChatConversation::new_project(project_id.clone());
    let mut workspace = AgentConversationWorkspace::new(
        origin.id.clone(),
        project_id,
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("PR #42: Add linked PR".to_string()),
        Some("base123".to_string()),
        "feature/linked-pr".to_string(),
        "/tmp/ralphx-followup-test".to_string(),
    );
    workspace.branch_mode = AgentConversationWorkspaceBranchMode::Linked;
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 42,
        url: Some("https://github.test/pull/42".to_string()),
        title: Some("Add linked PR".to_string()),
        head_ref_name: "feature/linked-pr".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head123".to_string()),
    });

    let selection = followup_base_selection(Some(&workspace));

    assert_eq!(
        selection.kind,
        Some(IdeationAnalysisBaseRefKind::LocalBranch)
    );
    assert_eq!(
        selection.branch_mode,
        Some(AgentConversationWorkspaceBranchMode::Linked)
    );
    assert_eq!(selection.base_ref.as_deref(), Some("feature/linked-pr"));
    assert_eq!(
        selection
            .source_pull_request
            .as_ref()
            .and_then(|pull_request| pull_request.base_ref_name.as_deref()),
        Some("main")
    );
}

#[tokio::test]
async fn create_followup_returns_existing_active_followup_for_same_blocker() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let task = app_state
        .task_repo
        .create(Task::new(project_id.clone(), "Source task".to_string()))
        .await
        .unwrap();
    let source_task_id = task.id.as_str().to_string();
    let followup_conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(followup_conversation.clone())
        .await
        .unwrap();
    let followup_workspace = test_workspace(&followup_conversation, &project_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(followup_workspace)
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup_conversation.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin.id.clone(),
                source_task_id: Some(source_task_id.clone()),
                source_context_type: Some("review".to_string()),
                source_context_id: Some("review-1".to_string()),
                source_agent_name: Some("ralphx-execution-reviewer".to_string()),
                spawn_reason: Some("plan_drift".to_string()),
                blocker_fingerprint: Some("drift:task-1".to_string()),
            },
        )
        .await
        .unwrap();

    let response = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(origin.id.as_str()));
        req.source_task_id = Some(source_task_id);
        req
    })
    .await
    .unwrap();

    assert!(response.reused_existing);
    assert_eq!(response.origin_conversation_id, origin.id.as_str());
    assert_eq!(response.conversation.id, followup_conversation.id.as_str());
    assert!(response.workspace.is_some());
    assert!(response.send_result.is_none());
}

#[tokio::test]
async fn execution_blocked_followup_reuses_canonical_rails_test_database_blocker() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let task = app_state
        .task_repo
        .create(Task::new(project_id.clone(), "Source task".to_string()))
        .await
        .unwrap();
    let source_task_id = task.id.as_str().to_string();
    let followup_conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(followup_conversation.clone())
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(test_workspace(&followup_conversation, &project_id))
        .await
        .unwrap();
    let canonical_fingerprint =
        "v1:setup:project:rails-test-database:schema-unavailable".to_string();
    app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup_conversation.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin.id.clone(),
                source_task_id: Some(source_task_id.clone()),
                source_context_type: Some("task_execution".to_string()),
                source_context_id: Some(source_task_id.clone()),
                source_agent_name: Some("ralphx-execution-worker".to_string()),
                spawn_reason: Some("execution_blocked".to_string()),
                blocker_fingerprint: Some(canonical_fingerprint.clone()),
            },
        )
        .await
        .unwrap();

    let response = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(origin.id.as_str()));
        req.source_task_id = Some(source_task_id);
        req.source_context_type = Some("task_execution".to_string());
        req.title = "Fix RSpec test DB setup for Printspeak worktrees".to_string();
        req.description = Some(
            "Rails test DB/schema setup is unhealthy and blocks focused RSpec validation."
                .to_string(),
        );
        req.initial_prompt = Some(
            "db:schema:load fails with PG::UndefinedTable for failed_messages_seq".to_string(),
        );
        req.spawn_reason = Some("execution_blocked".to_string());
        req.blocker_fingerprint = None;
        req
    })
    .await
    .unwrap();

    assert!(response.reused_existing);
    assert_eq!(response.conversation.id, followup_conversation.id.as_str());
    assert_eq!(
        response.blocker_fingerprint.as_deref(),
        Some(canonical_fingerprint.as_str())
    );
    assert!(response.send_result.is_none());
}

#[tokio::test]
async fn execution_blocked_followup_does_not_reuse_a_different_blocker() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let task = app_state
        .task_repo
        .create(Task::new(project_id.clone(), "Source task".to_string()))
        .await
        .unwrap();
    let source_task_id = task.id.as_str().to_string();
    let followup_conversation = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(followup_conversation.clone())
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(test_workspace(&followup_conversation, &project_id))
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup_conversation.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin.id.clone(),
                source_task_id: Some(source_task_id.clone()),
                source_context_type: Some("task_execution".to_string()),
                source_context_id: Some(source_task_id.clone()),
                source_agent_name: Some("ralphx-execution-worker".to_string()),
                spawn_reason: Some("execution_blocked".to_string()),
                blocker_fingerprint: Some(
                    "v1:setup:project:rails-test-database:schema-unavailable".to_string(),
                ),
            },
        )
        .await
        .unwrap();

    let error = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(origin.id.as_str()));
        req.source_task_id = Some(source_task_id);
        req.title = "Runtime index prerequisite missing".to_string();
        req.description =
            Some("The runtime-index API required by this task is absent.".to_string());
        req.initial_prompt = None;
        req.spawn_reason = Some("execution_blocked".to_string());
        req.blocker_fingerprint = None;
        req
    })
    .await
    .expect_err("a different blocker must continue to new follow-up creation");

    assert_eq!(error.0, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn create_followup_validates_origin_before_spawning_new_branch() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));

    let missing_context = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(None);
        req.source_task_id = None;
        req
    })
    .await
    .expect_err("missing context should be rejected");
    assert_eq!(missing_context.0, StatusCode::BAD_REQUEST);

    let task_conversation = ChatConversation::new_task(TaskId::new());
    app_state
        .chat_conversation_repo
        .create(task_conversation.clone())
        .await
        .unwrap();
    let wrong_context = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(task_conversation.id.as_str()));
        req.source_task_id = None;
        req
    })
    .await
    .expect_err("task conversation should be rejected as origin");
    assert_eq!(wrong_context.0, StatusCode::BAD_REQUEST);

    let (_project_id, origin) = seed_project_conversation(&app_state).await;
    let launch_error = create_followup_agent_conversation(
        State(state),
        Json({
            let mut req = followup_request(Some(origin.id.as_str()));
            req.source_task_id = None;
            req
        }),
    )
    .await
    .expect_err("origin validation must complete before the headless launch fixture fails");
    assert_eq!(launch_error.0, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn create_followup_resolves_origin_from_source_task_workspace() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;
    let session_id = IdeationSessionId::from_string("session-followup");
    let task = seed_source_task(&app_state, &project_id, Some(session_id.clone())).await;
    let mut workspace = test_workspace(&origin, &project_id);
    workspace.linked_ideation_session_id = Some(session_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let launch_error = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(None);
        req.source_task_id = Some(task.id.as_str().to_string());
        req
    })
    .await
    .expect_err("resolved source-task origin must reach the headless launch fixture");

    assert_eq!(launch_error.0, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn create_followup_rejects_invalid_source_task_origins() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let (project_id, origin) = seed_project_conversation(&app_state).await;

    let task_without_session = seed_source_task(&app_state, &project_id, None).await;
    let no_session = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(None);
        req.source_task_id = Some(task_without_session.id.as_str().to_string());
        req
    })
    .await
    .expect_err("source task without ideation session should be rejected");
    assert_eq!(no_session.0, StatusCode::BAD_REQUEST);

    let session_id = IdeationSessionId::from_string("session-without-workspace");
    let task_without_workspace =
        seed_source_task(&app_state, &project_id, Some(session_id.clone())).await;
    let no_workspace = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(None);
        req.source_task_id = Some(task_without_workspace.id.as_str().to_string());
        req
    })
    .await
    .expect_err("source task without linked Agent workspace should be rejected");
    assert_eq!(no_workspace.0, StatusCode::BAD_REQUEST);

    let orphan_session_id = IdeationSessionId::from_string("session-orphan-workspace");
    let orphan_task =
        seed_source_task(&app_state, &project_id, Some(orphan_session_id.clone())).await;
    let orphan_conversation = ChatConversation::new_project(project_id.clone());
    let mut orphan_workspace = test_workspace(&orphan_conversation, &project_id);
    orphan_workspace.linked_ideation_session_id = Some(orphan_session_id);
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(orphan_workspace)
        .await
        .unwrap();
    let missing_conversation = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(None);
        req.source_task_id = Some(orphan_task.id.as_str().to_string());
        req
    })
    .await
    .expect_err("linked workspace without conversation should be rejected");
    assert_eq!(missing_conversation.0, StatusCode::NOT_FOUND);

    let other_project_id = ProjectId::new();
    let other_task = seed_source_task(&app_state, &other_project_id, None).await;
    let project_mismatch = create_followup_agent_conversation_for_request(&state, {
        let mut req = followup_request(Some(origin.id.as_str()));
        req.source_task_id = Some(other_task.id.as_str().to_string());
        req
    })
    .await
    .expect_err("source task from another project should be rejected");
    assert_eq!(project_mismatch.0, StatusCode::BAD_REQUEST);
}
