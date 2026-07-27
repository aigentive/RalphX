use async_trait::async_trait;
use ralphx_lib::application::{ApplyProposalsOptions, ApplyService, TargetColumn};
use ralphx_lib::domain::entities::ideation::{PLAN_CONTRACT_V1, PLAN_CONTRACT_V2};
use ralphx_lib::domain::entities::{self, *};
use ralphx_lib::domain::repositories::{self, *};
use ralphx_lib::error::{AppError, AppResult};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ========================================================================
// MOCK REPOSITORIES
// ========================================================================

struct MockSessionRepository {
    sessions: Mutex<HashMap<String, IdeationSession>>,
}

impl MockSessionRepository {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn with_session(session: IdeationSession) -> Self {
        let repo = Self::new();
        repo.sessions
            .lock()
            .unwrap()
            .insert(session.id.to_string(), session);
        repo
    }
}

#[async_trait]
impl IdeationSessionRepository for MockSessionRepository {
    async fn create(&self, session: IdeationSession) -> AppResult<IdeationSession> {
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id.to_string(), session.clone());
        Ok(session)
    }

    async fn get_by_id(&self, id: &IdeationSessionId) -> AppResult<Option<IdeationSession>> {
        Ok(self.sessions.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id)
            .cloned()
            .collect())
    }

    async fn update_status(
        &self,
        id: &IdeationSessionId,
        status: IdeationSessionStatus,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.status = status;
        }
        Ok(())
    }

    async fn update_title(
        &self,
        id: &IdeationSessionId,
        title: Option<String>,
        _title_source: &str,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.title = title;
        }
        Ok(())
    }

    async fn update_plan_artifact_id(
        &self,
        id: &IdeationSessionId,
        plan_artifact_id: Option<String>,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.lock().unwrap().get_mut(&id.to_string()) {
            session.plan_artifact_id = plan_artifact_id.map(entities::ArtifactId::from_string);
        }
        Ok(())
    }

    async fn delete(&self, id: &IdeationSessionId) -> AppResult<()> {
        self.sessions.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn get_active_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
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
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id && s.status == status)
            .count() as u32)
    }

    async fn get_by_plan_artifact_id(
        &self,
        plan_artifact_id: &str,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
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
            .lock()
            .unwrap()
            .values()
            .filter(|s| {
                s.inherited_plan_artifact_id.as_ref().map(|id| id.as_str()) == Some(artifact_id)
            })
            .cloned()
            .collect())
    }

    async fn get_children(&self, parent_id: &IdeationSessionId) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.parent_session_id.as_ref() == Some(parent_id))
            .cloned()
            .collect())
    }

    async fn get_ancestor_chain(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn set_parent(
        &self,
        _id: &IdeationSessionId,
        _parent_id: Option<&IdeationSessionId>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn update_verification_state(
        &self,
        _id: &IdeationSessionId,
        _status: entities::VerificationStatus,
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
    ) -> AppResult<(i32, entities::VerificationRunSnapshot)> {
        unimplemented!()
    }

    async fn get_verification_status(
        &self,
        _id: &IdeationSessionId,
    ) -> AppResult<Option<(entities::VerificationStatus, bool)>> {
        Ok(None)
    }

    async fn save_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _snapshot: &entities::VerificationRunSnapshot,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_verification_run_snapshot(
        &self,
        _id: &IdeationSessionId,
        _generation: i32,
    ) -> AppResult<Option<entities::VerificationRunSnapshot>> {
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
        _project_id: &str,
        _status: &str,
        _limit: u32,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(Vec::new())
    }

    async fn get_group_counts(
        &self,
        _project_id: &ProjectId,
        _search: Option<&str>,
    ) -> AppResult<repositories::ideation_session_repository::SessionGroupCounts> {
        unimplemented!()
    }

    async fn list_by_group(
        &self,
        _project_id: &ProjectId,
        _group: &str,
        _offset: u32,
        _limit: u32,
        _search: Option<&str>,
    ) -> AppResult<(
        Vec<repositories::ideation_session_repository::IdeationSessionWithProgress>,
        u32,
    )> {
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
        Ok(vec![])
    }

    async fn list_active_external_sessions_for_archival(
        &self,
        _stale_before: Option<chrono::DateTime<chrono::Utc>>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn list_stalled_external_sessions(
        &self,
        _stalled_before: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(vec![])
    }

    async fn set_dependencies_acknowledged(&self, _session_id: &str) -> AppResult<()> {
        unimplemented!()
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

    async fn list_active_verification_children(
        &self,
    ) -> AppResult<Vec<ralphx_lib::domain::entities::IdeationSession>> {
        Ok(vec![])
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
        Ok(vec![])
    }

    async fn count_pending_sessions_for_project(
        &self,
        _project_id: &ralphx_lib::domain::entities::ProjectId,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn update_acceptance_status(
        &self,
        _session_id: &IdeationSessionId,
        _expected_current: Option<AcceptanceStatus>,
        _new_status: Option<AcceptanceStatus>,
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

    async fn count_active_proposals(&self, _session_id: &IdeationSessionId) -> AppResult<usize> {
        Ok(0)
    }

    async fn get_latest_verification_child(
        &self,
        _parent_id: &IdeationSessionId,
    ) -> AppResult<Option<IdeationSession>> {
        Ok(None)
    }
}

struct MockProposalRepository {
    proposals: Mutex<HashMap<String, TaskProposal>>,
}

impl MockProposalRepository {
    fn new() -> Self {
        Self {
            proposals: Mutex::new(HashMap::new()),
        }
    }

    fn with_proposals(proposals: Vec<TaskProposal>) -> Self {
        let repo = Self::new();
        for p in proposals {
            repo.proposals.lock().unwrap().insert(p.id.to_string(), p);
        }
        repo
    }
}

#[async_trait]
impl TaskProposalRepository for MockProposalRepository {
    async fn create(&self, proposal: TaskProposal) -> AppResult<TaskProposal> {
        self.proposals
            .lock()
            .unwrap()
            .insert(proposal.id.to_string(), proposal.clone());
        Ok(proposal)
    }

    async fn get_by_id(&self, id: &TaskProposalId) -> AppResult<Option<TaskProposal>> {
        Ok(self.proposals.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_session(&self, session_id: &IdeationSessionId) -> AppResult<Vec<TaskProposal>> {
        let mut proposals: Vec<_> = self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id)
            .cloned()
            .collect();
        proposals.sort_by_key(|p| p.sort_order);
        Ok(proposals)
    }

    async fn update(&self, proposal: &TaskProposal) -> AppResult<()> {
        self.proposals
            .lock()
            .unwrap()
            .insert(proposal.id.to_string(), proposal.clone());
        Ok(())
    }

    async fn update_priority(
        &self,
        id: &TaskProposalId,
        assessment: &PriorityAssessment,
    ) -> AppResult<()> {
        if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
            p.suggested_priority = assessment.suggested_priority;
            p.priority_score = assessment.priority_score;
        }
        Ok(())
    }

    async fn update_selection(&self, id: &TaskProposalId, selected: bool) -> AppResult<()> {
        if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
            p.selected = selected;
        }
        Ok(())
    }

    async fn set_created_task_id(&self, id: &TaskProposalId, task_id: &TaskId) -> AppResult<()> {
        if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
            p.created_task_id = Some(task_id.clone());
        }
        Ok(())
    }

    async fn delete(&self, id: &TaskProposalId) -> AppResult<()> {
        self.proposals.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn reorder(
        &self,
        _session_id: &IdeationSessionId,
        proposal_ids: Vec<TaskProposalId>,
    ) -> AppResult<()> {
        for (i, id) in proposal_ids.iter().enumerate() {
            if let Some(p) = self.proposals.lock().unwrap().get_mut(&id.to_string()) {
                p.sort_order = i as i32;
            }
        }
        Ok(())
    }

    async fn get_selected_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        let mut proposals: Vec<_> = self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id && p.selected)
            .cloned()
            .collect();
        proposals.sort_by_key(|p| p.sort_order);
        Ok(proposals)
    }

    async fn count_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        Ok(self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id)
            .count() as u32)
    }

    async fn count_selected_by_session(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        Ok(self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| &p.session_id == session_id && p.selected)
            .count() as u32)
    }

    async fn get_by_plan_artifact_id(
        &self,
        artifact_id: &ArtifactId,
    ) -> AppResult<Vec<TaskProposal>> {
        Ok(self
            .proposals
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.plan_artifact_id.as_ref() == Some(artifact_id))
            .cloned()
            .collect())
    }

    async fn clear_created_task_ids_by_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn archive(&self, _id: &TaskProposalId) -> AppResult<TaskProposal> {
        unimplemented!()
    }
}

struct MockProposalDependencyRepository {
    dependencies: Mutex<Vec<(String, String)>>,
}

impl MockProposalDependencyRepository {
    fn new() -> Self {
        Self {
            dependencies: Mutex::new(Vec::new()),
        }
    }

    fn with_dependencies(deps: Vec<(TaskProposalId, TaskProposalId)>) -> Self {
        let repo = Self::new();
        for (from, to) in deps {
            repo.dependencies
                .lock()
                .unwrap()
                .push((from.to_string(), to.to_string()));
        }
        repo
    }
}

#[async_trait]
impl ProposalDependencyRepository for MockProposalDependencyRepository {
    async fn add_dependency(
        &self,
        proposal_id: &TaskProposalId,
        depends_on_id: &TaskProposalId,
        _reason: Option<&str>,
        _source: Option<&str>,
    ) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .push((proposal_id.to_string(), depends_on_id.to_string()));
        Ok(())
    }

    async fn remove_dependency(
        &self,
        proposal_id: &TaskProposalId,
        depends_on_id: &TaskProposalId,
    ) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .retain(|(p, d)| p != &proposal_id.to_string() || d != &depends_on_id.to_string());
        Ok(())
    }

    async fn get_dependencies(
        &self,
        proposal_id: &TaskProposalId,
    ) -> AppResult<Vec<TaskProposalId>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == &proposal_id.to_string())
            .map(|(_, d)| TaskProposalId::from_string(d.clone()))
            .collect())
    }

    async fn get_dependents(&self, proposal_id: &TaskProposalId) -> AppResult<Vec<TaskProposalId>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d == &proposal_id.to_string())
            .map(|(p, _)| TaskProposalId::from_string(p.clone()))
            .collect())
    }

    async fn get_all_for_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<(TaskProposalId, TaskProposalId, Option<String>)>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .map(|(p, d)| {
                (
                    TaskProposalId::from_string(p.clone()),
                    TaskProposalId::from_string(d.clone()),
                    None,
                )
            })
            .collect())
    }

    async fn would_create_cycle(
        &self,
        _proposal_id: &TaskProposalId,
        _depends_on_id: &TaskProposalId,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn clear_dependencies(&self, proposal_id: &TaskProposalId) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .retain(|(p, d)| p != &proposal_id.to_string() && d != &proposal_id.to_string());
        Ok(())
    }

    async fn clear_session_dependencies(&self, _session_id: &IdeationSessionId) -> AppResult<()> {
        self.dependencies.lock().unwrap().clear();
        Ok(())
    }

    async fn get_all_for_session_with_source(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<(TaskProposalId, TaskProposalId, Option<String>, String)>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .map(|(p, d)| {
                (
                    TaskProposalId::from_string(p.clone()),
                    TaskProposalId::from_string(d.clone()),
                    None,
                    String::from("auto"),
                )
            })
            .collect())
    }

    async fn clear_auto_dependencies(&self, _session_id: &IdeationSessionId) -> AppResult<()> {
        self.dependencies.lock().unwrap().clear();
        Ok(())
    }

    async fn count_dependencies(&self, proposal_id: &TaskProposalId) -> AppResult<u32> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p == &proposal_id.to_string())
            .count() as u32)
    }

    async fn count_dependents(&self, proposal_id: &TaskProposalId) -> AppResult<u32> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d == &proposal_id.to_string())
            .count() as u32)
    }
}

struct MockTaskRepository {
    tasks: Mutex<HashMap<String, Task>>,
}

impl MockTaskRepository {
    fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: Task) -> AppResult<Task> {
        self.tasks
            .lock()
            .unwrap()
            .insert(task.id.to_string(), task.clone());
        Ok(task)
    }

    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>> {
        Ok(self.tasks.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| &t.project_id == project_id)
            .cloned()
            .collect())
    }

    async fn update(&self, task: &Task) -> AppResult<()> {
        self.tasks
            .lock()
            .unwrap()
            .insert(task.id.to_string(), task.clone());
        Ok(())
    }

    async fn update_with_expected_status(
        &self,
        task: &Task,
        expected_status: InternalStatus,
    ) -> AppResult<bool> {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(existing) = tasks.get(&task.id.to_string()) {
            if existing.internal_status != expected_status {
                return Ok(false);
            }
        } else {
            return Ok(false);
        }
        tasks.insert(task.id.to_string(), task.clone());
        Ok(true)
    }

    async fn update_metadata(&self, _id: &TaskId, _metadata: Option<String>) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, id: &TaskId) -> AppResult<()> {
        self.tasks.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn get_by_status(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| &t.project_id == project_id && t.internal_status == status)
            .cloned()
            .collect())
    }

    async fn persist_status_change(
        &self,
        _id: &TaskId,
        _from: InternalStatus,
        _to: InternalStatus,
        _trigger: &str,
    ) -> AppResult<String> {
        Ok(uuid::Uuid::new_v4().to_string())
    }

    async fn get_status_history(
        &self,
        _id: &TaskId,
    ) -> AppResult<Vec<repositories::StatusTransition>> {
        Ok(vec![])
    }

    async fn get_status_entered_at(
        &self,
        _task_id: &TaskId,
        _status: entities::InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_status_last_entered_at(
        &self,
        _task_id: &TaskId,
        _status: entities::InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<chrono::Utc>>> {
        Ok(None)
    }

    async fn get_next_executable(&self, _project_id: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_by_ideation_session(
        &self,
        _session_id: &entities::IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_by_project_filtered(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| {
                &t.project_id == project_id && (include_archived || t.archived_at.is_none())
            })
            .cloned()
            .collect())
    }

    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id.to_string()) {
            task.archived_at = Some(chrono::Utc::now());
            Ok(task.clone())
        } else {
            Err(AppError::NotFound(format!(
                "Task {} not found",
                task_id.as_str()
            )))
        }
    }

    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id.to_string()) {
            task.archived_at = None;
            Ok(task.clone())
        } else {
            Err(AppError::NotFound(format!(
                "Task {} not found",
                task_id.as_str()
            )))
        }
    }

    async fn get_archived_count(
        &self,
        project_id: &ProjectId,
        _ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| &t.project_id == project_id && t.archived_at.is_some())
            .count() as u32)
    }

    async fn list_paginated(
        &self,
        project_id: &ProjectId,
        _statuses: Option<Vec<InternalStatus>>,
        _offset: u32,
        _limit: u32,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
        _categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        // Simple mock implementation
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| &t.project_id == project_id)
            .cloned()
            .collect())
    }

    async fn count_tasks(
        &self,
        project_id: &ProjectId,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| &t.project_id == project_id)
            .count() as u32)
    }

    async fn search(
        &self,
        project_id: &ProjectId,
        query: &str,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        let query_lower = query.to_lowercase();
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .values()
            .filter(|t| {
                &t.project_id == project_id
                    && (include_archived || t.archived_at.is_none())
                    && (t.title.to_lowercase().contains(&query_lower)
                        || t.description
                            .as_ref()
                            .map(|d| d.to_lowercase().contains(&query_lower))
                            .unwrap_or(false))
            })
            .cloned()
            .collect())
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_oldest_ready_tasks(&self, _limit: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn update_latest_state_history_metadata(
        &self,
        _task_id: &TaskId,
        _metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn has_task_in_states(
        &self,
        _project_id: &ProjectId,
        _statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_status_history_batch(
        &self,
        _task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<repositories::StatusTransition>>> {
        Ok(HashMap::new())
    }
}

struct MockTaskDependencyRepository {
    dependencies: Mutex<Vec<(String, String)>>,
}

impl MockTaskDependencyRepository {
    fn new() -> Self {
        Self {
            dependencies: Mutex::new(Vec::new()),
        }
    }
}

struct MockTaskStepRepository {
    steps: Mutex<HashMap<String, TaskStep>>,
}

impl MockTaskStepRepository {
    fn new() -> Self {
        Self {
            steps: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl TaskStepRepository for MockTaskStepRepository {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        self.steps
            .lock()
            .unwrap()
            .insert(step.id.to_string(), step.clone());
        Ok(step)
    }

    async fn get_by_id(&self, id: &entities::TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(self.steps.lock().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<TaskStep>> {
        let mut steps: Vec<_> = self
            .steps
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.task_id == task_id)
            .cloned()
            .collect();
        steps.sort_by_key(|s| s.sort_order);
        Ok(steps)
    }

    async fn get_by_task_and_status(
        &self,
        task_id: &TaskId,
        status: entities::TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        let mut steps: Vec<_> = self
            .steps
            .lock()
            .unwrap()
            .values()
            .filter(|s| &s.task_id == task_id && s.status == status)
            .cloned()
            .collect();
        steps.sort_by_key(|s| s.sort_order);
        Ok(steps)
    }

    async fn update(&self, step: &TaskStep) -> AppResult<()> {
        self.steps
            .lock()
            .unwrap()
            .insert(step.id.to_string(), step.clone());
        Ok(())
    }

    async fn delete(&self, id: &entities::TaskStepId) -> AppResult<()> {
        self.steps.lock().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        self.steps
            .lock()
            .unwrap()
            .retain(|_, s| &s.task_id != task_id);
        Ok(())
    }

    async fn count_by_status(
        &self,
        task_id: &TaskId,
    ) -> AppResult<HashMap<entities::TaskStepStatus, u32>> {
        let steps = self.get_by_task(task_id).await?;
        let mut counts = HashMap::new();
        for step in steps {
            *counts.entry(step.status).or_insert(0) += 1;
        }
        Ok(counts)
    }

    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        let mut created_steps = Vec::new();
        for step in steps {
            self.steps
                .lock()
                .unwrap()
                .insert(step.id.to_string(), step.clone());
            created_steps.push(step);
        }
        Ok(created_steps)
    }

    async fn reorder(
        &self,
        task_id: &TaskId,
        step_ids: Vec<entities::TaskStepId>,
    ) -> AppResult<()> {
        for (new_order, step_id) in step_ids.iter().enumerate() {
            if let Some(step) = self.steps.lock().unwrap().get_mut(&step_id.to_string()) {
                if &step.task_id == task_id {
                    step.sort_order = new_order as i32;
                }
            }
        }
        Ok(())
    }

    async fn reset_all_to_pending(&self, task_id: &TaskId) -> AppResult<u32> {
        use entities::TaskStepStatus;
        let mut count = 0u32;
        for step in self.steps.lock().unwrap().values_mut() {
            if &step.task_id == task_id && step.status != TaskStepStatus::Pending {
                step.status = TaskStepStatus::Pending;
                step.started_at = None;
                step.completed_at = None;
                step.completion_note = None;
                count += 1;
            }
        }
        Ok(count)
    }
}

#[async_trait]
impl TaskDependencyRepository for MockTaskDependencyRepository {
    async fn add_dependency(&self, task_id: &TaskId, depends_on_task_id: &TaskId) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .push((task_id.to_string(), depends_on_task_id.to_string()));
        Ok(())
    }

    async fn remove_dependency(
        &self,
        task_id: &TaskId,
        depends_on_task_id: &TaskId,
    ) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .retain(|(t, d)| t != &task_id.to_string() || d != &depends_on_task_id.to_string());
        Ok(())
    }

    async fn get_blockers(&self, task_id: &TaskId) -> AppResult<Vec<TaskId>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == &task_id.to_string())
            .map(|(_, d)| TaskId::from_string(d.clone()))
            .collect())
    }

    async fn get_blocked_by(&self, task_id: &TaskId) -> AppResult<Vec<TaskId>> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d == &task_id.to_string())
            .map(|(t, _)| TaskId::from_string(t.clone()))
            .collect())
    }

    async fn has_circular_dependency(
        &self,
        _task_id: &TaskId,
        _potential_dep: &TaskId,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn clear_dependencies(&self, task_id: &TaskId) -> AppResult<()> {
        self.dependencies
            .lock()
            .unwrap()
            .retain(|(t, d)| t != &task_id.to_string() && d != &task_id.to_string());
        Ok(())
    }

    async fn count_blockers(&self, task_id: &TaskId) -> AppResult<u32> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(t, _)| t == &task_id.to_string())
            .count() as u32)
    }

    async fn count_blocked_by(&self, task_id: &TaskId) -> AppResult<u32> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, d)| d == &task_id.to_string())
            .count() as u32)
    }

    async fn has_dependency(
        &self,
        task_id: &TaskId,
        depends_on_task_id: &TaskId,
    ) -> AppResult<bool> {
        Ok(self
            .dependencies
            .lock()
            .unwrap()
            .iter()
            .any(|(t, d)| t == &task_id.to_string() && d == &depends_on_task_id.to_string()))
    }

    async fn get_blockers_batch(
        &self,
        _task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<TaskId>>> {
        Ok(HashMap::new())
    }
}

// ========================================================================
// HELPER FUNCTIONS
// ========================================================================

fn create_service(
    session: IdeationSession,
    proposals: Vec<TaskProposal>,
    deps: Vec<(TaskProposalId, TaskProposalId)>,
) -> ApplyService<
    MockSessionRepository,
    MockProposalRepository,
    MockProposalDependencyRepository,
    MockTaskRepository,
    MockTaskDependencyRepository,
    MockTaskStepRepository,
> {
    ApplyService::new(
        Arc::new(MockSessionRepository::with_session(session)),
        Arc::new(MockProposalRepository::with_proposals(proposals)),
        Arc::new(MockProposalDependencyRepository::with_dependencies(deps)),
        Arc::new(MockTaskRepository::new()),
        Arc::new(MockTaskDependencyRepository::new()),
        Arc::new(MockTaskStepRepository::new()),
    )
}

fn create_test_proposal(session_id: &IdeationSessionId, title: &str) -> TaskProposal {
    let mut proposal = TaskProposal::new(
        session_id.clone(),
        title,
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.blueprint_artifact_id =
        Some(ArtifactId::from_string("blueprint-artifact-1".to_string()));
    proposal.blueprint_version_at_creation = Some(1);
    proposal
}

// ========================================================================
// VALIDATION TESTS
// ========================================================================

#[tokio::test]
async fn test_validate_selection_empty() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let service = create_service(session.clone(), vec![], vec![]);

    let result = service.validate_selection(&session.id, &[]).await.unwrap();

    assert!(result.is_valid);
    assert!(result.cycles.is_empty());
    assert!(!result.warnings.is_empty());
}

#[tokio::test]
async fn test_validate_selection_no_cycles() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Setup");
    let p2 = create_test_proposal(&session_id, "Feature");
    let p3 = create_test_proposal(&session_id, "Tests");

    // p2 depends on p1, p3 depends on p2 (linear chain)
    let deps = vec![
        (p2.id.clone(), p1.id.clone()),
        (p3.id.clone(), p2.id.clone()),
    ];

    let service = create_service(
        session.clone(),
        vec![p1.clone(), p2.clone(), p3.clone()],
        deps,
    );

    let result = service
        .validate_selection(&session_id, &[p1.id.clone(), p2.id.clone(), p3.id.clone()])
        .await
        .unwrap();

    assert!(result.is_valid);
    assert!(result.cycles.is_empty());
}

#[tokio::test]
async fn test_validate_selection_with_cycle() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "A");
    let p2 = create_test_proposal(&session_id, "B");
    let p3 = create_test_proposal(&session_id, "C");

    // Circular: p1 -> p2 -> p3 -> p1
    let deps = vec![
        (p1.id.clone(), p2.id.clone()),
        (p2.id.clone(), p3.id.clone()),
        (p3.id.clone(), p1.id.clone()),
    ];

    let service = create_service(
        session.clone(),
        vec![p1.clone(), p2.clone(), p3.clone()],
        deps,
    );

    let result = service
        .validate_selection(&session_id, &[p1.id.clone(), p2.id.clone(), p3.id.clone()])
        .await
        .unwrap();

    assert!(!result.is_valid);
    assert!(!result.cycles.is_empty());
}

#[tokio::test]
async fn test_validate_selection_missing_dependency_warning() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Setup");
    let p2 = create_test_proposal(&session_id, "Feature");

    // p2 depends on p1, but we only select p2
    let deps = vec![(p2.id.clone(), p1.id.clone())];

    let service = create_service(session.clone(), vec![p1.clone(), p2.clone()], deps);

    let result = service
        .validate_selection(&session_id, std::slice::from_ref(&p2.id))
        .await
        .unwrap();

    assert!(result.is_valid); // Still valid, just a warning
    assert!(result.warnings.iter().any(|w| w.contains("not selected")));
}

// ========================================================================
// TARGET COLUMN TESTS
// ========================================================================

#[test]
fn test_target_column_to_status_draft() {
    assert_eq!(TargetColumn::Draft.to_status(), InternalStatus::Backlog);
}

#[test]
fn test_target_column_to_status_backlog() {
    assert_eq!(TargetColumn::Backlog.to_status(), InternalStatus::Backlog);
}

#[test]
fn test_target_column_to_status_todo() {
    assert_eq!(TargetColumn::Todo.to_status(), InternalStatus::Ready);
}

// ========================================================================
// APPLY PROPOSALS TESTS
// ========================================================================

#[tokio::test]
async fn test_apply_proposals_creates_tasks() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Task 1");
    let p2 = create_test_proposal(&session_id, "Task 2");

    let service = create_service(session.clone(), vec![p1.clone(), p2.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone(), p2.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 2);
    assert!(result.created_tasks.iter().any(|t| t.title == "Task 1"));
    assert!(result.created_tasks.iter().any(|t| t.title == "Task 2"));
}

#[tokio::test]
async fn test_apply_proposals_rejects_blueprintless_proposal_after_v2_promotion() {
    let project_id = ProjectId::new();
    let mut session = IdeationSession::new(project_id.clone());
    session.plan_contract_version = PLAN_CONTRACT_V1;
    let session_id = session.id.clone();
    let proposal = TaskProposal::new(
        session_id.clone(),
        "Legacy proposal",
        ProposalCategory::Feature,
        Priority::Medium,
    );

    session.plan_contract_version = PLAN_CONTRACT_V2;
    session.plan_artifact_id = Some(ArtifactId::from_string("overview-2".to_string()));
    session.plan_blueprint_artifact_id = Some(ArtifactId::from_string("blueprint-1".to_string()));

    let task_repo = Arc::new(MockTaskRepository::new());
    let service = ApplyService::new(
        Arc::new(MockSessionRepository::with_session(session)),
        Arc::new(MockProposalRepository::with_proposals(vec![
            proposal.clone()
        ])),
        Arc::new(MockProposalDependencyRepository::with_dependencies(vec![])),
        task_repo.clone(),
        Arc::new(MockTaskDependencyRepository::new()),
        Arc::new(MockTaskStepRepository::new()),
    );

    let error = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![proposal.id],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .expect_err("promoted v2 sessions must reject blueprint-less proposal lineage");

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("blueprint")),
        "expected a blueprint validation error"
    );
    assert!(
        task_repo
            .get_by_project(&project_id)
            .await
            .unwrap()
            .is_empty(),
        "validation must happen before any task is persisted"
    );
}

#[tokio::test]
async fn test_apply_proposals_sets_correct_status() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Task 1");

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Todo,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    assert_eq!(
        result.created_tasks[0].internal_status,
        InternalStatus::Ready
    );
}

#[tokio::test]
async fn test_apply_proposals_preserves_dependencies() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Setup");
    let p2 = create_test_proposal(&session_id, "Feature");

    // p2 depends on p1
    let deps = vec![(p2.id.clone(), p1.id.clone())];

    let service = create_service(session.clone(), vec![p1.clone(), p2.clone()], deps);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone(), p2.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 2);
    assert_eq!(result.dependencies_created, 1);
}

#[tokio::test]
async fn test_apply_proposals_fails_for_archived_session() {
    let project_id = ProjectId::new();
    let mut session = IdeationSession::new(project_id.clone());
    session.archive();
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Task 1");

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_apply_proposals_fails_for_nonexistent_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let nonexistent_id = IdeationSessionId::new();

    let p1 = create_test_proposal(&session.id, "Task 1");

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &nonexistent_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_apply_proposals_fails_with_circular_deps() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "A");
    let p2 = create_test_proposal(&session_id, "B");

    // Circular: p1 -> p2 -> p1
    let deps = vec![
        (p1.id.clone(), p2.id.clone()),
        (p2.id.clone(), p1.id.clone()),
    ];

    let service = create_service(session.clone(), vec![p1.clone(), p2.clone()], deps);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone(), p2.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_apply_proposals_copies_fields_correctly() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let mut p1 = create_test_proposal(&session_id, "My Task");
    p1.description = Some("This is a description".to_string());
    p1.priority_score = 75;
    p1.category = ProposalCategory::Fix;

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    let task = &result.created_tasks[0];
    assert_eq!(task.title, "My Task");
    assert_eq!(task.description, Some("This is a description".to_string()));
    assert_eq!(task.priority, 75);
    // The proposal category "Fix" maps to TaskCategory::Regular for Task entities
    assert_eq!(task.category, TaskCategory::Regular);
}

// ========================================================================
// APPLY SELECTED TESTS
// ========================================================================

#[tokio::test]
async fn test_apply_selected_proposals() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let mut p1 = create_test_proposal(&session_id, "Task 1");
    p1.selected = true;
    let mut p2 = create_test_proposal(&session_id, "Task 2");
    p2.selected = false; // Not selected
    let mut p3 = create_test_proposal(&session_id, "Task 3");
    p3.selected = true;

    let service = create_service(
        session.clone(),
        vec![p1.clone(), p2.clone(), p3.clone()],
        vec![],
    );

    let result = service
        .apply_selected_proposals(&session_id, TargetColumn::Backlog)
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 2);
    assert!(result.created_tasks.iter().any(|t| t.title == "Task 1"));
    assert!(result.created_tasks.iter().any(|t| t.title == "Task 3"));
}

// ========================================================================
// SESSION CONVERSION TESTS
// ========================================================================

#[tokio::test]
async fn test_apply_all_converts_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Task 1");
    let p2 = create_test_proposal(&session_id, "Task 2");

    let service = create_service(session.clone(), vec![p1.clone(), p2.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone(), p2.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert!(result.session_converted);
}

#[tokio::test]
async fn test_apply_partial_does_not_convert_session() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Task 1");
    let p2 = create_test_proposal(&session_id, "Task 2");

    let service = create_service(session.clone(), vec![p1.clone(), p2.clone()], vec![]);

    // Only apply p1, not p2
    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert!(!result.session_converted);
}

// ========================================================================
// STEP IMPORT TESTS
// ========================================================================

#[tokio::test]
async fn test_apply_proposals_imports_steps_from_proposal() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let mut p1 = create_test_proposal(&session_id, "Task with steps");
    p1.steps = Some(r#"["Step 1", "Step 2", "Step 3"]"#.to_string());

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    let task_id = &result.created_tasks[0].id;

    // Verify steps were created
    let steps = service
        .task_step_repo_for_test()
        .get_by_task(task_id)
        .await
        .unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(steps[0].title, "Step 1");
    assert_eq!(steps[1].title, "Step 2");
    assert_eq!(steps[2].title, "Step 3");
    assert_eq!(steps[0].created_by, "proposal");
    assert_eq!(steps[0].sort_order, 0);
    assert_eq!(steps[1].sort_order, 1);
    assert_eq!(steps[2].sort_order, 2);
}

#[tokio::test]
async fn test_apply_proposals_handles_empty_steps() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let mut p1 = create_test_proposal(&session_id, "Task with empty steps");
    p1.steps = Some(r#"[]"#.to_string());

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    let task_id = &result.created_tasks[0].id;

    // No steps should be created
    let steps = service
        .task_step_repo_for_test()
        .get_by_task(task_id)
        .await
        .unwrap();
    assert_eq!(steps.len(), 0);
}

#[tokio::test]
async fn test_apply_proposals_handles_no_steps() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let p1 = create_test_proposal(&session_id, "Task without steps");
    // p1.steps is None by default

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    let task_id = &result.created_tasks[0].id;

    // No steps should be created
    let steps = service
        .task_step_repo_for_test()
        .get_by_task(task_id)
        .await
        .unwrap();
    assert_eq!(steps.len(), 0);
}

#[tokio::test]
async fn test_apply_proposals_handles_invalid_json_steps() {
    let project_id = ProjectId::new();
    let session = IdeationSession::new(project_id.clone());
    let session_id = session.id.clone();

    let mut p1 = create_test_proposal(&session_id, "Task with invalid steps");
    p1.steps = Some("not valid json".to_string());

    let service = create_service(session.clone(), vec![p1.clone()], vec![]);

    let result = service
        .apply_proposals(
            &session_id,
            ApplyProposalsOptions {
                proposal_ids: vec![p1.id.clone()],
                target_column: TargetColumn::Backlog,
            },
        )
        .await
        .unwrap();

    assert_eq!(result.created_tasks.len(), 1);
    let task_id = &result.created_tasks[0].id;

    // No steps should be created due to JSON parse error
    let steps = service
        .task_step_repo_for_test()
        .get_by_task(task_id)
        .await
        .unwrap();
    assert_eq!(steps.len(), 0);
}
