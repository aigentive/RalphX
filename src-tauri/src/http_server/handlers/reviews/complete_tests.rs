use super::*;
use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceFollowupProvenance,
    ChatConversation, IdeationAnalysisBaseRefKind, IdeationSessionId, ProjectId, Review,
    ReviewerType, ScopeDriftStatus, Task, TaskContext,
};
use crate::domain::review::{build_unrelated_drift_followup_draft, ReviewSettings};
use crate::http_server::types::HttpServerState;
use std::sync::Arc;

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

fn task_context_for(task: Task) -> TaskContext {
    TaskContext {
        task,
        source_proposal: None,
        plan_artifact: None,
        related_artifacts: Vec::new(),
        steps: Vec::new(),
        step_progress: None,
        context_hints: Vec::new(),
        blocked_by: Vec::new(),
        blocks: Vec::new(),
        tier: None,
        task_branch: None,
        worktree_path: None,
        validation_cache: None,
        actual_changed_files: vec![
            "src-tauri/src/http_server/handlers/reviews/complete.rs".to_string(),
            "config/ralphx.yaml".to_string(),
        ],
        scope_drift_status: ScopeDriftStatus::ScopeExpansion,
        out_of_scope_files: vec!["config/ralphx.yaml".to_string()],
        out_of_scope_blocker_fingerprint: None,
        followup_sessions: Vec::new(),
    }
}

fn test_workspace(
    conversation: &ChatConversation,
    project_id: &ProjectId,
    linked_session_id: Option<IdeationSessionId>,
) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("abc123".to_string()),
        format!("agent/{}", conversation.id.as_str()),
        "/tmp/ralphx-review-complete-test".to_string(),
    );
    workspace.linked_ideation_session_id = linked_session_id;
    workspace
}

async fn seed_origin_workspace(
    app_state: &AppState,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
) -> ChatConversation {
    let origin = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(origin.clone())
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(test_workspace(
            &origin,
            project_id,
            Some(session_id.clone()),
        ))
        .await
        .unwrap();
    origin
}

fn review_settings(auto_create_followup_agent_conversation: bool) -> ReviewSettings {
    ReviewSettings {
        max_revision_cycles: 1,
        auto_create_followup_agent_conversation,
        ..ReviewSettings::default()
    }
}

#[tokio::test]
async fn unrelated_drift_issue_is_recorded_when_auto_followup_is_disabled() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let mut task = Task::new(project_id.clone(), "Handle drift".to_string());
    task.ideation_session_id = Some(session_id.clone());
    let task_context = task_context_for(task.clone());
    let review = Review::new(project_id.clone(), task.id.clone(), ReviewerType::Ai);
    let origin = seed_origin_workspace(&app_state, &project_id, &session_id).await;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let result = maybe_register_unrelated_drift_issue(
        &state,
        &task,
        &review,
        &task_context,
        ReviewToolOutcome::Escalate,
        Some(ScopeDriftClassification::UnrelatedDrift),
        1,
        &review_settings(false),
        Some("summary"),
        Some("feedback"),
        Some("escalation"),
    )
    .await;

    assert!(result.is_none());
    let issues = app_state
        .agent_conversation_issue_repo
        .list_by_conversation(&origin.id, false)
        .await
        .unwrap();
    assert_eq!(issues.len(), 1);
    let issue = &issues[0];
    assert_eq!(issue.conversation_id, origin.id);
    assert_eq!(issue.source_task_id.as_deref(), Some(task.id.as_str()));
    assert_eq!(issue.source_context_type.as_deref(), Some("review"));
    assert_eq!(issue.source_context_id.as_deref(), Some(review.id.as_str()));
    assert_eq!(
        issue.source_agent_name.as_deref(),
        Some("ralphx-execution-reviewer")
    );
    assert_eq!(issue.issue_kind, "plan_drift");
    assert_eq!(issue.blocking_scope, "followup_only");
    assert!(issue.auto_followup_eligible);
    assert!(issue.linked_followup_conversation_id.is_none());
    assert!(issue.blocker_fingerprint.is_some());
}

#[tokio::test]
async fn unrelated_drift_issue_reuses_and_links_existing_followup_when_auto_enabled() {
    let app_state = Arc::new(AppState::new_test());
    let state = test_http_state(Arc::clone(&app_state));
    let project_id = ProjectId::new();
    let session_id = IdeationSessionId::new();
    let mut task = Task::new(project_id.clone(), "Handle drift".to_string());
    task.ideation_session_id = Some(session_id.clone());
    let task_context = task_context_for(task.clone());
    let review = Review::new(project_id.clone(), task.id.clone(), ReviewerType::Ai);
    let origin = seed_origin_workspace(&app_state, &project_id, &session_id).await;
    let task = app_state.task_repo.create(task).await.unwrap();
    let draft = build_unrelated_drift_followup_draft(
        &task,
        &task_context,
        Some("summary"),
        Some("feedback"),
        Some("escalation"),
        1,
        &review_settings(true),
    );
    let followup = ChatConversation::new_project(project_id.clone());
    app_state
        .chat_conversation_repo
        .create(followup.clone())
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(test_workspace(&followup, &project_id, None))
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .save_followup_provenance(
            &followup.id,
            AgentWorkspaceFollowupProvenance {
                origin_conversation_id: origin.id.clone(),
                source_task_id: Some(task.id.as_str().to_string()),
                source_context_type: Some("review".to_string()),
                source_context_id: Some(review.id.as_str().to_string()),
                source_agent_name: Some("ralphx-execution-reviewer".to_string()),
                spawn_reason: Some("out_of_scope_failure".to_string()),
                blocker_fingerprint: draft.blocker_fingerprint.clone(),
            },
        )
        .await
        .unwrap();

    let result = maybe_register_unrelated_drift_issue(
        &state,
        &task,
        &review,
        &task_context,
        ReviewToolOutcome::Escalate,
        Some(ScopeDriftClassification::UnrelatedDrift),
        1,
        &review_settings(true),
        Some("summary"),
        Some("feedback"),
        Some("escalation"),
    )
    .await;

    assert_eq!(result.as_deref(), Some(followup.id.as_str().as_str()));
    let issues = app_state
        .agent_conversation_issue_repo
        .list_by_conversation(&origin.id, false)
        .await
        .unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(
        issues[0].linked_followup_conversation_id.as_ref(),
        Some(&followup.id)
    );
}
