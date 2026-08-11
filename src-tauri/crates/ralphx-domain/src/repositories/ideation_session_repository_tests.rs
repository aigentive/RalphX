use super::*;
use crate::domain::entities::VerificationStatus;
use chrono::Utc;
use std::sync::Arc;

// Mock implementation for testing trait object usage
struct MockIdeationSessionRepository {
    return_session: Option<IdeationSession>,
    sessions: Vec<IdeationSession>,
}

impl MockIdeationSessionRepository {
    fn new() -> Self {
        Self {
            return_session: None,
            sessions: vec![],
        }
    }

    fn with_session(session: IdeationSession) -> Self {
        Self {
            return_session: Some(session.clone()),
            sessions: vec![session],
        }
    }

    fn with_sessions(sessions: Vec<IdeationSession>) -> Self {
        Self {
            return_session: sessions.first().cloned(),
            sessions,
        }
    }
}

#[async_trait]
impl IdeationSessionRepository for MockIdeationSessionRepository {
    async fn create(&self, session: IdeationSession) -> AppResult<IdeationSession> {
        Ok(session)
    }

    async fn get_by_id(&self, _id: &IdeationSessionId) -> AppResult<Option<IdeationSession>> {
        Ok(self.return_session.clone())
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| &s.project_id == project_id)
            .cloned()
            .collect())
    }

    async fn update_status(
        &self,
        _id: &IdeationSessionId,
        _status: IdeationSessionStatus,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_title(
        &self,
        _id: &IdeationSessionId,
        _title: Option<String>,
        _title_source: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_plan_artifact_id(
        &self,
        _id: &IdeationSessionId,
        _plan_artifact_id: Option<String>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &IdeationSessionId) -> AppResult<()> {
        Ok(())
    }

    async fn get_active_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| &s.project_id == project_id && s.status == IdeationSessionStatus::Active)
            .cloned()
            .collect())
    }

    async fn count_by_status(
        &self,
        project_id: &ProjectId,
        status: IdeationSessionStatus,
    ) -> AppResult<u32> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| &s.project_id == project_id && s.status == status)
            .count() as u32)
    }

    async fn get_by_plan_artifact_id(
        &self,
        plan_artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| s.plan_artifact_id.as_ref().map(|id| id.as_str()) == Some(plan_artifact_id))
            .cloned()
            .collect())
    }

    async fn get_by_inherited_plan_artifact_id(
        &self,
        artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| {
                s.inherited_plan_artifact_id.as_ref().map(|id| id.as_str()) == Some(artifact_id)
            })
            .cloned()
            .collect())
    }

    async fn get_children(&self, parent_id: &IdeationSessionId) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .iter()
            .filter(|s| s.parent_session_id.as_ref() == Some(parent_id))
            .cloned()
            .collect())
    }

    async fn get_ancestor_chain(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        let mut chain = Vec::new();
        let mut current_id = session_id.clone();

        // Walk up the parent chain
        while let Some(session) = self.sessions.iter().find(|s| s.id == current_id) {
            if let Some(parent_id) = &session.parent_session_id {
                current_id = parent_id.clone();
                if let Some(parent) = self.sessions.iter().find(|s| s.id == current_id) {
                    chain.push(parent.clone());
                }
            } else {
                break;
            }
        }

        Ok(chain)
    }

    async fn set_parent(
        &self,
        _id: &IdeationSessionId,
        _parent_id: Option<&IdeationSessionId>,
    ) -> AppResult<()> {
        // This is a mock implementation that doesn't actually persist
        // In real implementations, this would update the database
        Ok(())
    }

    async fn update_verification_state(
        &self,
        _id: &IdeationSessionId,
        _status: VerificationStatus,
        _in_progress: bool,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn reset_verification(&self, _id: &IdeationSessionId) -> AppResult<bool> {
        Ok(false)
    }

    async fn reset_and_begin_reverify(
        &self,
        _session_id: &str,
    ) -> AppResult<(i32, crate::domain::entities::VerificationRunSnapshot)> {
        unimplemented!()
    }

    async fn get_verification_status(
        &self,
        _id: &IdeationSessionId,
    ) -> AppResult<Option<(VerificationStatus, bool)>> {
        Ok(None)
    }

    async fn save_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _snapshot: &crate::domain::entities::VerificationRunSnapshot,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _generation: i32,
    ) -> AppResult<Option<crate::domain::entities::VerificationRunSnapshot>> {
        Ok(None)
    }

    async fn revert_plan_and_skip_verification(
        &self,
        _id: &IdeationSessionId,
        _new_plan_artifact_id: String,
        _convergence_reason: String,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn revert_plan_and_skip_with_artifact(
        &self,
        _session_id: &IdeationSessionId,
        _new_artifact_id: String,
        _artifact_type_str: String,
        _artifact_name: String,
        _content_text: String,
        _version: u32,
        _previous_version_id: String,
        _convergence_reason: String,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn increment_verification_generation(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_stale_in_progress_sessions(
        &self,
        _stale_before: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn get_all_in_progress_sessions(&self) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn get_verification_children(
        &self,
        _parent_session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn get_by_project_and_status(
        &self,
        project_id: &str,
        status: &str,
        limit: u32,
    ) -> AppResult<Vec<IdeationSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .iter()
            .filter(|s| s.project_id.as_str() == project_id && s.status.to_string() == status)
            .cloned()
            .collect();
        sessions.truncate(limit as usize);
        Ok(sessions)
    }

    async fn get_group_counts(
        &self,
        _project_id: &ProjectId,
        _search: Option<&str>,
    ) -> AppResult<SessionGroupCounts> {
        unimplemented!()
    }

    async fn list_by_group(
        &self,
        _project_id: &ProjectId,
        _group: &str,
        _offset: u32,
        _limit: u32,
        _search: Option<&str>,
    ) -> AppResult<(Vec<IdeationSessionWithProgress>, u32)> {
        unimplemented!()
    }

    fn set_expected_proposal_count_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
        _count: u32,
    ) -> AppResult<()>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    async fn set_auto_accept_status(
        &self,
        _session_id: &str,
        _status: &str,
        _auto_accept_started_at: Option<String>,
    ) -> AppResult<()> {
        unimplemented!()
    }

    async fn count_active_proposals(&self, _session_id: &IdeationSessionId) -> AppResult<usize> {
        Ok(0)
    }

    fn count_active_by_session_sync(
        _conn: &rusqlite::Connection,
        _session_id: &str,
    ) -> AppResult<i64>
    where
        Self: Sized,
    {
        unimplemented!()
    }

    async fn get_by_idempotency_key(
        &self,
        _api_key_id: &str,
        _idempotency_key: &str,
    ) -> AppResult<Option<IdeationSession>> {
        Ok(None)
    }

    async fn update_external_activity_phase(
        &self,
        _id: &IdeationSessionId,
        _phase: Option<&str>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_external_last_read_message_id(
        &self,
        _id: &IdeationSessionId,
        _message_id: &str,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn list_active_external_by_project(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn list_active_external_sessions_for_archival(
        &self,
        _stale_before: Option<DateTime<Utc>>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn list_stalled_external_sessions(
        &self,
        _stalled_before: DateTime<Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn set_dependencies_acknowledged(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn reset_acceptance_cycle_fields(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn touch_updated_at(&self, _session_id: &str) -> AppResult<()> {
        Ok(())
    }

    async fn update_last_effective_model(&self, _session_id: &str, _model: &str) -> AppResult<()> {
        Ok(())
    }

    async fn list_active_verification_children(&self) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn set_pending_initial_prompt(
        &self,
        _session_id: &str,
        _prompt: Option<String>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn set_pending_initial_prompt_if_unset(
        &self,
        _session_id: &str,
        _prompt: String,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn claim_pending_session_for_project(
        &self,
        _project_id: &str,
    ) -> AppResult<Option<(String, String)>> {
        Ok(None)
    }

    async fn list_projects_with_pending_sessions(&self) -> AppResult<Vec<String>> {
        Ok(Vec::new())
    }

    async fn count_pending_sessions_for_project(&self, _project_id: &ProjectId) -> AppResult<u32> {
        Ok(0)
    }

    async fn update_acceptance_status(
        &self,
        _session_id: &IdeationSessionId,
        _expected_current: Option<crate::domain::entities::AcceptanceStatus>,
        _new_status: Option<crate::domain::entities::AcceptanceStatus>,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn get_sessions_with_pending_acceptance(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn set_verification_confirmation_status(
        &self,
        _session_id: &IdeationSessionId,
        _status: Option<VerificationConfirmationStatus>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_pending_verification_confirmations(
        &self,
        _project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn get_latest_verification_child(
        &self,
        _parent_id: &IdeationSessionId,
    ) -> AppResult<Option<IdeationSession>> {
        Ok(None)
    }
}

fn create_test_session(project_id: &ProjectId) -> IdeationSession {
    IdeationSession {
        id: IdeationSessionId::new(),
        project_id: project_id.clone(),
        title: Some("Test Session".to_string()),
        status: IdeationSessionStatus::Active,
        plan_artifact_id: None,
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        plan_contract_version: 2,
        seed_task_id: None,
        parent_session_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: Default::default(),
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        origin: Default::default(),
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    }
}

#[test]
fn test_ideation_session_repository_trait_can_be_object_safe() {
    // Verify that IdeationSessionRepository can be used as a trait object
    let repo: Arc<dyn IdeationSessionRepository> = Arc::new(MockIdeationSessionRepository::new());
    assert!(Arc::strong_count(&repo) == 1);
}

#[tokio::test]
async fn test_mock_repository_create() {
    let repo = MockIdeationSessionRepository::new();
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);

    let result = repo.create(session.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().id, session.id);
}

#[tokio::test]
async fn test_mock_repository_get_by_id_returns_none() {
    let repo = MockIdeationSessionRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo.get_by_id(&session_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_mock_repository_get_by_id_returns_session() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let repo = MockIdeationSessionRepository::with_session(session.clone());

    let result = repo.get_by_id(&session.id).await;
    assert!(result.is_ok());
    let returned = result.unwrap();
    assert!(returned.is_some());
    assert_eq!(returned.unwrap().id, session.id);
}

#[tokio::test]
async fn test_mock_repository_get_by_project_empty() {
    let repo = MockIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let result = repo.get_by_project(&project_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_repository_get_by_project_with_sessions() {
    let project_id = ProjectId::new();
    let session1 = create_test_session(&project_id);
    let session2 = create_test_session(&project_id);

    let repo =
        MockIdeationSessionRepository::with_sessions(vec![session1.clone(), session2.clone()]);

    let result = repo.get_by_project(&project_id).await;
    assert!(result.is_ok());
    let sessions = result.unwrap();
    assert_eq!(sessions.len(), 2);
}

#[tokio::test]
async fn test_mock_repository_get_by_project_filters_by_project() {
    let project_id1 = ProjectId::new();
    let project_id2 = ProjectId::new();
    let session1 = create_test_session(&project_id1);
    let session2 = create_test_session(&project_id2);

    let repo =
        MockIdeationSessionRepository::with_sessions(vec![session1.clone(), session2.clone()]);

    let result = repo.get_by_project(&project_id1).await;
    assert!(result.is_ok());
    let sessions = result.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].project_id, project_id1);
}

#[tokio::test]
async fn test_mock_repository_update_status() {
    let repo = MockIdeationSessionRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo
        .update_status(&session_id, IdeationSessionStatus::Archived)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_update_title() {
    let repo = MockIdeationSessionRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo
        .update_title(&session_id, Some("New Title".to_string()), "auto")
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_update_title_to_none() {
    let repo = MockIdeationSessionRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo.update_title(&session_id, None, "auto").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_delete() {
    let repo = MockIdeationSessionRepository::new();
    let session_id = IdeationSessionId::new();

    let result = repo.delete(&session_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_repository_get_active_by_project_empty() {
    let repo = MockIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let result = repo.get_active_by_project(&project_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_repository_get_active_by_project_filters_status() {
    let project_id = ProjectId::new();
    let mut active_session = create_test_session(&project_id);
    active_session.status = IdeationSessionStatus::Active;

    let mut archived_session = create_test_session(&project_id);
    archived_session.status = IdeationSessionStatus::Archived;

    let repo = MockIdeationSessionRepository::with_sessions(vec![
        active_session.clone(),
        archived_session.clone(),
    ]);

    let result = repo.get_active_by_project(&project_id).await;
    assert!(result.is_ok());
    let sessions = result.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, IdeationSessionStatus::Active);
}

#[tokio::test]
async fn test_mock_repository_count_by_status_zero() {
    let repo = MockIdeationSessionRepository::new();
    let project_id = ProjectId::new();

    let result = repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 0);
}

#[tokio::test]
async fn test_mock_repository_count_by_status_counts_correctly() {
    let project_id = ProjectId::new();
    let mut active1 = create_test_session(&project_id);
    active1.status = IdeationSessionStatus::Active;

    let mut active2 = create_test_session(&project_id);
    active2.status = IdeationSessionStatus::Active;

    let mut archived = create_test_session(&project_id);
    archived.status = IdeationSessionStatus::Archived;

    let repo = MockIdeationSessionRepository::with_sessions(vec![
        active1.clone(),
        active2.clone(),
        archived.clone(),
    ]);

    let active_count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await;
    assert!(active_count.is_ok());
    assert_eq!(active_count.unwrap(), 2);

    let archived_count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Archived)
        .await;
    assert!(archived_count.is_ok());
    assert_eq!(archived_count.unwrap(), 1);

    let converted_count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Accepted)
        .await;
    assert!(converted_count.is_ok());
    assert_eq!(converted_count.unwrap(), 0);
}

#[tokio::test]
async fn test_repository_trait_object_in_arc() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let repo: Arc<dyn IdeationSessionRepository> =
        Arc::new(MockIdeationSessionRepository::with_session(session.clone()));

    // Use through trait object
    let result = repo.get_by_id(&session.id).await;
    assert!(result.is_ok());

    let all = repo.get_by_project(&project_id).await;
    assert!(all.is_ok());
    assert_eq!(all.unwrap().len(), 1);
}

#[tokio::test]
async fn test_repository_trait_object_status_operations() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let repo: Arc<dyn IdeationSessionRepository> =
        Arc::new(MockIdeationSessionRepository::with_session(session.clone()));

    // Test update_status through trait object
    let result = repo
        .update_status(&session.id, IdeationSessionStatus::Accepted)
        .await;
    assert!(result.is_ok());

    // Test count_by_status through trait object
    let count = repo
        .count_by_status(&project_id, IdeationSessionStatus::Active)
        .await;
    assert!(count.is_ok());
}

#[tokio::test]
async fn test_repository_trait_object_title_operations() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let repo: Arc<dyn IdeationSessionRepository> =
        Arc::new(MockIdeationSessionRepository::with_session(session.clone()));

    // Test update_title through trait object
    let result = repo
        .update_title(&session.id, Some("Updated Title".to_string()), "auto")
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_repository_trait_object_delete_operation() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let repo: Arc<dyn IdeationSessionRepository> =
        Arc::new(MockIdeationSessionRepository::with_session(session.clone()));

    // Test delete through trait object
    let result = repo.delete(&session.id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_children() {
    let project_id = ProjectId::new();
    let parent = create_test_session(&project_id);
    let mut child1 = create_test_session(&project_id);
    child1.parent_session_id = Some(parent.id.clone());
    let mut child2 = create_test_session(&project_id);
    child2.parent_session_id = Some(parent.id.clone());

    let repo = MockIdeationSessionRepository::with_sessions(vec![
        parent.clone(),
        child1.clone(),
        child2.clone(),
    ]);

    let result = repo.get_children(&parent.id).await;
    assert!(result.is_ok());
    let children = result.unwrap();
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn test_get_children_empty() {
    let project_id = ProjectId::new();
    let parent = create_test_session(&project_id);
    let repo = MockIdeationSessionRepository::with_sessions(vec![parent.clone()]);

    let result = repo.get_children(&parent.id).await;
    assert!(result.is_ok());
    let children = result.unwrap();
    assert!(children.is_empty());
}

#[tokio::test]
async fn test_get_ancestor_chain_single_parent() {
    let project_id = ProjectId::new();
    let grandparent = create_test_session(&project_id);
    let mut parent = create_test_session(&project_id);
    parent.parent_session_id = Some(grandparent.id.clone());
    let mut child = create_test_session(&project_id);
    child.parent_session_id = Some(parent.id.clone());

    let repo = MockIdeationSessionRepository::with_sessions(vec![
        grandparent.clone(),
        parent.clone(),
        child.clone(),
    ]);

    let result = repo.get_ancestor_chain(&child.id).await;
    assert!(result.is_ok());
    let chain = result.unwrap();
    // Should include parent and grandparent
    assert!(!chain.is_empty());
    assert_eq!(chain[0].id, parent.id);
}

#[tokio::test]
async fn test_get_ancestor_chain_three_levels_deep() {
    let project_id = ProjectId::new();
    let level1 = create_test_session(&project_id);
    let mut level2 = create_test_session(&project_id);
    level2.parent_session_id = Some(level1.id.clone());
    let mut level3 = create_test_session(&project_id);
    level3.parent_session_id = Some(level2.id.clone());

    let repo = MockIdeationSessionRepository::with_sessions(vec![
        level1.clone(),
        level2.clone(),
        level3.clone(),
    ]);

    let result = repo.get_ancestor_chain(&level3.id).await;
    assert!(result.is_ok());
    let chain = result.unwrap();
    // Should walk up the chain: level3 → level2 → level1
    assert!(!chain.is_empty());
}

#[tokio::test]
async fn test_get_ancestor_chain_no_parent() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let repo = MockIdeationSessionRepository::with_sessions(vec![session.clone()]);

    let result = repo.get_ancestor_chain(&session.id).await;
    assert!(result.is_ok());
    let chain = result.unwrap();
    // Session with no parent should return empty chain
    assert!(chain.is_empty());
}

#[tokio::test]
async fn test_set_parent() {
    let project_id = ProjectId::new();
    let session = create_test_session(&project_id);
    let parent = create_test_session(&project_id);
    let repo = MockIdeationSessionRepository::with_sessions(vec![session.clone()]);

    let result = repo.set_parent(&session.id, Some(&parent.id)).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_set_parent_to_none() {
    let project_id = ProjectId::new();
    let mut session = create_test_session(&project_id);
    let parent = create_test_session(&project_id);
    session.parent_session_id = Some(parent.id.clone());
    let repo = MockIdeationSessionRepository::with_sessions(vec![session.clone()]);

    let result = repo.set_parent(&session.id, None).await;
    assert!(result.is_ok());
}
