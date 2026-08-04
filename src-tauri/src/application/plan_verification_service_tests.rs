use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use crate::application::chat_service::MockChatService;
use crate::application::chat_service::SendQueuePolicy;
use crate::application::plan_verification_service::{
    admit_automatic_plan_verification, ensure_plan_verification_for_acceptance,
    get_plan_verification_status, request_plan_verification, source_allows_verified_retry,
    AutomaticPlanVerificationDisposition, PlanVerificationCompletionAdapter,
    PlanVerificationRequestOutcome, PlanVerificationRequestSource, PlanVerificationStatusKind,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, AgentRunActionKind, AgentRunId, ArtifactId, ChatConversation,
    IdeationAnalysisBaseRefKind, IdeationSession, ProjectId,
};
use crate::domain::services::EffectiveGatePolicy;
use crate::domain::services::{QueueKey, QueuedMessage};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::MemoryQueuedMessageRepository;

struct FailSecondQueueList {
    calls: AtomicUsize,
}

#[async_trait]
impl crate::domain::repositories::QueuedMessageRepository for FailSecondQueueList {
    async fn enqueue_back(&self, _key: &QueueKey, _message: &QueuedMessage) -> AppResult<()> {
        Ok(())
    }

    async fn enqueue_front(&self, _key: &QueueKey, _message: &QueuedMessage) -> AppResult<()> {
        Ok(())
    }

    async fn list(&self, _key: &QueueKey) -> AppResult<Vec<QueuedMessage>> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(Vec::new())
        } else {
            Err(AppError::Infrastructure(
                "injected queue read failure".to_string(),
            ))
        }
    }

    async fn list_keys(&self) -> AppResult<Vec<QueueKey>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _key: &QueueKey, _message_id: &str) -> AppResult<bool> {
        Ok(false)
    }

    async fn delete_by_id(&self, _message_id: &str) -> AppResult<bool> {
        Ok(false)
    }

    async fn clear(&self, _key: &QueueKey) -> AppResult<()> {
        Ok(())
    }

    async fn pop_front(&self, _key: &QueueKey) -> AppResult<Option<QueuedMessage>> {
        Ok(None)
    }

    async fn remove_stale(
        &self,
        _key: &QueueKey,
        _threshold_secs: u64,
    ) -> AppResult<Vec<QueuedMessage>> {
        Ok(Vec::new())
    }
}

fn mock_chat(state: &AppState) -> MockChatService {
    MockChatService::with_agent_run_repo(std::sync::Arc::clone(&state.agent_run_repo))
}

fn policy(auto_verify_plans: bool, require_verification_for_accept: bool) -> EffectiveGatePolicy {
    EffectiveGatePolicy {
        auto_verify_plans,
        require_verification_for_accept,
        require_accept_for_finalize: false,
    }
}

#[test]
fn only_manual_requests_may_retry_exact_verified_artifacts() {
    assert!(source_allows_verified_retry(
        PlanVerificationRequestSource::Manual
    ));
    assert!(!source_allows_verified_retry(
        PlanVerificationRequestSource::Automatic
    ));
    assert!(!source_allows_verified_retry(
        PlanVerificationRequestSource::External
    ));
}

async fn session_with_plan(state: &AppState) -> IdeationSession {
    let mut session = IdeationSession::new(ProjectId::new());
    session.plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    session.plan_blueprint_artifact_id = Some(ArtifactId::from_string("plan-current-blueprint"));
    session.plan_contract_version = 2;

    state
        .ideation_session_repo
        .create(session)
        .await
        .expect("session should be created")
}

fn plan_bundle_action_target(session: &IdeationSession) -> String {
    session
        .plan_artifact_bundle()
        .expect("v2 plan fixture should include both artifacts")
        .action_target_id()
}

async fn completed_plan_workspace_run(
    state: &AppState,
    action_kind: Option<AgentRunActionKind>,
) -> (IdeationSession, ChatConversation, AgentRun) {
    let project_id = ProjectId::new();
    let mut session = IdeationSession::new(project_id.clone());
    session.plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    session.plan_blueprint_artifact_id = Some(ArtifactId::from_string("plan-current-blueprint"));
    session.plan_contract_version = 2;
    let session = state.ideation_session_repo.create(session).await.unwrap();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id,
        project_id,
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
    let mut run = AgentRun::new(conversation.id);
    run.action_kind = action_kind;
    let run = state.agent_run_repo.create(run).await.unwrap();
    assert!(state
        .agent_run_repo
        .complete_if_running(&run.id)
        .await
        .unwrap());
    let run = state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    let session = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    (session, conversation, run)
}

#[tokio::test]
async fn acceptance_queues_required_automatic_verification_and_remains_blocked() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = mock_chat(&state);

    let error =
        ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(true, true))
            .await
            .expect_err("acceptance must wait for the queued verification turn");

    assert!(
        error.to_string().contains("queued"),
        "the caller should receive an actionable retry-after-verification error: {error}"
    );
    assert_eq!(chat.call_count(), 1, "exactly one turn should be queued");
    let options = chat.get_sent_options().await;
    let metadata = options[0]
        .metadata
        .as_deref()
        .expect("verification action metadata should be present");
    assert!(metadata.contains("\"ralphx_action_kind\":\"verify_plan\""));
    assert!(metadata.contains(session.id.as_str()));
    assert!(metadata.contains("plan-current"));
    assert_eq!(options[0].conversation_id_override, None);
    assert_eq!(
        options[0].queue_policy,
        SendQueuePolicy::RequireImmediateStart
    );
    assert_eq!(options[0].harness_override, None);
    assert_eq!(options[0].model_override, None);
    assert_eq!(options[0].logical_effort_override, None);
    assert_eq!(options[0].service_tier_override, None);

    let prompts = chat.get_sent_messages().await;
    let prompt = prompts
        .first()
        .expect("verification action should receive its review contract");
    for required_lens in [
        "industry best practices",
        "reuse existing components",
        "UI/UX",
        "product sense",
        "remote base branch drift",
        "instead of assuming no drift",
    ] {
        assert!(
            prompt.contains(required_lens),
            "verification prompt must require the {required_lens:?} review lens"
        );
    }
}

#[tokio::test]
async fn acceptance_does_not_auto_verify_when_verification_is_advisory() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = mock_chat(&state);

    ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(true, false))
        .await
        .expect("advisory verification must not delay acceptance");

    assert_eq!(
        chat.call_count(),
        0,
        "no verification turn should be queued"
    );
}

#[tokio::test]
async fn acceptance_requires_manual_verification_when_auto_trigger_is_disabled() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = mock_chat(&state);

    let error =
        ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(false, true))
            .await
            .expect_err("required unverified plan must remain blocked");

    assert!(error.to_string().contains("must be verified"));
    assert_eq!(chat.call_count(), 0, "manual mode must not queue a turn");
}

#[tokio::test]
async fn exact_current_proof_allows_acceptance_without_another_turn() {
    let state = AppState::new_test();
    let mut session = session_with_plan(&state).await;
    session.verified_plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    session.verified_plan_blueprint_artifact_id =
        Some(ArtifactId::from_string("plan-current-blueprint"));
    let chat = mock_chat(&state);

    ensure_plan_verification_for_acceptance(&state, &chat, &session, &policy(true, true))
        .await
        .expect("exact proof should open the acceptance gate");

    assert_eq!(
        chat.call_count(),
        0,
        "verified plans must not queue duplicate work"
    );
}

#[tokio::test]
async fn verification_status_reports_no_plan_and_exact_proof_without_run_inference() {
    let state = AppState::new_test();
    let no_plan = state
        .ideation_session_repo
        .create(IdeationSession::new(ProjectId::new()))
        .await
        .unwrap();

    let status = get_plan_verification_status(&state, &no_plan.id)
        .await
        .unwrap();
    assert_eq!(status.status, PlanVerificationStatusKind::Unverified);
    assert!(!status.in_progress);

    let unverified = session_with_plan(&state).await;
    let status = get_plan_verification_status(&state, &unverified.id)
        .await
        .unwrap();
    assert_eq!(status.status, PlanVerificationStatusKind::Unverified);
    assert!(!status.in_progress);

    let plan_target = plan_bundle_action_target(&unverified);
    state.message_queue.queue_with_overrides(
        crate::domain::entities::ChatContextType::Ideation,
        unverified.id.as_str(),
        "Verify plan".to_string(),
        Some(format!(
            r#"{{"ralphx_action_kind":"verify_plan","ralphx_action_context_id":"{}","ralphx_action_target_id":"{}"}}"#,
            unverified.id, plan_target
        )),
        None,
        None,
    );
    let status = get_plan_verification_status(&state, &unverified.id)
        .await
        .unwrap();
    assert_eq!(status.status, PlanVerificationStatusKind::Queued);
    assert!(status.in_progress);

    let mut verified = IdeationSession::new(ProjectId::new());
    verified.plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    verified.plan_blueprint_artifact_id = Some(ArtifactId::from_string("plan-current-blueprint"));
    verified.plan_contract_version = 2;
    verified.verified_plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    verified.verified_plan_blueprint_artifact_id =
        Some(ArtifactId::from_string("plan-current-blueprint"));
    let verified = state.ideation_session_repo.create(verified).await.unwrap();

    let status = get_plan_verification_status(&state, &verified.id)
        .await
        .unwrap();
    assert_eq!(status.status, PlanVerificationStatusKind::Verified);
    assert!(!status.in_progress);
    assert_eq!(status.started_at, None);
    assert_eq!(status.completed_at, None);
}

#[tokio::test]
async fn verification_status_prefers_active_owner_action() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(session.id.clone()))
        .await
        .unwrap();
    let mut verifier = AgentRun::new(conversation.id);
    verifier.action_kind = Some(AgentRunActionKind::VerifyPlan);
    verifier.action_context_id = Some(session.id.as_str().to_string());
    verifier.action_target_id = Some(plan_bundle_action_target(&session));
    state.agent_run_repo.create(verifier).await.unwrap();

    let status = get_plan_verification_status(&state, &session.id)
        .await
        .unwrap();
    assert_eq!(status.status, PlanVerificationStatusKind::Verifying);
    assert!(status.in_progress);
}

#[tokio::test]
async fn concurrent_verification_requests_admit_exactly_one_turn() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(session.id.clone()))
        .await
        .expect("active ideation conversation should be created");
    let chat = mock_chat(&state);

    let (first, second) = tokio::join!(
        request_plan_verification(
            &state,
            &chat,
            &session.id,
            PlanVerificationRequestSource::Manual,
        ),
        request_plan_verification(
            &state,
            &chat,
            &session.id,
            PlanVerificationRequestSource::Automatic,
        ),
    );

    let outcomes = [
        first.expect("first request should settle"),
        second.expect("second request should settle"),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PlanVerificationRequestOutcome::Queued)
            .count(),
        1,
        "exactly one request should launch the verifier"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PlanVerificationRequestOutcome::AlreadyRunning)
            .count(),
        1,
        "the serialized follower should observe the active typed run"
    );
    assert_eq!(
        chat.call_count(),
        1,
        "admission must be serialized per plan"
    );
}

#[tokio::test]
async fn failed_verification_launch_releases_admission_without_writing_proof() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = mock_chat(&state);
    chat.set_available(false).await;

    request_plan_verification(
        &state,
        &chat,
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .expect_err("failed launch must remain an error");

    assert!(state.plan_verification_admissions.is_empty());
    let stored = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("load session")
        .expect("session should exist");
    assert_eq!(stored.verified_plan_artifact_id, None);
    assert_eq!(stored.verified_plan_agent_run_id, None);
}

#[tokio::test]
async fn nominal_send_without_typed_run_or_durable_queue_fails_closed() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let chat = MockChatService::new();

    let error = request_plan_verification(
        &state,
        &chat,
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .expect_err("nominal send without action authority must fail");

    assert!(error.to_string().contains("matching typed run"));
    assert!(state.plan_verification_admissions.is_empty());
}

#[tokio::test]
async fn stale_admission_marker_is_not_reported_as_durable_queue_success() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    state.plan_verification_admissions.insert(
        session.id.as_str().to_string(),
        plan_bundle_action_target(&session),
    );
    let chat = mock_chat(&state);

    let outcome = request_plan_verification(
        &state,
        &chat,
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .unwrap();

    assert_eq!(outcome, PlanVerificationRequestOutcome::Queued);
    assert_eq!(chat.call_count(), 1);
    assert!(state.plan_verification_admissions.is_empty());
}

#[tokio::test]
async fn post_insert_repository_failure_clears_marker_and_allows_retry() {
    let mut state = AppState::new_test();
    let session = session_with_plan(&state).await;
    state.queued_message_repo = Arc::new(FailSecondQueueList {
        calls: AtomicUsize::new(0),
    });

    let error = request_plan_verification(
        &state,
        &MockChatService::new(),
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .expect_err("the second durable queue read should fail");

    assert!(error.to_string().contains("injected queue read failure"));
    assert!(state.plan_verification_admissions.is_empty());

    state.queued_message_repo = Arc::new(MemoryQueuedMessageRepository::new());
    let retry_chat = mock_chat(&state);
    let retry = request_plan_verification(
        &state,
        &retry_chat,
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .unwrap();
    assert_eq!(retry, PlanVerificationRequestOutcome::Queued);
    assert_eq!(retry_chat.call_count(), 1);
}

#[tokio::test]
async fn manual_reverification_preserves_exact_proof_while_automatic_is_idempotent() {
    let state = AppState::new_test();
    let mut session = IdeationSession::new(ProjectId::new());
    session.plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    session.plan_blueprint_artifact_id = Some(ArtifactId::from_string("plan-current-blueprint"));
    session.plan_contract_version = 2;
    session.verified_plan_artifact_id = Some(ArtifactId::from_string("plan-current"));
    session.verified_plan_blueprint_artifact_id =
        Some(ArtifactId::from_string("plan-current-blueprint"));
    let session = state.ideation_session_repo.create(session).await.unwrap();

    let automatic = request_plan_verification(
        &state,
        &mock_chat(&state),
        &session.id,
        PlanVerificationRequestSource::Automatic,
    )
    .await
    .unwrap();
    assert_eq!(
        automatic,
        crate::application::plan_verification_service::PlanVerificationRequestOutcome::AlreadyVerified
    );

    let manual = request_plan_verification(
        &state,
        &mock_chat(&state),
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .unwrap();
    assert_eq!(
        manual,
        crate::application::plan_verification_service::PlanVerificationRequestOutcome::Queued
    );
    let stored = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.verified_plan_artifact_id, session.plan_artifact_id);
}

#[tokio::test]
async fn completed_current_plain_plan_run_admits_one_automatic_verifier() {
    let state = AppState::new_test();
    let (_session, conversation, run) = completed_plan_workspace_run(&state, None).await;
    let chat = mock_chat(&state);

    let disposition =
        admit_automatic_plan_verification(&state, &chat, &conversation.id, &run.id, true)
            .await
            .unwrap();

    assert_eq!(
        disposition,
        AutomaticPlanVerificationDisposition::VerificationPending
    );
    assert_eq!(chat.call_count(), 1);
}

#[tokio::test]
async fn automatic_admission_rejects_typed_and_non_winning_finalizers() {
    let state = AppState::new_test();
    let (_session, conversation, run) =
        completed_plan_workspace_run(&state, Some(AgentRunActionKind::VerifyPlan)).await;
    let chat = mock_chat(&state);

    for completion_applied in [true, false] {
        let disposition = admit_automatic_plan_verification(
            &state,
            &chat,
            &conversation.id,
            &run.id,
            completion_applied,
        )
        .await
        .unwrap();
        assert_eq!(
            disposition,
            AutomaticPlanVerificationDisposition::NotEligible
        );
    }
    assert_eq!(chat.call_count(), 0);
}

#[tokio::test]
async fn automatic_admission_rejects_missing_and_superseded_finalizers() {
    let state = AppState::new_test();
    let (_session, conversation, run) = completed_plan_workspace_run(&state, None).await;
    let chat = mock_chat(&state);

    assert_eq!(
        admit_automatic_plan_verification(
            &state,
            &chat,
            &conversation.id,
            &AgentRunId::new(),
            true,
        )
        .await
        .unwrap(),
        AutomaticPlanVerificationDisposition::NotEligible
    );

    let newer = state
        .agent_run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .unwrap();
    assert!(state
        .agent_run_repo
        .complete_if_running(&newer.id)
        .await
        .unwrap());
    assert_eq!(
        admit_automatic_plan_verification(&state, &chat, &conversation.id, &run.id, true)
            .await
            .unwrap(),
        AutomaticPlanVerificationDisposition::NotEligible
    );
    assert_eq!(chat.call_count(), 0);
}

#[tokio::test]
async fn automatic_admission_requires_active_linked_enabled_workspace_with_a_plan() {
    let state = AppState::new_test();
    let (session, conversation, run) = completed_plan_workspace_run(&state, None).await;
    let chat = mock_chat(&state);
    let mut workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();

    workspace.status = AgentConversationWorkspaceStatus::Archived;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    assert_eq!(
        admit_automatic_plan_verification(&state, &chat, &conversation.id, &run.id, true)
            .await
            .unwrap(),
        AutomaticPlanVerificationDisposition::NotEligible
    );

    workspace.status = AgentConversationWorkspaceStatus::Active;
    workspace.linked_ideation_session_id = None;
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    assert_eq!(
        admit_automatic_plan_verification(&state, &chat, &conversation.id, &run.id, true)
            .await
            .unwrap(),
        AutomaticPlanVerificationDisposition::NotEligible
    );

    workspace.linked_ideation_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let mut settings = state.ideation_settings_repo.get_settings().await.unwrap();
    settings.auto_verify_draft_plans = false;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    assert_eq!(
        admit_automatic_plan_verification(&state, &chat, &conversation.id, &run.id, true)
            .await
            .unwrap(),
        AutomaticPlanVerificationDisposition::NotEligible
    );

    settings.auto_verify_draft_plans = true;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .update_plan_artifact_id(&session.id, None)
        .await
        .unwrap();
    assert_eq!(
        admit_automatic_plan_verification(&state, &chat, &conversation.id, &run.id, true)
            .await
            .unwrap(),
        AutomaticPlanVerificationDisposition::NotEligible
    );
    assert_eq!(chat.call_count(), 0);
}

#[tokio::test]
async fn detached_verification_actions_cannot_poison_owner_status_or_admission() {
    let state = AppState::new_test();
    let (session, _conversation, _) = completed_plan_workspace_run(&state, None).await;
    let detached_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(session.project_id.clone()))
        .await
        .unwrap();
    let mut detached_failed = AgentRun::new(detached_conversation.id);
    detached_failed.action_kind = Some(AgentRunActionKind::VerifyPlan);
    detached_failed.action_context_id = Some(session.id.as_str().to_string());
    detached_failed.action_target_id = Some(plan_bundle_action_target(&session));
    let detached_failed = state.agent_run_repo.create(detached_failed).await.unwrap();
    state
        .agent_run_repo
        .fail(&detached_failed.id, "detached failure")
        .await
        .unwrap();

    let status = get_plan_verification_status(&state, &session.id)
        .await
        .unwrap();
    assert_eq!(status.status, PlanVerificationStatusKind::Unverified);
    assert_eq!(status.error, None);

    let mut detached_running = AgentRun::new(detached_conversation.id);
    detached_running.action_kind = Some(AgentRunActionKind::VerifyPlan);
    detached_running.action_context_id = Some(session.id.as_str().to_string());
    detached_running.action_target_id = Some(plan_bundle_action_target(&session));
    state.agent_run_repo.create(detached_running).await.unwrap();
    let chat = mock_chat(&state);

    let outcome = request_plan_verification(
        &state,
        &chat,
        &session.id,
        PlanVerificationRequestSource::Manual,
    )
    .await
    .unwrap();

    assert_eq!(outcome, PlanVerificationRequestOutcome::Queued);
    assert_eq!(chat.call_count(), 1);
}

#[tokio::test]
async fn terminal_adapter_admits_only_the_current_completed_plan_turn() {
    let state = AppState::new_test();
    let (_session, conversation, run) = completed_plan_workspace_run(&state, None).await;
    let chat = mock_chat(&state);
    let adapter = PlanVerificationCompletionAdapter::from_app_state(&state);

    let disposition = adapter
        .admit_automatic(&chat, &conversation.id, &run.id, true)
        .await
        .expect("explicit terminal adapter should preserve automatic admission");

    assert_eq!(
        disposition,
        AutomaticPlanVerificationDisposition::VerificationPending
    );
    assert_eq!(
        chat.call_count(),
        1,
        "only one verifier turn may be admitted"
    );
}

#[tokio::test]
async fn terminal_adapter_records_once_and_rejects_stale_verification_authority() {
    let state = AppState::new_test();
    let session = session_with_plan(&state).await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(session.project_id.clone()))
        .await
        .unwrap();
    let target = plan_bundle_action_target(&session);
    let mut authoritative = AgentRun::new(conversation.id.clone());
    authoritative.action_kind = Some(AgentRunActionKind::VerifyPlan);
    authoritative.action_context_id = Some(session.id.as_str().to_string());
    authoritative.action_target_id = Some(target.clone());
    let authoritative = state.agent_run_repo.create(authoritative).await.unwrap();
    let authoritative_id = authoritative.id.as_str();
    let adapter = PlanVerificationCompletionAdapter::from_app_state(&state);

    let first = adapter
        .complete_verification(&session.id, &authoritative_id)
        .await
        .expect("current typed verification run must record proof");
    let duplicate = adapter
        .complete_verification(&session.id, &authoritative_id)
        .await
        .expect("the same authoritative completion must remain idempotent");
    assert!(first.newly_recorded);
    assert!(!duplicate.newly_recorded);

    let mut stale = AgentRun::new(conversation.id);
    stale.action_kind = Some(AgentRunActionKind::VerifyPlan);
    stale.action_context_id = Some(session.id.as_str().to_string());
    stale.action_target_id = Some("superseded-plan-target".to_string());
    let stale = state.agent_run_repo.create(stale).await.unwrap();
    let stale_id = stale.id.as_str();
    let stale_error = adapter
        .complete_verification(&session.id, &stale_id)
        .await
        .expect_err("stale action target must not overwrite current proof");
    assert!(stale_error.to_string().contains("rejected"));
    let persisted = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.verified_plan_agent_run_id.as_deref(),
        Some(authoritative_id.as_str()),
        "stale completion must leave the first authoritative proof intact"
    );
}
