use async_trait::async_trait;

use crate::application::attention_service::AttentionService;
use crate::application::interactive_notification_producer::InteractiveNotificationProducer;
use crate::application::plan_approval_notification_service::{
    has_deferred_plan_approval, has_deferred_plan_approval_in_db,
    reconcile_deferred_plan_approvals_on_startup, reconcile_plan_approval_on_publish,
    release_deferred_plan_approval, release_deferred_plan_approval_for_conversation,
    release_deferred_plan_approval_for_run, PlanApprovalNotificationDisposition,
    PlanApprovalPublishAuthority,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentRun, AgentRunActionKind,
    AgentRunId, ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSession,
    IdeationSessionFlow, NotificationTarget, Project,
};
use crate::domain::ideation::{IdeationSettings, TasksFeatureState};
use crate::domain::repositories::{IdeationSettingsRepository, PlanApprovalActor};
use crate::infrastructure::memory::MemoryPlanArtifactApprovalRepository;

struct FailingIdeationSettingsRepository;

#[async_trait]
impl IdeationSettingsRepository for FailingIdeationSettingsRepository {
    async fn get_settings(&self) -> Result<IdeationSettings, Box<dyn std::error::Error>> {
        Err("injected settings read failure".into())
    }

    async fn update_settings(
        &self,
        _settings: &IdeationSettings,
    ) -> Result<IdeationSettings, Box<dyn std::error::Error>> {
        Err("injected settings write failure".into())
    }

    async fn compare_and_set_tasks_feature_state(
        &self,
        _expected: TasksFeatureState,
        _next: TasksFeatureState,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Err("injected settings write failure".into())
    }
}

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
    session.plan_blueprint_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-current-blueprint".to_string(),
    ));
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

async fn seed_deferred_marker(state: &AppState, session: &IdeationSession, artifact_id: &str) {
    let session_id = session.id.as_str().to_string();
    let artifact_id = artifact_id.to_string();
    let plan_target_id = session
        .plan_artifact_bundle()
        .expect("deferred approval tests require a complete plan bundle")
        .action_target_id();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO deferred_plan_approval_notifications
                    (session_id, artifact_id, plan_target_id, created_at)
                 VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![session_id, artifact_id, plan_target_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}

fn plan_target_id(session: &IdeationSession) -> String {
    session
        .plan_artifact_bundle()
        .expect("plan approval tests require a complete plan bundle")
        .action_target_id()
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
    let expected_dedupe = format!("plan:{}:{}", session.id, plan_target_id(&session));
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

#[tokio::test]
async fn blueprint_only_revision_records_a_new_exact_bundle_notification() {
    let state = AppState::new_test();
    let (mut session, _) = planning_session_with_workspace(&state).await;
    session.plan_contract_version = 2;
    session.plan_blueprint_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "blueprint-1",
    ));
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        None,
    )
    .await;
    state
        .notification_service()
        .record_result(InteractiveNotificationProducer::plan_approval(
            session.project_id.to_string(),
            session.id.as_str(),
            "plan-current",
            session.title.as_deref(),
            NotificationTarget::none(),
        ))
        .await
        .unwrap();

    let prior_session = session.clone();
    session.plan_blueprint_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "blueprint-2",
    ));
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "blueprint-2",
        std::slice::from_ref(&prior_session),
        None,
    )
    .await;

    let notifications = state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications;
    assert_eq!(
        notifications.len(),
        3,
        "a Blueprint-only revision must not dedupe against the prior pair or legacy key"
    );
    let current_target = session
        .plan_artifact_bundle()
        .expect("v2 session should have a complete bundle")
        .action_target_id();
    let expected_current_key = format!("plan:{}:{current_target}", session.id);
    let current = notifications
        .iter()
        .find(|notification| notification.dedupe_key.as_deref() == Some(&expected_current_key))
        .expect("the revised exact bundle should create its own notification");
    assert!(
        current.read_at.is_none(),
        "the revised exact bundle notification must remain actionable"
    );

    let prior_target = prior_session
        .plan_artifact_bundle()
        .expect("prior v2 session should have a complete bundle")
        .action_target_id();
    let expected_prior_key = format!("plan:{}:{prior_target}", session.id);
    let prior = notifications
        .iter()
        .find(|notification| notification.dedupe_key.as_deref() == Some(&expected_prior_key))
        .expect("the prior exact bundle notification should be retained");
    assert!(
        prior.read_at.is_some(),
        "the prior exact bundle notification should be settled"
    );
    let expected_legacy_key = format!("plan:{}:plan-current", session.id);
    let legacy = notifications
        .iter()
        .find(|notification| notification.dedupe_key.as_deref() == Some(&expected_legacy_key))
        .expect("the legacy Overview-keyed notification should be retained");
    assert!(
        legacy.read_at.is_some(),
        "the legacy Overview-keyed notification should be settled"
    );
}

#[tokio::test]
async fn stale_and_missing_deferred_releases_are_skipped_without_attention() {
    let state = AppState::new_test();
    let (session, _) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &session, "plan-current").await;
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-revised".to_string()))
        .await
        .unwrap();

    assert_eq!(
        release_deferred_plan_approval(&state, &session.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );
    assert!(
        !has_deferred_plan_approval_in_db(&state.db, &session.id, "plan-current")
            .await
            .unwrap()
    );
    assert_eq!(
        release_deferred_plan_approval(&state, &session.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );
    assert!(state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications
        .is_empty());
}

#[tokio::test]
async fn non_planning_publish_clears_deferred_attention_without_recording() {
    let state = AppState::new_test();
    let (mut session, _) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &session, "plan-current").await;
    session.session_flow = IdeationSessionFlow::Ideation;
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

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
            .unwrap()
    );
    assert!(state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications
        .is_empty());
}

#[tokio::test]
async fn conversation_release_waits_for_verifier_then_terminal_run_records_attention() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &session, "plan-current").await;
    let mut verifier = AgentRun::new(conversation.id);
    verifier.action_kind = Some(AgentRunActionKind::VerifyPlan);
    verifier.action_context_id = Some(session.id.as_str().to_string());
    verifier.action_target_id = Some(plan_target_id(&session));
    let verifier = state.agent_run_repo.create(verifier).await.unwrap();

    assert_eq!(
        release_deferred_plan_approval_for_conversation(&state, &conversation.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Deferred
    );
    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );

    state
        .agent_run_repo
        .fail(&verifier.id, "verification failed")
        .await
        .unwrap();
    assert_eq!(
        release_deferred_plan_approval_for_run(&state, &verifier.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Recorded
    );
    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );

    seed_deferred_marker(&state, &session, "plan-current").await;
    assert_eq!(
        release_deferred_plan_approval_for_conversation(&state, &conversation.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Recorded
    );
}

#[tokio::test]
async fn conversation_release_skips_edit_workspace_before_verification_settlement() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &session, "plan-current").await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let mut verifier = AgentRun::new(conversation.id);
    verifier.action_kind = Some(AgentRunActionKind::VerifyPlan);
    verifier.action_context_id = Some(session.id.as_str().to_string());
    verifier.action_target_id = Some(plan_target_id(&session));
    state.agent_run_repo.create(verifier).await.unwrap();

    assert_eq!(
        release_deferred_plan_approval_for_conversation(&state, &conversation.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );
    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn run_release_rejects_missing_untyped_and_mismatched_authority() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &session, "plan-current").await;

    assert_eq!(
        release_deferred_plan_approval_for_run(&state, &AgentRunId::new())
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );

    let plain = state
        .agent_run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .unwrap();
    state.agent_run_repo.complete(&plain.id).await.unwrap();
    assert_eq!(
        release_deferred_plan_approval_for_run(&state, &plain.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );

    let mut verifier = AgentRun::new(conversation.id);
    verifier.action_kind = Some(AgentRunActionKind::VerifyPlan);
    let verifier = state.agent_run_repo.create(verifier).await.unwrap();
    state.agent_run_repo.complete(&verifier.id).await.unwrap();
    assert_eq!(
        release_deferred_plan_approval_for_run(&state, &verifier.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );

    let mut missing_session = AgentRun::new(conversation.id);
    missing_session.action_kind = Some(AgentRunActionKind::VerifyPlan);
    missing_session.action_context_id = Some("missing-session".to_string());
    missing_session.action_target_id = Some(plan_target_id(&session));
    let missing_session = state.agent_run_repo.create(missing_session).await.unwrap();
    state
        .agent_run_repo
        .complete(&missing_session.id)
        .await
        .unwrap();
    assert_eq!(
        release_deferred_plan_approval_for_run(&state, &missing_session.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );

    let mut stale_target = AgentRun::new(conversation.id);
    stale_target.action_kind = Some(AgentRunActionKind::VerifyPlan);
    stale_target.action_context_id = Some(session.id.as_str().to_string());
    stale_target.action_target_id = Some("plan-stale".to_string());
    let stale_target = state.agent_run_repo.create(stale_target).await.unwrap();
    state
        .agent_run_repo
        .complete(&stale_target.id)
        .await
        .unwrap();
    assert_eq!(
        release_deferred_plan_approval_for_run(&state, &stale_target.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );
    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn startup_reconciliation_releases_terminal_markers_and_preserves_active_or_unknown_ones() {
    let state = AppState::new_test();
    let (terminal_session, _) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &terminal_session, "plan-current").await;

    let (active_session, active_conversation) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &active_session, "plan-current").await;
    let mut active_verifier = AgentRun::new(active_conversation.id);
    active_verifier.action_kind = Some(AgentRunActionKind::VerifyPlan);
    active_verifier.action_context_id = Some(active_session.id.as_str().to_string());
    active_verifier.action_target_id = Some(plan_target_id(&active_session));
    state.agent_run_repo.create(active_verifier).await.unwrap();

    state
        .db
        .run(|conn| {
            conn.execute(
                "INSERT INTO deferred_plan_approval_notifications
                    (session_id, artifact_id, created_at)
                 VALUES ('missing-session', 'plan-current', datetime('now'))",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    reconcile_deferred_plan_approvals_on_startup(&state)
        .await
        .unwrap();

    assert!(
        !has_deferred_plan_approval(&state, &terminal_session.id, "plan-current")
            .await
            .unwrap()
    );
    assert!(
        has_deferred_plan_approval(&state, &active_session.id, "plan-current")
            .await
            .unwrap()
    );
    assert!(has_deferred_plan_approval_in_db(
        &state.db,
        &crate::domain::entities::IdeationSessionId::from_string("missing-session"),
        "plan-current"
    )
    .await
    .unwrap());
}

#[tokio::test]
async fn publish_authority_failures_fall_back_to_immediate_attention() {
    let mut state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let authority = live_publish_authority(&state, &conversation).await;
    state.ideation_settings_repo = std::sync::Arc::new(FailingIdeationSettingsRepository);

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
}

#[tokio::test]
async fn disabled_setting_and_missing_run_authority_cannot_defer_attention() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let mut settings = state.ideation_settings_repo.get_settings().await.unwrap();
    settings.auto_verify_draft_plans = false;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    let missing_run_authority =
        PlanApprovalPublishAuthority::new(AgentRunId::new(), conversation.id);

    reconcile_plan_approval_on_publish(
        &state,
        None,
        "plan-current",
        std::slice::from_ref(&session),
        Some(&missing_run_authority),
    )
    .await;
    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );

    settings.auto_verify_draft_plans = true;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, Some("plan-next".to_string()))
        .await
        .unwrap();
    let mut next_session = session.clone();
    next_session.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-next",
    ));
    reconcile_plan_approval_on_publish(
        &state,
        Some("plan-current"),
        "plan-next",
        std::slice::from_ref(&next_session),
        Some(&missing_run_authority),
    )
    .await;
    assert!(
        !has_deferred_plan_approval(&state, &session.id, "plan-next")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn exact_typed_verifier_authority_can_defer_its_own_artifact() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let mut verifier = AgentRun::new(conversation.id);
    verifier.action_kind = Some(AgentRunActionKind::VerifyPlan);
    verifier.action_context_id = Some(session.id.as_str().to_string());
    verifier.action_target_id = Some(plan_target_id(&session));
    let verifier = state.agent_run_repo.create(verifier).await.unwrap();
    let authority = PlanApprovalPublishAuthority::new(verifier.id, conversation.id);

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
}

#[tokio::test]
async fn exact_delegation_wake_authority_can_defer_plan_approval() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let mut resumed = AgentRun::new(conversation.id);
    resumed.action_kind = Some(AgentRunActionKind::DelegationParkWake);
    resumed.action_context_id = Some(conversation.id.as_str());
    resumed.action_target_id = Some(crate::domain::entities::DelegationParkId::new().as_str());
    let resumed = state.agent_run_repo.create(resumed).await.unwrap();
    let authority = PlanApprovalPublishAuthority::new(resumed.id, conversation.id);

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
}

#[tokio::test]
async fn foreign_delegation_wake_context_cannot_defer_plan_approval() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let mut resumed = AgentRun::new(conversation.id);
    resumed.action_kind = Some(AgentRunActionKind::DelegationParkWake);
    resumed.action_context_id = Some(ChatConversationId::new().as_str());
    resumed.action_target_id = Some(crate::domain::entities::DelegationParkId::new().as_str());
    let resumed = state.agent_run_repo.create(resumed).await.unwrap();
    let authority = PlanApprovalPublishAuthority::new(resumed.id, conversation.id);

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
}

#[tokio::test]
async fn coverage_regression_pr_autofix_authority_cannot_defer_plan_approval() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    let mut fixer = AgentRun::new(conversation.id);
    fixer.action_kind = Some(AgentRunActionKind::PrAutofix);
    fixer.action_context_id = Some("851".to_string());
    fixer.action_target_id = Some("failing-check".to_string());
    let fixer = state.agent_run_repo.create(fixer).await.unwrap();
    let authority = PlanApprovalPublishAuthority::new(fixer.id, conversation.id);

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
}

#[tokio::test]
async fn coverage_regression_conversation_release_skips_when_no_marker_exists() {
    let state = AppState::new_test();
    let (_, conversation) = planning_session_with_workspace(&state).await;

    let disposition = release_deferred_plan_approval_for_conversation(&state, &conversation.id)
        .await
        .unwrap();

    assert_eq!(disposition, PlanApprovalNotificationDisposition::Skipped);
    assert!(state
        .notification_repo
        .list(None, None, 20)
        .await
        .unwrap()
        .notifications
        .is_empty());
}

#[tokio::test]
async fn release_skips_non_planning_and_already_approved_sessions() {
    let approval_repo = std::sync::Arc::new(MemoryPlanArtifactApprovalRepository::new());
    let mut state = AppState::new_test();
    state.plan_approval_repo = approval_repo.clone();
    let (session, _) = planning_session_with_workspace(&state).await;

    let mut non_planning = IdeationSession::new(session.project_id.clone());
    non_planning.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-non-planning",
    ));
    non_planning.plan_blueprint_artifact_id = Some(
        crate::domain::entities::ArtifactId::from_string("plan-non-planning-blueprint"),
    );
    let non_planning = state
        .ideation_session_repo
        .create(non_planning)
        .await
        .unwrap();
    seed_deferred_marker(&state, &non_planning, "plan-non-planning").await;
    assert_eq!(
        release_deferred_plan_approval(&state, &non_planning.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );

    approval_repo.approve_bundle(
        session.id.clone(),
        crate::domain::entities::ArtifactId::from_string("plan-current"),
        crate::domain::entities::ArtifactId::from_string("plan-current-blueprint"),
        1,
        PlanApprovalActor::User,
    );
    seed_deferred_marker(&state, &session, "plan-current").await;
    assert_eq!(
        release_deferred_plan_approval(&state, &session.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );
    assert!(
        !has_deferred_plan_approval(&state, &session.id, &plan_target_id(&session))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn startup_reconciliation_clears_orphaned_marker_without_attention() {
    let state = AppState::new_test();
    let (_seed_session, _) = planning_session_with_workspace(&state).await;
    let mut session = IdeationSession::new(crate::domain::entities::ProjectId::new());
    session.session_flow = IdeationSessionFlow::Planning;
    session.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-orphaned",
    ));
    session.plan_blueprint_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
        "plan-orphaned-blueprint",
    ));
    let session = state.ideation_session_repo.create(session).await.unwrap();
    seed_deferred_marker(&state, &session, "plan-orphaned").await;

    reconcile_deferred_plan_approvals_on_startup(&state)
        .await
        .unwrap();
    assert!(
        !has_deferred_plan_approval(&state, &session.id, &plan_target_id(&session))
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
}

#[tokio::test]
async fn conversation_release_without_linked_session_leaves_attention_deferred() {
    let state = AppState::new_test();
    let (session, conversation) = planning_session_with_workspace(&state).await;
    seed_deferred_marker(&state, &session, "plan-current").await;
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    workspace.linked_ideation_session_id = None;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    assert_eq!(
        release_deferred_plan_approval_for_conversation(&state, &conversation.id)
            .await
            .unwrap(),
        PlanApprovalNotificationDisposition::Skipped
    );
    assert!(
        has_deferred_plan_approval(&state, &session.id, "plan-current")
            .await
            .unwrap()
    );
}
