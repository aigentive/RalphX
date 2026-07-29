// IdeationService
// Application service for orchestrating ideation workflow
//
// This service manages the ideation flow:
// - Creating and managing ideation sessions
// - Creating and updating task proposals
// - Adding chat messages
// - Retrieving session data with proposals and messages

mod types;

pub use types::{
    CreateProposalOptions, PlanArtifactConfig, SessionStats, SessionWithData, UpdateProposalOptions,
    UpdateSource,
};

use crate::domain::entities::{
    ChatMessage, IdeationSession, IdeationSessionId, IdeationSessionStatus, MethodologyExtension,
    ProjectId, ProposalStatus, TaskProposal, TaskProposalId,
};
use crate::domain::repositories::{
    ChatMessageRepository, IdeationSessionRepository, ProposalDependencyRepository,
    TaskProposalRepository,
};
use crate::error::{AppError, AppResult};
use chrono::Utc;
use std::sync::Arc;

pub(crate) fn build_default_ideation_session_title() -> String {
    format!("Ideation Session {}", Utc::now().format("%Y-%m-%d %H:%M"))
}

/// Service for orchestrating ideation workflow
pub struct IdeationService<
    S: IdeationSessionRepository,
    P: TaskProposalRepository,
    M: ChatMessageRepository,
    D: ProposalDependencyRepository,
> {
    /// Repository for ideation sessions
    session_repo: Arc<S>,
    /// Repository for task proposals
    proposal_repo: Arc<P>,
    /// Repository for chat messages
    message_repo: Arc<M>,
    /// Repository for proposal dependencies
    dependency_repo: Arc<D>,
}

impl<S, P, M, D> IdeationService<S, P, M, D>
where
    S: IdeationSessionRepository,
    P: TaskProposalRepository,
    M: ChatMessageRepository,
    D: ProposalDependencyRepository,
{
    /// Create a new ideation service
    pub fn new(
        session_repo: Arc<S>,
        proposal_repo: Arc<P>,
        message_repo: Arc<M>,
        dependency_repo: Arc<D>,
    ) -> Self {
        Self {
            session_repo,
            proposal_repo,
            message_repo,
            dependency_repo,
        }
    }

    #[doc(hidden)]
    pub fn session_repo_for_test(&self) -> &Arc<S> {
        &self.session_repo
    }

    /// Create a new ideation session with optional auto-generated title
    pub async fn create_session(
        &self,
        project_id: ProjectId,
        title: Option<String>,
    ) -> AppResult<IdeationSession> {
        let session = match title {
            Some(t) => IdeationSession::new_with_title(project_id, t),
            None => IdeationSession::new_with_title(project_id, build_default_ideation_session_title()),
        };

        self.session_repo.create(session).await
    }

    /// Get a session by ID
    pub async fn get_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<IdeationSession>> {
        self.session_repo.get_by_id(session_id).await
    }

    /// Get a session with all its proposals and messages
    pub async fn get_session_with_data(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<SessionWithData>> {
        let session = self.session_repo.get_by_id(session_id).await?;

        match session {
            Some(session) => {
                let proposals = self.proposal_repo.get_by_session(session_id).await?;
                let messages = self.message_repo.get_by_session(session_id).await?;

                Ok(Some(SessionWithData {
                    session,
                    proposals,
                    messages,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get all sessions for a project
    pub async fn get_sessions_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        self.session_repo.get_by_project(project_id).await
    }

    /// Get active sessions for a project
    pub async fn get_active_sessions(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<IdeationSession>> {
        self.session_repo.get_active_by_project(project_id).await
    }

    /// Archive a session
    pub async fn archive_session(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        self.session_repo
            .update_status(session_id, IdeationSessionStatus::Archived)
            .await
    }

    /// Update session title and source ("auto" for ralphx-utility-session-namer, "user" for manual rename)
    pub async fn update_session_title(
        &self,
        session_id: &IdeationSessionId,
        title: Option<String>,
        title_source: &str,
    ) -> AppResult<()> {
        self.session_repo
            .update_title(session_id, title, title_source)
            .await
    }

    /// Delete a session and all its data
    pub async fn delete_session(&self, session_id: &IdeationSessionId) -> AppResult<()> {
        self.session_repo.delete(session_id).await
    }

    /// Create a new task proposal in a session
    pub async fn create_proposal(
        &self,
        session_id: IdeationSessionId,
        options: CreateProposalOptions,
    ) -> AppResult<TaskProposal> {
        // Verify session exists and is active
        let session = self.session_repo.get_by_id(&session_id).await?;
        match session {
            None => {
                return Err(AppError::NotFound(format!(
                    "Session {} not found",
                    session_id
                )))
            }
            Some(s) if s.status != IdeationSessionStatus::Active => {
                return Err(AppError::Validation(format!(
                    "Cannot add proposal to {} session",
                    s.status
                )));
            }
            _ => {}
        }

        // Get current proposal count for sort_order
        let count = self.proposal_repo.count_by_session(&session_id).await?;

        let mut proposal = TaskProposal::new(
            session_id,
            options.title,
            options.category,
            options.suggested_priority,
        );
        proposal.description = options.description;
        proposal.steps = options.steps;
        proposal.acceptance_criteria = options.acceptance_criteria;
        proposal.sort_order = count as i32;

        self.proposal_repo.create(proposal).await
    }

    /// Update an existing proposal
    pub async fn update_proposal(
        &self,
        proposal_id: &TaskProposalId,
        options: UpdateProposalOptions,
    ) -> AppResult<TaskProposal> {
        let proposal = self.proposal_repo.get_by_id(proposal_id).await?;

        match proposal {
            None => Err(AppError::NotFound(format!(
                "Proposal {} not found",
                proposal_id
            ))),
            Some(mut proposal) => {
                let mut modified = false;

                if let Some(title) = options.title {
                    proposal.title = title;
                    modified = true;
                }

                if let Some(description) = options.description {
                    proposal.description = description;
                    modified = true;
                }

                if let Some(category) = options.category {
                    proposal.category = category;
                    modified = true;
                }

                if let Some(steps) = options.steps {
                    proposal.steps = steps;
                    modified = true;
                }

                if let Some(acceptance_criteria) = options.acceptance_criteria {
                    proposal.acceptance_criteria = acceptance_criteria;
                    modified = true;
                }

                if let Some(user_priority) = options.user_priority {
                    proposal.user_priority = Some(user_priority);
                    proposal.status = ProposalStatus::Modified;
                    modified = true;
                }

                if modified {
                    proposal.user_modified = true;
                    proposal.touch();
                    self.proposal_repo.update(&proposal).await?;
                }

                Ok(proposal)
            }
        }
    }

    /// Archive a proposal (soft-delete)
    pub async fn archive_proposal(&self, proposal_id: &TaskProposalId) -> AppResult<()> {
        // First clear any dependencies
        self.dependency_repo.clear_dependencies(proposal_id).await?;
        // Then archive the proposal
        self.proposal_repo.archive(proposal_id).await.map(|_| ())
    }

    /// Toggle proposal selection state
    pub async fn toggle_proposal_selection(&self, proposal_id: &TaskProposalId) -> AppResult<bool> {
        let proposal = self.proposal_repo.get_by_id(proposal_id).await?;

        match proposal {
            None => Err(AppError::NotFound(format!(
                "Proposal {} not found",
                proposal_id
            ))),
            Some(proposal) => {
                let new_selected = !proposal.selected;
                self.proposal_repo
                    .update_selection(proposal_id, new_selected)
                    .await?;
                Ok(new_selected)
            }
        }
    }

    /// Set proposal selection state
    pub async fn set_proposal_selection(
        &self,
        proposal_id: &TaskProposalId,
        selected: bool,
    ) -> AppResult<()> {
        self.proposal_repo
            .update_selection(proposal_id, selected)
            .await
    }

    /// Get proposals for a session
    pub async fn get_proposals(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        self.proposal_repo.get_by_session(session_id).await
    }

    /// Get selected proposals for a session
    pub async fn get_selected_proposals(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<TaskProposal>> {
        self.proposal_repo.get_selected_by_session(session_id).await
    }

    /// Add a user message to a session
    pub async fn add_user_message(
        &self,
        session_id: IdeationSessionId,
        content: impl Into<String>,
    ) -> AppResult<ChatMessage> {
        let message = ChatMessage::user_in_session(session_id, content);
        self.message_repo.create(message).await
    }

    /// Add an orchestrator message to a session
    pub async fn add_orchestrator_message(
        &self,
        session_id: IdeationSessionId,
        content: impl Into<String>,
    ) -> AppResult<ChatMessage> {
        let message = ChatMessage::orchestrator_in_session(session_id, content);
        self.message_repo.create(message).await
    }

    /// Add a system message to a session
    pub async fn add_system_message(
        &self,
        session_id: IdeationSessionId,
        content: impl Into<String>,
    ) -> AppResult<ChatMessage> {
        let message = ChatMessage::system_in_session(session_id, content);
        self.message_repo.create(message).await
    }

    /// Get all messages for a session
    pub async fn get_session_messages(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<ChatMessage>> {
        self.message_repo.get_by_session(session_id).await
    }

    /// Get recent messages for a session
    pub async fn get_recent_messages(
        &self,
        session_id: &IdeationSessionId,
        limit: u32,
    ) -> AppResult<Vec<ChatMessage>> {
        self.message_repo
            .get_recent_by_session(session_id, limit)
            .await
    }

    /// Reorder proposals within a session
    pub async fn reorder_proposals(
        &self,
        session_id: &IdeationSessionId,
        proposal_ids: Vec<TaskProposalId>,
    ) -> AppResult<()> {
        self.proposal_repo.reorder(session_id, proposal_ids).await
    }

    /// Select all proposals in a session
    pub async fn select_all_proposals(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        let proposals = self.proposal_repo.get_by_session(session_id).await?;
        let mut count = 0;

        for proposal in proposals {
            if !proposal.selected {
                self.proposal_repo
                    .update_selection(&proposal.id, true)
                    .await?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Deselect all proposals in a session
    pub async fn deselect_all_proposals(&self, session_id: &IdeationSessionId) -> AppResult<u32> {
        let proposals = self.proposal_repo.get_by_session(session_id).await?;
        let mut count = 0;

        for proposal in proposals {
            if proposal.selected {
                self.proposal_repo
                    .update_selection(&proposal.id, false)
                    .await?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Get session statistics
    pub async fn get_session_stats(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<SessionStats> {
        let total_proposals = self.proposal_repo.count_by_session(session_id).await?;
        let selected_proposals = self
            .proposal_repo
            .count_selected_by_session(session_id)
            .await?;
        let message_count = self.message_repo.count_by_session(session_id).await?;

        Ok(SessionStats {
            total_proposals,
            selected_proposals,
            message_count,
        })
    }

    /// Get plan artifact configuration
    ///
    /// Returns the artifact type and bucket to use for ideation plans.
    /// If a methodology is active and provides custom configuration, that is used.
    /// Otherwise, returns default configuration (Specification type, prd-library bucket).
    ///
    /// This is infrastructure for future methodology integration - currently always returns defaults.
    pub fn get_plan_artifact_config(
        active_methodology: Option<&MethodologyExtension>,
    ) -> PlanArtifactConfig {
        match active_methodology.and_then(|m| m.plan_artifact_config.as_ref()) {
            Some(config) => PlanArtifactConfig {
                artifact_type: config.artifact_type.clone(),
                bucket_id: config.bucket_id.clone(),
            },
            None => PlanArtifactConfig {
                artifact_type: "specification".to_string(),
                bucket_id: "prd-library".to_string(),
            },
        }
    }
}
