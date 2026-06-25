// In-memory IdeationSessionRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe in-memory storage

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::domain::entities::{
    AcceptanceStatus, IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId,
    SessionOrigin, VerificationConfirmationStatus, VerificationRunSnapshot, VerificationStatus,
};
use crate::domain::repositories::ideation_session_repository::{
    IdeationSessionWithProgress, SessionGroupCounts,
};
use crate::domain::repositories::IdeationSessionRepository;
use crate::error::{AppError, AppResult};

/// In-memory implementation of IdeationSessionRepository for testing
pub struct MemoryIdeationSessionRepository {
    sessions: RwLock<HashMap<String, IdeationSession>>,
    verification_runs: RwLock<HashMap<(String, i32), VerificationRunSnapshot>>,
}

impl MemoryIdeationSessionRepository {
    /// Create a new empty repository
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            verification_runs: RwLock::new(HashMap::new()),
        }
    }

    fn clear_verification_summary(session: &mut IdeationSession) {
        session.verification_current_round = None;
        session.verification_max_rounds = None;
        session.verification_gap_count = 0;
        session.verification_gap_score = None;
        session.verification_convergence_reason = None;
    }

    fn apply_verification_snapshot_summary(
        session: &mut IdeationSession,
        snapshot: &VerificationRunSnapshot,
    ) {
        session.verification_current_round =
            (snapshot.current_round > 0).then_some(snapshot.current_round);
        session.verification_max_rounds = (snapshot.max_rounds > 0).then_some(snapshot.max_rounds);
        session.verification_gap_count = snapshot.current_gaps.len() as u32;
        session.verification_gap_score = (!snapshot.current_gaps.is_empty())
            .then_some(crate::domain::services::gap_score(&snapshot.current_gaps));
        session.verification_convergence_reason = snapshot.convergence_reason.clone();
    }
}

impl Default for MemoryIdeationSessionRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IdeationSessionRepository for MemoryIdeationSessionRepository {
    async fn create(&self, session: IdeationSession) -> AppResult<IdeationSession> {
        self.sessions
            .write()
            .unwrap()
            .insert(session.id.to_string(), session.clone());
        Ok(session)
    }

    async fn get_by_id(&self, id: &IdeationSessionId) -> AppResult<Option<IdeationSession>> {
        Ok(self.sessions.read().unwrap().get(&id.to_string()).cloned())
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<IdeationSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id)
            .cloned()
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    async fn update_status(
        &self,
        id: &IdeationSessionId,
        status: IdeationSessionStatus,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            session.status = status;
            session.updated_at = Utc::now();
            match status {
                IdeationSessionStatus::Archived => {
                    session.archived_at = Some(Utc::now());
                    session.verification_in_progress = false;
                    session.pending_initial_prompt = None;
                }
                IdeationSessionStatus::Accepted => {
                    session.converted_at = Some(Utc::now());
                    session.pending_initial_prompt = None;
                }
                IdeationSessionStatus::Active => {
                    session.archived_at = None;
                    session.converted_at = None;
                }
            }
        }
        Ok(())
    }

    async fn update_title(
        &self,
        id: &IdeationSessionId,
        title: Option<String>,
        title_source: &str,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            session.title = title;
            session.title_source = Some(title_source.to_string());
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_plan_artifact_id(
        &self,
        id: &IdeationSessionId,
        plan_artifact_id: Option<String>,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            session.plan_artifact_id =
                plan_artifact_id.map(crate::domain::entities::ArtifactId::from_string);
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn delete(&self, id: &IdeationSessionId) -> AppResult<()> {
        self.sessions.write().unwrap().remove(&id.to_string());
        Ok(())
    }

    async fn get_active_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| &s.project_id == project_id && s.status == IdeationSessionStatus::Active)
            .cloned()
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    async fn count_by_status(
        &self,
        project_id: &ProjectId,
        status: IdeationSessionStatus,
    ) -> AppResult<u32> {
        Ok(self
            .sessions
            .read()
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
            .read()
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
            .read()
            .unwrap()
            .values()
            .filter(|s| {
                s.inherited_plan_artifact_id.as_ref().map(|id| id.as_str()) == Some(artifact_id)
            })
            .cloned()
            .collect())
    }

    async fn get_children(&self, parent_id: &IdeationSessionId) -> AppResult<Vec<IdeationSession>> {
        let mut children: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| s.parent_session_id.as_ref() == Some(parent_id))
            .cloned()
            .collect();
        children.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(children)
    }

    async fn get_ancestor_chain(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        let mut chain = Vec::new();
        let sessions_lock = self.sessions.read().unwrap();
        let mut current_id = session_id.clone();

        // Walk up the parent chain
        loop {
            if let Some(session) = sessions_lock.get(&current_id.to_string()) {
                if let Some(parent_id) = &session.parent_session_id {
                    current_id = parent_id.clone();
                    if let Some(parent) = sessions_lock.get(&current_id.to_string()) {
                        chain.push(parent.clone());
                    } else {
                        // Parent doesn't exist, stop here
                        break;
                    }
                } else {
                    // No parent, end of chain
                    break;
                }
            } else {
                // Session doesn't exist, stop
                break;
            }
        }

        Ok(chain)
    }

    async fn set_parent(
        &self,
        id: &IdeationSessionId,
        parent_id: Option<&IdeationSessionId>,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            session.parent_session_id = parent_id.cloned();
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn update_verification_state(
        &self,
        id: &IdeationSessionId,
        status: VerificationStatus,
        in_progress: bool,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            session.verification_status = status;
            session.verification_in_progress = in_progress;
            Self::clear_verification_summary(session);
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn reset_verification(&self, id: &IdeationSessionId) -> AppResult<bool> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            if session.verification_in_progress {
                return Ok(false);
            }
            // ImportedVerified sessions are never reset — their pre-verified status must be preserved.
            if session.verification_status == VerificationStatus::ImportedVerified {
                return Ok(false);
            }
            session.verification_status = VerificationStatus::Unverified;
            session.verification_in_progress = false;
            Self::clear_verification_summary(session);
            session.verification_generation += 1;
            session.updated_at = Utc::now();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn reset_and_begin_reverify(
        &self,
        session_id: &str,
    ) -> AppResult<(i32, VerificationRunSnapshot)> {
        let mut sessions = self.sessions.write().unwrap();
        let session = sessions.get_mut(session_id).ok_or_else(|| {
            crate::error::AppError::Database(format!("Session not found: {}", session_id))
        })?;

        let new_gen = session.verification_generation + 1;
        let snapshot = VerificationRunSnapshot {
            generation: new_gen,
            status: VerificationStatus::Reviewing,
            in_progress: true,
            current_round: 0,
            max_rounds: 0,
            best_round_index: None,
            convergence_reason: None,
            current_gaps: Vec::new(),
            rounds: Vec::new(),
        };

        session.verification_status = VerificationStatus::Reviewing;
        session.verification_in_progress = true;
        session.verification_generation = new_gen;
        Self::clear_verification_summary(session);
        session.updated_at = chrono::Utc::now();

        Ok((new_gen, snapshot))
    }

    async fn get_verification_status(
        &self,
        id: &IdeationSessionId,
    ) -> AppResult<Option<(VerificationStatus, bool)>> {
        let session = self.sessions.read().unwrap().get(&id.to_string()).cloned();
        let Some(session) = session else {
            return Ok(None);
        };

        let effective = self
            .verification_runs
            .read()
            .unwrap()
            .get(&(id.to_string(), session.verification_generation))
            .map(|snapshot| (snapshot.status, snapshot.in_progress))
            .unwrap_or((
                session.verification_status,
                session.verification_in_progress,
            ));

        Ok(Some(effective))
    }

    async fn save_verification_run_snapshot(
        &self,
        id: &IdeationSessionId,
        snapshot: &VerificationRunSnapshot,
    ) -> AppResult<()> {
        self.verification_runs
            .write()
            .unwrap()
            .insert((id.to_string(), snapshot.generation), snapshot.clone());
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            if session.verification_generation == snapshot.generation {
                Self::apply_verification_snapshot_summary(session, snapshot);
                session.verification_status = snapshot.status;
                session.verification_in_progress = snapshot.in_progress;
            }
        }
        Ok(())
    }

    async fn get_verification_run_snapshot(
        &self,
        id: &IdeationSessionId,
        generation: i32,
    ) -> AppResult<Option<VerificationRunSnapshot>> {
        Ok(self
            .verification_runs
            .read()
            .unwrap()
            .get(&(id.to_string(), generation))
            .cloned())
    }

    async fn revert_plan_and_skip_verification(
        &self,
        id: &IdeationSessionId,
        new_plan_artifact_id: String,
        convergence_reason: String,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(&id.to_string()) {
            session.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
                new_plan_artifact_id,
            ));
            session.verification_status = VerificationStatus::Skipped;
            session.verification_in_progress = false;
            session.verification_current_round = None;
            session.verification_max_rounds = None;
            session.verification_gap_count = 0;
            session.verification_gap_score = Some(0);
            session.verification_convergence_reason = Some(convergence_reason);
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn revert_plan_and_skip_with_artifact(
        &self,
        session_id: &IdeationSessionId,
        new_artifact_id: String,
        _artifact_type_str: String,
        _artifact_name: String,
        _content_text: String,
        _version: u32,
        _previous_version_id: String,
        convergence_reason: String,
    ) -> AppResult<()> {
        if let Some(session) = self
            .sessions
            .write()
            .unwrap()
            .get_mut(&session_id.to_string())
        {
            session.plan_artifact_id = Some(crate::domain::entities::ArtifactId::from_string(
                new_artifact_id,
            ));
            session.verification_status = VerificationStatus::Skipped;
            session.verification_in_progress = false;
            session.verification_current_round = None;
            session.verification_max_rounds = None;
            session.verification_gap_count = 0;
            session.verification_gap_score = Some(0);
            session.verification_convergence_reason = Some(convergence_reason);
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn increment_verification_generation(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<()> {
        if let Some(session) = self.sessions.write().unwrap().get_mut(session_id.as_str()) {
            session.verification_generation += 1;
            Self::clear_verification_summary(session);
        }
        Ok(())
    }

    async fn get_stale_in_progress_sessions(
        &self,
        stale_before: DateTime<Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| {
                s.verification_in_progress
                    && s.updated_at < stale_before
                    && s.status != IdeationSessionStatus::Archived
            })
            .cloned()
            .collect())
    }

    async fn get_all_in_progress_sessions(&self) -> AppResult<Vec<IdeationSession>> {
        Ok(self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| s.verification_in_progress && s.status != IdeationSessionStatus::Archived)
            .cloned()
            .collect())
    }

    async fn get_latest_verification_child(
        &self,
        parent_id: &IdeationSessionId,
    ) -> AppResult<Option<IdeationSession>> {
        use crate::domain::entities::ideation::SessionPurpose;
        let mut children: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| {
                s.parent_session_id.as_ref() == Some(parent_id)
                    && s.session_purpose == SessionPurpose::Verification
            })
            .cloned()
            .collect();
        children.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(children.into_iter().next())
    }

    async fn get_verification_children(
        &self,
        parent_session_id: &IdeationSessionId,
    ) -> AppResult<Vec<IdeationSession>> {
        use crate::domain::entities::ideation::SessionPurpose;
        let mut children: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| {
                s.parent_session_id.as_ref() == Some(parent_session_id)
                    && s.session_purpose == SessionPurpose::Verification
                    && s.status != IdeationSessionStatus::Archived
            })
            .cloned()
            .collect();
        children.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        children.truncate(1);
        Ok(children)
    }

    async fn get_by_project_and_status(
        &self,
        project_id: &str,
        status: &str,
        limit: u32,
    ) -> AppResult<Vec<IdeationSession>> {
        let mut sessions: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| s.project_id.as_str() == project_id && s.status.to_string() == status)
            .cloned()
            .collect();
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        sessions.truncate(limit as usize);
        Ok(sessions)
    }

    async fn get_group_counts(
        &self,
        project_id: &ProjectId,
        _search: Option<&str>,
    ) -> AppResult<SessionGroupCounts> {
        use crate::domain::entities::ideation::SessionPurpose;
        let sessions = self.sessions.read().unwrap();
        // Exclude verification child sessions from counts
        let project_sessions: Vec<_> = sessions
            .values()
            .filter(|s| {
                &s.project_id == project_id && s.session_purpose != SessionPurpose::Verification
            })
            .collect();

        let drafts = project_sessions
            .iter()
            .filter(|s| s.status == IdeationSessionStatus::Active)
            .count() as u32;
        let archived = project_sessions
            .iter()
            .filter(|s| s.status == IdeationSessionStatus::Archived)
            .count() as u32;
        // Simplified: memory repo can't classify in_progress/done sub-groups without task repo access.
        // All accepted sessions are counted under accepted.
        let accepted = project_sessions
            .iter()
            .filter(|s| s.status == IdeationSessionStatus::Accepted)
            .count() as u32;

        Ok(SessionGroupCounts {
            drafts,
            in_progress: 0,
            accepted,
            done: 0,
            archived,
        })
    }

    async fn list_by_group(
        &self,
        project_id: &ProjectId,
        group: &str,
        offset: u32,
        limit: u32,
        _search: Option<&str>,
    ) -> AppResult<(Vec<IdeationSessionWithProgress>, u32)> {
        // Validate group
        if !matches!(
            group,
            "drafts" | "in_progress" | "accepted" | "done" | "archived"
        ) {
            return Err(AppError::Validation(format!(
                "Unknown session group: '{}'. Valid groups: drafts, in_progress, accepted, done, archived",
                group
            )));
        }

        use crate::domain::entities::ideation::SessionPurpose;
        let sessions = self.sessions.read().unwrap();

        // Simplified classification: no task repo access, so in_progress/done always empty
        // Exclude verification child sessions from results
        let mut matching: Vec<_> = sessions
            .values()
            .filter(|s| {
                if s.project_id != *project_id {
                    return false;
                }
                if s.session_purpose == SessionPurpose::Verification {
                    return false;
                }
                match group {
                    "drafts" => s.status == IdeationSessionStatus::Active,
                    "archived" => s.status == IdeationSessionStatus::Archived,
                    "accepted" => s.status == IdeationSessionStatus::Accepted,
                    "in_progress" | "done" => false, // requires task repo — not available in memory repo
                    _ => false,
                }
            })
            .cloned()
            .collect();

        matching.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let total = matching.len() as u32;

        // Count verification children for each session
        let page: Vec<_> = matching
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|session| {
                let verification_child_count = sessions
                    .values()
                    .filter(|s| {
                        s.parent_session_id.as_ref() == Some(&session.id)
                            && s.session_purpose == SessionPurpose::Verification
                    })
                    .count() as u32;
                IdeationSessionWithProgress {
                    session,
                    progress: None,
                    parent_session_title: None,
                    verification_child_count,
                    has_pending_prompt: false,
                }
            })
            .collect();

        Ok((page, total))
    }

    fn set_expected_proposal_count_sync(
        _conn: &Connection,
        _session_id: &str,
        _count: u32,
    ) -> AppResult<()>
    where
        Self: Sized,
    {
        Err(AppError::Infrastructure(
            "set_expected_proposal_count_sync not supported in memory repo".to_string(),
        ))
    }

    async fn set_auto_accept_status(
        &self,
        session_id: &str,
        status: &str,
        _auto_accept_started_at: Option<String>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            session.auto_accept_status = Some(status.to_string());
        }
        Ok(())
    }

    fn count_active_by_session_sync(_conn: &Connection, _session_id: &str) -> AppResult<i64>
    where
        Self: Sized,
    {
        Err(AppError::Infrastructure(
            "count_active_by_session_sync not supported in memory repo".to_string(),
        ))
    }

    async fn count_active_proposals(&self, _session_id: &IdeationSessionId) -> AppResult<usize> {
        // Memory repo has no proposal storage; return 0 (not an error).
        Ok(0)
    }

    async fn get_by_idempotency_key(
        &self,
        api_key_id: &str,
        idempotency_key: &str,
    ) -> AppResult<Option<IdeationSession>> {
        let sessions = self.sessions.read().unwrap();
        Ok(sessions
            .values()
            .find(|s| {
                s.api_key_id.as_deref() == Some(api_key_id)
                    && s.idempotency_key.as_deref() == Some(idempotency_key)
            })
            .cloned())
    }

    async fn update_external_activity_phase(
        &self,
        id: &IdeationSessionId,
        phase: Option<&str>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(id.as_str()) {
            session.external_activity_phase = phase.map(|p| p.to_string());
            session.updated_at = chrono::Utc::now();
        }
        Ok(())
    }

    async fn update_external_last_read_message_id(
        &self,
        id: &IdeationSessionId,
        message_id: &str,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(id.as_str()) {
            session.external_last_read_message_id = Some(message_id.to_string());
            session.updated_at = chrono::Utc::now();
        }
        Ok(())
    }

    async fn list_active_external_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        let sessions = self.sessions.read().unwrap();
        let mut result: Vec<IdeationSession> = sessions
            .values()
            .filter(|s| {
                s.project_id == *project_id
                    && s.status == IdeationSessionStatus::Active
                    && s.origin == SessionOrigin::External
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result)
    }

    async fn list_active_external_sessions_for_archival(
        &self,
        stale_before: Option<DateTime<Utc>>,
    ) -> AppResult<Vec<IdeationSession>> {
        let sessions = self.sessions.read().unwrap();
        let mut result: Vec<IdeationSession> = sessions
            .values()
            .filter(|s| {
                if s.origin != SessionOrigin::External || s.status != IdeationSessionStatus::Active
                {
                    return false;
                }
                let phase_matches = matches!(
                    s.external_activity_phase.as_deref(),
                    Some("created") | Some("error")
                );
                if !phase_matches {
                    return false;
                }
                if let Some(cutoff) = stale_before {
                    s.updated_at < cutoff
                } else {
                    true
                }
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        Ok(result)
    }

    async fn list_stalled_external_sessions(
        &self,
        stalled_before: DateTime<Utc>,
    ) -> AppResult<Vec<IdeationSession>> {
        let sessions = self.sessions.read().unwrap();
        let mut result: Vec<IdeationSession> = sessions
            .values()
            .filter(|s| {
                if s.origin != SessionOrigin::External || s.status != IdeationSessionStatus::Active
                {
                    return false;
                }
                let phase_eligible = match s.external_activity_phase.as_deref() {
                    None => false,
                    Some("error") | Some("stalled") => false,
                    Some(_) => true,
                };
                phase_eligible && s.updated_at < stalled_before
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        Ok(result)
    }

    async fn set_dependencies_acknowledged(&self, session_id: &str) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            session.dependencies_acknowledged = true;
        }
        Ok(())
    }

    async fn reset_acceptance_cycle_fields(&self, session_id: &str) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            session.expected_proposal_count = None;
            session.dependencies_acknowledged = false;
            session.auto_accept_status = None;
            session.auto_accept_started_at = None;
            session.cross_project_checked = false;
        }
        Ok(())
    }

    async fn touch_updated_at(&self, session_id: &str) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            session.updated_at = chrono::Utc::now();
        }
        Ok(())
    }

    async fn update_last_effective_model(&self, session_id: &str, model: &str) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            session.last_effective_model = Some(model.to_string());
        }
        Ok(())
    }

    async fn list_active_verification_children(&self) -> AppResult<Vec<IdeationSession>> {
        use crate::domain::entities::ideation::SessionPurpose;
        let mut children: Vec<_> = self
            .sessions
            .read()
            .unwrap()
            .values()
            .filter(|s| {
                s.session_purpose == SessionPurpose::Verification
                    && s.status != IdeationSessionStatus::Archived
            })
            .cloned()
            .collect();
        children.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(children)
    }

    async fn set_pending_initial_prompt(
        &self,
        session_id: &str,
        prompt: Option<String>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            session.pending_initial_prompt = prompt;
            session.updated_at = chrono::Utc::now();
        }
        Ok(())
    }

    async fn set_pending_initial_prompt_if_unset(
        &self,
        session_id: &str,
        prompt: String,
    ) -> AppResult<bool> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
            if session.pending_initial_prompt.is_none() {
                session.pending_initial_prompt = Some(prompt);
                session.updated_at = chrono::Utc::now();
                return Ok(true);
            }
            return Ok(false);
        }
        Ok(false)
    }

    async fn claim_pending_session_for_project(
        &self,
        project_id: &str,
    ) -> AppResult<Option<(String, String)>> {
        let mut sessions = self.sessions.write().unwrap();
        // Find the oldest active session with a pending prompt for this project
        let candidate = sessions
            .values()
            .filter(|s| {
                s.project_id.as_str() == project_id
                    && s.status == IdeationSessionStatus::Active
                    && s.pending_initial_prompt.is_some()
            })
            .min_by_key(|s| s.created_at)
            .map(|s| {
                (
                    s.id.as_str().to_string(),
                    s.pending_initial_prompt.clone().unwrap(),
                )
            });

        match candidate {
            None => Ok(None),
            Some((session_id, prompt)) => {
                if let Some(session) = sessions.values_mut().find(|s| s.id.as_str() == session_id) {
                    session.pending_initial_prompt = None;
                    session.updated_at = chrono::Utc::now();
                }
                Ok(Some((session_id, prompt)))
            }
        }
    }

    async fn list_projects_with_pending_sessions(&self) -> AppResult<Vec<String>> {
        let sessions = self.sessions.read().unwrap();
        let mut project_ids: Vec<String> = sessions
            .values()
            .filter(|s| {
                s.status == IdeationSessionStatus::Active && s.pending_initial_prompt.is_some()
            })
            .map(|s| s.project_id.as_str().to_string())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        project_ids.sort();
        Ok(project_ids)
    }

    async fn count_pending_sessions_for_project(&self, project_id: &ProjectId) -> AppResult<u32> {
        let sessions = self.sessions.read().unwrap();
        let count = sessions
            .values()
            .filter(|s| {
                s.project_id == *project_id
                    && s.status == IdeationSessionStatus::Active
                    && s.pending_initial_prompt.is_some()
            })
            .count() as u32;
        Ok(count)
    }

    async fn update_acceptance_status(
        &self,
        session_id: &IdeationSessionId,
        expected_current: Option<AcceptanceStatus>,
        new_status: Option<AcceptanceStatus>,
    ) -> AppResult<bool> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(session_id.as_str()) {
            if session.acceptance_status == expected_current {
                session.acceptance_status = new_status;
                session.updated_at = Utc::now();
                return Ok(true);
            }
            return Ok(false);
        }
        Err(AppError::NotFound(format!(
            "Session {} not found",
            session_id.as_str()
        )))
    }

    async fn get_sessions_with_pending_acceptance(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        let sessions = self.sessions.read().unwrap();
        Ok(sessions
            .values()
            .filter(|s| {
                s.project_id.to_string() == project_id.to_string()
                    && s.status == IdeationSessionStatus::Active
                    && s.acceptance_status == Some(AcceptanceStatus::Pending)
            })
            .cloned()
            .collect())
    }

    async fn set_verification_confirmation_status(
        &self,
        session_id: &IdeationSessionId,
        status: Option<VerificationConfirmationStatus>,
    ) -> AppResult<()> {
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(session_id.as_str()) {
            session.verification_confirmation_status = status;
            session.updated_at = Utc::now();
        }
        Ok(())
    }

    async fn get_pending_verification_confirmations(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        let sessions = self.sessions.read().unwrap();
        Ok(sessions
            .values()
            .filter(|s| {
                s.project_id.to_string() == project_id.to_string()
                    && s.verification_confirmation_status
                        == Some(VerificationConfirmationStatus::Pending)
            })
            .cloned()
            .collect())
    }
}

#[cfg(test)]
#[path = "memory_ideation_session_repo_tests.rs"]
mod tests;
