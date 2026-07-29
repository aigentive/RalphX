use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    IdeationSession, IdeationSessionStatus, Priority, ProjectId, ProposalCategory, Task,
    TaskProposal, VerificationGap, VerificationRunSnapshot, VerificationStatus,
};

fn build_service(state: &AppState) -> SessionReopenService {
    let cleanup = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        None,
    );
    SessionReopenService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.task_proposal_repo),
        Arc::clone(&state.ideation_session_repo),
        Arc::clone(&state.plan_branch_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.execution_plan_repo),
        cleanup,
    )
}

#[tokio::test]
async fn test_reopen_accepted_session() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create session and accept it
    let session = IdeationSession::new(project_id.clone());
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create a proposal with created_task_id set
    let mut proposal = TaskProposal::new(
        created.id.clone(),
        "Test Proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.created_task_id = Some(crate::domain::entities::TaskId::new());
    state
        .task_proposal_repo
        .create(proposal.clone())
        .await
        .unwrap();

    // Create tasks linked to this session
    let mut task = Task::new(project_id.clone(), "Test Task".to_string());
    task.ideation_session_id = Some(created.id.clone());
    let created_task = state.task_repo.create(task).await.unwrap();

    // Reopen
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // Verify session is Active
    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, IdeationSessionStatus::Active);

    // Verify task is archived (cleanup now archives instead of deleting)
    let task = state.task_repo.get_by_id(&created_task.id).await.unwrap().unwrap();
    assert!(task.archived_at.is_some(), "Task should be archived after session reopen cleanup");

    // Verify proposal created_task_id is cleared
    let updated_proposal = state
        .task_proposal_repo
        .get_by_id(&proposal.id)
        .await
        .unwrap()
        .unwrap();
    assert!(updated_proposal.created_task_id.is_none());
}

#[tokio::test]
async fn test_reopen_archived_session() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Archived)
        .await
        .unwrap();

    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_reopen_active_session_fails() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state.ideation_session_repo.create(session).await.unwrap();

    let service = build_service(&state);

    let result = service.reopen(&created.id, &state).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reopen_nonexistent_session_fails() {
    let state = AppState::new_test();

    let service = build_service(&state);

    let result = service.reopen(&IdeationSessionId::new(), &state).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_reopen_with_no_tasks() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_reopen_deletes_plan_branch_record_to_allow_re_accept() {
    use crate::domain::entities::{ArtifactId, PlanBranch};

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create session and accept it
    let session = IdeationSession::new(project_id.clone());
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create plan branch for this session
    let plan_branch = PlanBranch::new(
        ArtifactId::new(),
        created.id.clone(),
        project_id,
        "ralphx/test-project/plan-test".to_string(),
        "main".to_string(),
    );
    state
        .plan_branch_repo
        .create(plan_branch.clone())
        .await
        .unwrap();

    // Verify plan branch exists
    let found = state
        .plan_branch_repo
        .get_by_session_id(&created.id)
        .await
        .unwrap();
    assert!(found.is_some());

    // Reopen session
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // Plan branch DB record is deleted so the next accept can INSERT without hitting the UNIQUE INDEX.
    let after_reopen = state
        .plan_branch_repo
        .get_by_session_id(&created.id)
        .await
        .unwrap();
    assert!(after_reopen.is_none(), "plan branch record must be deleted to allow re-accept");
}

#[tokio::test]
async fn test_reopen_marks_execution_plan_superseded() {
    use crate::domain::entities::{ExecutionPlan, ExecutionPlanStatus};

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create session and accept it
    let session = IdeationSession::new(project_id.clone());
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create an active ExecutionPlan for this session
    let plan = ExecutionPlan::new(created.id.clone());
    let created_plan = state.execution_plan_repo.create(plan).await.unwrap();

    // Verify plan is active
    let active = state
        .execution_plan_repo
        .get_active_for_session(&created.id)
        .await
        .unwrap();
    assert!(active.is_some());
    assert_eq!(active.unwrap().status, ExecutionPlanStatus::Active);

    // Reopen session
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // Verify execution plan is now superseded
    let plan_after = state
        .execution_plan_repo
        .get_by_id(&created_plan.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(plan_after.status, ExecutionPlanStatus::Superseded);

    // Verify no active plan remains
    let active_after = state
        .execution_plan_repo
        .get_active_for_session(&created.id)
        .await
        .unwrap();
    assert!(active_after.is_none());
}

#[tokio::test]
async fn test_reopen_without_execution_plan_succeeds() {
    // Reopen should succeed even if no ExecutionPlan exists for the session
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let session = IdeationSession::new(project_id);
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_reopen_resets_verification_state() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create a session that was mid-verification when it was accepted
    let mut session = IdeationSession::new(project_id.clone());
    session.verification_status = VerificationStatus::Reviewing;
    session.verification_in_progress = true;
    session.verification_current_round = Some(3);
    session.verification_max_rounds = Some(5);
    session.verification_gap_count = 2;
    session.verification_gap_score = Some(13);

    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reopened.status, IdeationSessionStatus::Active);
    assert_eq!(
        reopened.verification_status,
        VerificationStatus::Unverified,
        "verification_status must be reset to Unverified on reopen"
    );
    assert!(
        !reopened.verification_in_progress,
        "verification_in_progress must be cleared on reopen"
    );
}

#[tokio::test]
async fn test_reopen_resets_active_generation_snapshot() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let mut session = IdeationSession::new(project_id.clone());
    session.verification_generation = 4;
    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();
    state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &created.id,
            &VerificationRunSnapshot {
                generation: 4,
                status: VerificationStatus::NeedsRevision,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: Some(1),
                convergence_reason: None,
                current_gaps: vec![VerificationGap {
                    severity: "high".to_string(),
                    category: "testing".to_string(),
                    description: "Missing regression".to_string(),
                    why_it_matters: None,
                    source: Some("completeness".to_string()),
                }],
                rounds: vec![],
            },
        )
        .await
        .unwrap();

    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    let snapshot = state
        .ideation_session_repo
        .get_verification_run_snapshot(&created.id, 4)
        .await
        .unwrap()
        .expect("reopen should leave a reset authoritative snapshot for the active generation");
    assert_eq!(snapshot.status, VerificationStatus::Unverified);
    assert!(!snapshot.in_progress);
    assert_eq!(snapshot.current_round, 0);
    assert_eq!(snapshot.max_rounds, 0);
    assert!(snapshot.current_gaps.is_empty());
    assert!(snapshot.rounds.is_empty());
    assert!(snapshot.convergence_reason.is_none());
}

#[tokio::test]
async fn test_reopen_resets_acceptance_cycle_fields() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create session with stale acceptance-cycle fields (simulating a completed accept cycle)
    let mut session = IdeationSession::new(project_id.clone());
    session.expected_proposal_count = Some(5);
    session.dependencies_acknowledged = true;
    session.auto_accept_status = Some("success".to_string());
    session.auto_accept_started_at = Some("2026-01-01T00:00:00Z".to_string());
    session.cross_project_checked = true;

    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reopened.status, IdeationSessionStatus::Active);
    assert!(
        reopened.expected_proposal_count.is_none(),
        "expected_proposal_count must be reset to NULL"
    );
    assert!(
        !reopened.dependencies_acknowledged,
        "dependencies_acknowledged must be reset to false"
    );
    assert!(
        reopened.auto_accept_status.is_none(),
        "auto_accept_status must be reset to NULL"
    );
    assert!(
        reopened.auto_accept_started_at.is_none(),
        "auto_accept_started_at must be reset to NULL"
    );
    assert!(
        !reopened.cross_project_checked,
        "cross_project_checked must be reset to false"
    );
}

/// Delegating wrapper around MemoryIdeationSessionRepository that injects a failure
/// on `reset_acceptance_cycle_fields` to verify reopen propagates errors correctly.
struct FailingResetSessionRepo {
    inner: std::sync::Arc<crate::infrastructure::memory::MemoryIdeationSessionRepository>,
}

impl FailingResetSessionRepo {
    fn new_with_inner(
        inner: std::sync::Arc<crate::infrastructure::memory::MemoryIdeationSessionRepository>,
    ) -> Self {
        Self { inner }
    }
}

#[async_trait::async_trait]
impl crate::domain::repositories::IdeationSessionRepository for FailingResetSessionRepo {
    async fn create(
        &self,
        session: IdeationSession,
    ) -> crate::error::AppResult<IdeationSession> {
        self.inner.create(session).await
    }

    async fn get_by_id(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<Option<IdeationSession>> {
        self.inner.get_by_id(id).await
    }

    async fn get_by_project(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_by_project(project_id).await
    }

    async fn update_status(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        status: IdeationSessionStatus,
    ) -> crate::error::AppResult<()> {
        self.inner.update_status(id, status).await
    }

    async fn update_title(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        title: Option<String>,
        title_source: &str,
    ) -> crate::error::AppResult<()> {
        self.inner.update_title(id, title, title_source).await
    }

    async fn update_plan_artifact_id(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        plan_artifact_id: Option<String>,
    ) -> crate::error::AppResult<()> {
        self.inner.update_plan_artifact_id(id, plan_artifact_id).await
    }

    async fn delete(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<()> {
        self.inner.delete(id).await
    }

    async fn get_active_by_project(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_active_by_project(project_id).await
    }

    async fn count_by_status(
        &self,
        project_id: &ProjectId,
        status: IdeationSessionStatus,
    ) -> crate::error::AppResult<u32> {
        self.inner.count_by_status(project_id, status).await
    }

    async fn get_by_plan_artifact_id(
        &self,
        plan_artifact_id: &str,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_by_plan_artifact_id(plan_artifact_id).await
    }

    async fn get_by_inherited_plan_artifact_id(
        &self,
        artifact_id: &str,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_by_inherited_plan_artifact_id(artifact_id).await
    }

    async fn get_children(
        &self,
        parent_id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_children(parent_id).await
    }

    async fn get_ancestor_chain(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_ancestor_chain(session_id).await
    }

    async fn set_parent(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        parent_id: Option<&crate::domain::entities::IdeationSessionId>,
    ) -> crate::error::AppResult<()> {
        self.inner.set_parent(id, parent_id).await
    }

    async fn update_verification_state(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        status: VerificationStatus,
        in_progress: bool,
    ) -> crate::error::AppResult<()> {
        self.inner
            .update_verification_state(id, status, in_progress)
            .await
    }

    async fn reset_verification(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<bool> {
        self.inner.reset_verification(id).await
    }

    async fn get_verification_status(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<Option<(VerificationStatus, bool)>> {
        self.inner.get_verification_status(id).await
    }

    async fn save_verification_run_snapshot(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        snapshot: &crate::domain::entities::VerificationRunSnapshot,
    ) -> crate::error::AppResult<()> {
        self.inner.save_verification_run_snapshot(id, snapshot).await
    }

    async fn get_verification_run_snapshot(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        generation: i32,
    ) -> crate::error::AppResult<Option<crate::domain::entities::VerificationRunSnapshot>> {
        self.inner.get_verification_run_snapshot(id, generation).await
    }

    async fn revert_plan_and_skip_verification(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        new_plan_artifact_id: String,
        convergence_reason: String,
    ) -> crate::error::AppResult<()> {
        self.inner
            .revert_plan_and_skip_verification(id, new_plan_artifact_id, convergence_reason)
            .await
    }

    async fn revert_plan_and_skip_with_artifact(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
        new_artifact_id: String,
        artifact_type_str: String,
        artifact_name: String,
        content_text: String,
        version: u32,
        previous_version_id: String,
        convergence_reason: String,
    ) -> crate::error::AppResult<()> {
        self.inner
            .revert_plan_and_skip_with_artifact(
                session_id,
                new_artifact_id,
                artifact_type_str,
                artifact_name,
                content_text,
                version,
                previous_version_id,
                convergence_reason,
            )
            .await
    }

    async fn increment_verification_generation(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<()> {
        self.inner.increment_verification_generation(session_id).await
    }

    async fn reset_and_begin_reverify(
        &self,
        session_id: &str,
    ) -> crate::error::AppResult<(i32, crate::domain::entities::VerificationRunSnapshot)> {
        self.inner.reset_and_begin_reverify(session_id).await
    }

    async fn get_stale_in_progress_sessions(
        &self,
        stale_before: chrono::DateTime<chrono::Utc>,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_stale_in_progress_sessions(stale_before).await
    }

    async fn get_all_in_progress_sessions(&self) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_all_in_progress_sessions().await
    }

    async fn get_verification_children(
        &self,
        parent_session_id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_verification_children(parent_session_id).await
    }

    async fn get_by_project_and_status(
        &self,
        project_id: &str,
        status: &str,
        limit: u32,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_by_project_and_status(project_id, status, limit).await
    }

    async fn get_group_counts(
        &self,
        project_id: &ProjectId,
        search: Option<&str>,
    ) -> crate::error::AppResult<
        crate::domain::repositories::ideation_session_repository::SessionGroupCounts,
    > {
        self.inner.get_group_counts(project_id, search).await
    }

    async fn list_by_group(
        &self,
        project_id: &ProjectId,
        group: &str,
        offset: u32,
        limit: u32,
        search: Option<&str>,
    ) -> crate::error::AppResult<(
        Vec<crate::domain::repositories::ideation_session_repository::IdeationSessionWithProgress>,
        u32,
    )> {
        self.inner.list_by_group(project_id, group, offset, limit, search).await
    }

    fn set_expected_proposal_count_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
        _count: u32,
    ) -> crate::error::AppResult<()>
    where
        Self: Sized,
    {
        Err(crate::error::AppError::Infrastructure(
            "set_expected_proposal_count_sync not supported in FailingResetSessionRepo".to_string(),
        ))
    }

    async fn set_auto_accept_status(
        &self,
        session_id: &str,
        status: &str,
        auto_accept_started_at: Option<String>,
    ) -> crate::error::AppResult<()> {
        self.inner
            .set_auto_accept_status(session_id, status, auto_accept_started_at)
            .await
    }

    fn count_active_by_session_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
    ) -> crate::error::AppResult<i64>
    where
        Self: Sized,
    {
        Err(crate::error::AppError::Infrastructure(
            "count_active_by_session_sync not supported in FailingResetSessionRepo".to_string(),
        ))
    }

    async fn get_by_idempotency_key(
        &self,
        api_key_id: &str,
        idempotency_key: &str,
    ) -> crate::error::AppResult<Option<IdeationSession>> {
        self.inner.get_by_idempotency_key(api_key_id, idempotency_key).await
    }

    async fn update_external_activity_phase(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        phase: Option<&str>,
    ) -> crate::error::AppResult<()> {
        self.inner.update_external_activity_phase(id, phase).await
    }

    async fn update_external_last_read_message_id(
        &self,
        id: &crate::domain::entities::IdeationSessionId,
        message_id: &str,
    ) -> crate::error::AppResult<()> {
        self.inner.update_external_last_read_message_id(id, message_id).await
    }

    async fn list_active_external_by_project(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.list_active_external_by_project(project_id).await
    }

    async fn list_active_external_sessions_for_archival(
        &self,
        stale_before: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.list_active_external_sessions_for_archival(stale_before).await
    }

    async fn list_stalled_external_sessions(
        &self,
        stalled_before: chrono::DateTime<chrono::Utc>,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.list_stalled_external_sessions(stalled_before).await
    }

    async fn set_dependencies_acknowledged(
        &self,
        session_id: &str,
    ) -> crate::error::AppResult<()> {
        self.inner.set_dependencies_acknowledged(session_id).await
    }

    /// Always returns an error to simulate a DB failure in reset_acceptance_cycle_fields.
    async fn reset_acceptance_cycle_fields(
        &self,
        _session_id: &str,
    ) -> crate::error::AppResult<()> {
        Err(crate::error::AppError::Database(
            "reset_acceptance_cycle_fields failed (injected test error)".to_string(),
        ))
    }

    async fn touch_updated_at(&self, session_id: &str) -> crate::error::AppResult<()> {
        self.inner.touch_updated_at(session_id).await
    }

    async fn update_last_effective_model(
        &self,
        _session_id: &str,
        _model: &str,
    ) -> crate::error::AppResult<()> {
        Ok(())
    }

    async fn list_active_verification_children(
        &self,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.list_active_verification_children().await
    }

    async fn set_pending_initial_prompt(
        &self,
        session_id: &str,
        prompt: Option<String>,
    ) -> crate::error::AppResult<()> {
        self.inner.set_pending_initial_prompt(session_id, prompt).await
    }

    async fn set_pending_initial_prompt_if_unset(
        &self,
        session_id: &str,
        prompt: String,
    ) -> crate::error::AppResult<bool> {
        self.inner.set_pending_initial_prompt_if_unset(session_id, prompt).await
    }

    async fn claim_pending_session_for_project(
        &self,
        project_id: &str,
    ) -> crate::error::AppResult<Option<(String, String)>> {
        self.inner.claim_pending_session_for_project(project_id).await
    }

    async fn list_projects_with_pending_sessions(&self) -> crate::error::AppResult<Vec<String>> {
        self.inner.list_projects_with_pending_sessions().await
    }

    async fn count_pending_sessions_for_project(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<u32> {
        self.inner.count_pending_sessions_for_project(project_id).await
    }

    async fn update_acceptance_status(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
        expected_current: Option<crate::domain::entities::AcceptanceStatus>,
        new_status: Option<crate::domain::entities::AcceptanceStatus>,
    ) -> crate::error::AppResult<bool> {
        self.inner.update_acceptance_status(session_id, expected_current, new_status).await
    }

    async fn get_sessions_with_pending_acceptance(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_sessions_with_pending_acceptance(project_id).await
    }

    async fn set_verification_confirmation_status(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
        status: Option<crate::domain::entities::VerificationConfirmationStatus>,
    ) -> crate::error::AppResult<()> {
        self.inner.set_verification_confirmation_status(session_id, status).await
    }

    async fn get_pending_verification_confirmations(
        &self,
        project_id: &ProjectId,
    ) -> crate::error::AppResult<Vec<IdeationSession>> {
        self.inner.get_pending_verification_confirmations(project_id).await
    }

    async fn count_active_proposals(
        &self,
        session_id: &crate::domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<usize> {
        self.inner.count_active_proposals(session_id).await
    }

    async fn get_latest_verification_child(
        &self,
        parent_id: &ralphx_domain::entities::IdeationSessionId,
    ) -> crate::error::AppResult<Option<IdeationSession>> {
        self.inner.get_latest_verification_child(parent_id).await
    }
}

#[tokio::test]
async fn test_reopen_field_reset_error_propagates() {
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create a shared inner repo so we can create the session AND read it back after the error
    let shared_inner = std::sync::Arc::new(MemoryIdeationSessionRepository::new());

    let session = IdeationSession::new(project_id.clone());
    let created = shared_inner.create(session).await.unwrap();
    shared_inner
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Build service with the failing session repo (wraps shared_inner)
    let failing_repo: std::sync::Arc<dyn crate::domain::repositories::IdeationSessionRepository> =
        std::sync::Arc::new(FailingResetSessionRepo::new_with_inner(
            std::sync::Arc::clone(&shared_inner),
        ));

    let cleanup = crate::application::task_cleanup_service::TaskCleanupService::new(
        std::sync::Arc::clone(&state.task_repo),
        std::sync::Arc::clone(&state.project_repo),
        std::sync::Arc::clone(&state.running_agent_registry),
        None,
    );
    let service = SessionReopenService::new(
        std::sync::Arc::clone(&state.task_repo),
        std::sync::Arc::clone(&state.task_proposal_repo),
        failing_repo,
        std::sync::Arc::clone(&state.plan_branch_repo),
        std::sync::Arc::clone(&state.project_repo),
        std::sync::Arc::clone(&state.execution_plan_repo),
        cleanup,
    );

    // Reopen should fail because reset_acceptance_cycle_fields returns an error
    let result = service.reopen(&created.id, &state).await;
    assert!(result.is_err(), "reopen must propagate reset_acceptance_cycle_fields error");

    // Session status must NOT be Active — step 7 (reset) failed before step 8 (update_status)
    let session_after = shared_inner.get_by_id(&created.id).await.unwrap().unwrap();
    assert_ne!(
        session_after.status,
        IdeationSessionStatus::Active,
        "session must not be Active when reset_acceptance_cycle_fields fails"
    );
}

#[tokio::test]
async fn test_full_reopen_reaccept_cycle() {
    use crate::domain::entities::{ArtifactId, PlanBranch, TaskId};

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // --- FIRST CYCLE: simulate a completed accept cycle with stale acceptance fields ---
    let mut session = IdeationSession::new(project_id.clone());
    session.expected_proposal_count = Some(3);
    session.dependencies_acknowledged = true;
    session.auto_accept_status = Some("success".to_string());
    session.cross_project_checked = true;

    let created = state.ideation_session_repo.create(session).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create 3 proposals with created_task_id set (simulating applied proposals)
    for _ in 0..3 {
        let mut proposal = TaskProposal::new(
            created.id.clone(),
            "Test Proposal",
            ProposalCategory::Feature,
            Priority::Medium,
        );
        proposal.created_task_id = Some(TaskId::new());
        state.task_proposal_repo.create(proposal).await.unwrap();
    }

    // Create a plan branch (simulating feature branch created during accept)
    let plan_branch = PlanBranch::new(
        ArtifactId::new(),
        created.id.clone(),
        project_id.clone(),
        "ralphx/test/plan-cycle-v1".to_string(),
        "main".to_string(),
    );
    state.plan_branch_repo.create(plan_branch).await.unwrap();

    // Verify setup: plan branch exists before reopen
    let pb_before = state
        .plan_branch_repo
        .get_by_session_id(&created.id)
        .await
        .unwrap();
    assert!(pb_before.is_some(), "plan branch must exist before reopen");

    // --- REOPEN ---
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // --- VERIFY ALL FIELDS RESET ---
    let reopened = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reopened.status, IdeationSessionStatus::Active, "status must be Active after reopen");
    assert!(
        reopened.expected_proposal_count.is_none(),
        "expected_proposal_count must be reset to NULL"
    );
    assert!(
        !reopened.dependencies_acknowledged,
        "dependencies_acknowledged must be reset to false"
    );
    assert!(
        reopened.auto_accept_status.is_none(),
        "auto_accept_status must be reset to NULL"
    );
    assert!(
        !reopened.cross_project_checked,
        "cross_project_checked must be reset to false"
    );

    // Plan branch must be deleted (unblocks next accept's INSERT)
    let pb_after = state
        .plan_branch_repo
        .get_by_session_id(&created.id)
        .await
        .unwrap();
    assert!(
        pb_after.is_none(),
        "PlanBranch record must be deleted after reopen to unblock re-accept"
    );

    // Proposals' created_task_id must be cleared
    let proposals = state
        .task_proposal_repo
        .get_by_session(&created.id)
        .await
        .unwrap();
    assert_eq!(proposals.len(), 3, "all 3 proposals must still exist");
    for p in &proposals {
        assert!(
            p.created_task_id.is_none(),
            "proposal created_task_id must be cleared after reopen"
        );
    }

    // --- SECOND CYCLE: verify re-accept flow can start ---
    // A new PlanBranch can be inserted for the same session_id (old record was deleted)
    let plan_branch_v2 = PlanBranch::new(
        ArtifactId::new(),
        created.id.clone(),
        project_id.clone(),
        "ralphx/test/plan-cycle-v2".to_string(),
        "main".to_string(),
    );
    // This insert would fail with UNIQUE constraint violation if old record was not deleted
    state.plan_branch_repo.create(plan_branch_v2).await.unwrap();

    let new_pb = state
        .plan_branch_repo
        .get_by_session_id(&created.id)
        .await
        .unwrap();
    assert!(
        new_pb.is_some(),
        "new PlanBranch must be created successfully after reopen (no UNIQUE constraint violation)"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests: reopen with active child sessions (lifecycle orphan cleanup)
//
// Covers the gap where reopened sessions were not treated as a clean slate:
// active child sessions were left running after reopen.
//
// After reopen with active children:
//   - Verification child: status=Archived
//   - General child: status=Archived
//   - Parent: status=Active (reopen succeeded)
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_reopen_archives_active_verification_child() {
    use crate::domain::entities::{IdeationSessionStatus, SessionPurpose};

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create parent session and accept it
    let mut parent = IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::Reviewing;
    parent.verification_in_progress = true;
    parent.verification_generation = 1;
    let created = state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create active verification child session
    let mut child = IdeationSession::new(project_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    child.parent_session_id = Some(created.id.clone());
    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child is Active before reopen
    let child_before = state
        .ideation_session_repo
        .get_by_id(&created_child.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_before.status,
        IdeationSessionStatus::Active,
        "verification child must be Active before reopen"
    );

    // Reopen parent — must stop and archive all active children
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // Parent must be Active after reopen
    let parent_after = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent_after.status,
        IdeationSessionStatus::Active,
        "parent must be Active after reopen"
    );

    // Verification child must be archived
    let child_after = state
        .ideation_session_repo
        .get_by_id(&created_child.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_after.status,
        IdeationSessionStatus::Archived,
        "verification child must be archived by reopen"
    );

    // No active verification children remain
    let active_children = state
        .ideation_session_repo
        .get_verification_children(&created.id)
        .await
        .unwrap();
    assert!(
        active_children.is_empty(),
        "no active verification children must remain after reopen"
    );
}

#[tokio::test]
async fn test_reopen_archives_active_general_child() {
    use crate::domain::entities::IdeationSessionStatus;

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create parent session and accept it
    let parent = IdeationSession::new(project_id.clone());
    let created = state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create active general child session (SessionPurpose::General is the default)
    let mut child = IdeationSession::new(project_id.clone());
    child.parent_session_id = Some(created.id.clone());
    let created_child = state.ideation_session_repo.create(child).await.unwrap();

    // Verify child is Active before reopen
    let child_before = state
        .ideation_session_repo
        .get_by_id(&created_child.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_before.status,
        IdeationSessionStatus::Active,
        "general child must be Active before reopen"
    );

    // Reopen parent — must archive ALL active children, not just verification ones
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // Parent must be Active after reopen
    let parent_after = state
        .ideation_session_repo
        .get_by_id(&created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent_after.status,
        IdeationSessionStatus::Active,
        "parent must be Active after reopen"
    );

    // General child must be archived (reopen cleans up ALL children, not just verification)
    let child_after = state
        .ideation_session_repo
        .get_by_id(&created_child.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_after.status,
        IdeationSessionStatus::Archived,
        "general child must be archived by reopen — reopen cleans ALL child types"
    );
}

#[tokio::test]
async fn test_reopen_frees_capacity_by_archiving_child_sessions() {
    use crate::domain::entities::{IdeationSessionStatus, SessionPurpose};

    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Create parent and accept it
    let parent = IdeationSession::new(project_id.clone());
    let created = state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .update_status(&created.id, IdeationSessionStatus::Accepted)
        .await
        .unwrap();

    // Create two active children: one verification, one general
    let mut verification_child = IdeationSession::new(project_id.clone());
    verification_child.session_purpose = SessionPurpose::Verification;
    verification_child.parent_session_id = Some(created.id.clone());
    let v_child = state
        .ideation_session_repo
        .create(verification_child)
        .await
        .unwrap();

    let mut general_child = IdeationSession::new(project_id.clone());
    general_child.parent_session_id = Some(created.id.clone());
    let g_child = state
        .ideation_session_repo
        .create(general_child)
        .await
        .unwrap();

    // Before reopen: 2 active children consume capacity (parent is Accepted, not Active)
    let active_before = state
        .ideation_session_repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await
        .unwrap();
    // Only the 2 children are Active — parent is Accepted (different status)
    assert_eq!(active_before, 2, "before reopen: 2 children must be Active (parent is Accepted)");

    // Reopen — must archive all children, freeing their capacity slots
    let service = build_service(&state);
    service.reopen(&created.id, &state).await.unwrap();

    // After reopen: parent becomes Active again, both children are Archived
    // Active count drops from 2 (children) to 1 (parent only) — capacity freed
    let active_after = state
        .ideation_session_repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await
        .unwrap();
    assert_eq!(
        active_after, 1,
        "after reopen: only parent must be Active — children archived, capacity freed"
    );

    // Explicitly verify children are Archived
    let v_child_after = state
        .ideation_session_repo
        .get_by_id(&v_child.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        v_child_after.status,
        IdeationSessionStatus::Archived,
        "verification child must be Archived after reopen"
    );

    let g_child_after = state
        .ideation_session_repo
        .get_by_id(&g_child.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        g_child_after.status,
        IdeationSessionStatus::Archived,
        "general child must be Archived after reopen"
    );
}
