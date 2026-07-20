use crate::application::attention_service::AttentionService;
use crate::application::plan_approval_notification_service::{
    has_deferred_plan_approval, reconcile_plan_approval_on_publish, release_deferred_plan_approval,
    PlanApprovalNotificationDisposition,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSession, IdeationSessionFlow, Project,
};

async fn planning_session_with_workspace(state: &AppState) -> (IdeationSession, ChatConversation) {
    state
        .db
        .run(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS deferred_plan_approval_notifications (
                    session_id TEXT PRIMARY KEY NOT NULL,
                    artifact_id TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );",
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let project = state
        .project_repo
        .create(Project::new(
            "Deferred plan attention".to_string(),
            "/tmp/deferred-plan-attention".to_string(),
        ))
        .await
        .unwrap();
    let mut session = IdeationSession::new(project.id.clone());
    session.session_flow = IdeationSessionFlow::Planning;
    let session = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-current".to_string()))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        project.id,
        AgentConversationWorkspaceMode::Plan,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        Some("main".to_string()),
        Some("base".to_string()),
        "plan-workspace".to_string(),
        "/tmp/plan-workspace".to_string(),
    );
    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    (session, conversation)
}

#[tokio::test]
async fn auto_verification_defers_all_plan_attention_until_terminal_release() {
    let state = AppState::new_test();
    let (session, _) = planning_session_with_workspace(&state).await;

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
    )
    .await;

    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
    assert!(state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications
        .is_empty());
    assert!(AttentionService::from_app_state(&state)
        .list_attention_items(None)
        .await
        .unwrap()
        .iter()
        .all(|item| item.category != crate::domain::entities::NotificationCategory::PlanApproval));

    assert_eq!(
        release_deferred_plan_approval(&state, &session.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Recorded
    );
    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
    let notifications = state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications;
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "Plan approval needed");
    let expected_dedupe = format!("plan:{}:plan-current", session.id);
    assert_eq!(
        notifications[0].dedupe_key.as_deref(),
        Some(expected_dedupe.as_str())
    );
}

#[tokio::test]
async fn plan_revision_replaces_deferred_identity_and_settles_prior_notification() {
    let state = AppState::new_test();
    let (mut session, _) = planning_session_with_workspace(&state).await;
    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
    )
    .await;
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-revised".to_string()))
        .await
        .unwrap();
    session.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-revised".to_string(),
    ));

    reconcile_plan_approval_on_publish(
        &state,
        Some("plan-current"),
        "plan-revised",
        std::slice::from_ref(&session),
    )
    .await;

    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-revised")
            .await
            .unwrap()
    );
}
