use crate::application::attention_service::AttentionService;
use crate::application::plan_approval_notification_service::{
    has_deferred_plan_approval, reconcile_plan_approval_on_publish, release_deferred_plan_approval,
    PlanApprovalNotificationDisposition, PlanApprovalPublishAuthority,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, ChatConversation,
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

async fn live_publish_authority(
    state: &AppState,
    conversation: &ChatConversation,
) -> PlanApprovalPublishAuthority {
    let run = state
        .agent_run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .unwrap();
    PlanApprovalPublishAuthority::new(run.id, conversation.id)
}

#[tokio::test]
async fn auto_verification_defers_all_plan_attention_until_terminal_release() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let authority = live_publish_authority(&state, &conversation).await;

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        Some(&authority),
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
async fn publish_without_live_exact_authority_records_attention_immediately() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        None,
    )
    .await;

    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap(),
        "a user or external publish without transport-owned run identity cannot defer attention"
    );
    assert_eq!(
        state
            .notification_repo
            .list(None, None, 20)
            .await
            .unwrap()
            .notifications
            .len(),
        1
    );

    let terminal_authority = live_publish_authority(&state, &conversation).await;
    state
        .agent_run_repo
        .complete(&terminal_authority.run_id)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-terminal".to_string()))
        .await
        .unwrap();
    let mut terminal_session = session.clone();
    terminal_session.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-terminal".to_string(),
    ));

    reconcile_plan_approval_on_publish(
        &state,
        Some("plan-current"),
        "plan-terminal",
        std::slice::from_ref(&terminal_session),
        Some(&terminal_authority),
    )
    .await;

    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-terminal")
            .await
            .unwrap(),
        "a terminal run cannot remain the owner of deferred attention"
    );
}

#[tokio::test]
async fn publish_authority_from_another_conversation_cannot_defer_attention() {
    let state = AppState::new_test();
    let (session, _) = planning_session_with_workspace(&state).await;
    let unrelated = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(session.project_id.clone()))
        .await
        .unwrap();
    let authority = live_publish_authority(&state, &unrelated).await;

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        Some(&authority),
    )
    .await;

    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn plan_revision_replaces_deferred_identity_and_settles_prior_notification() {
    let state = AppState::new_test();
    let (mut session, conversation) = planning_session_with_workspace(&state).await;
    let authority = live_publish_authority(&state, &conversation).await;
    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        Some(&authority),
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
        Some(&authority),
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
