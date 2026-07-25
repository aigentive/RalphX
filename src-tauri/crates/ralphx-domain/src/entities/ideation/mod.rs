//! Ideation system entities - brainstorming sessions and task proposals
//!
//! This module contains:
//! - IdeationSession: the main session entity
//! - TaskProposal: task proposals generated during ideation
//! - ChatMessage: messages in ideation conversations
//! - Priority assessment system with detailed factors
//! - Dependency graph for tracking proposal relationships

mod assessment;
mod chat;
mod child_session;
mod graph;
mod proposal;
pub mod session_context;
pub mod session_link;
mod types;

#[cfg(test)]
mod tests;

// Re-export public types
pub use assessment::*;
pub use chat::*;
pub use child_session::*;
pub use graph::*;
pub use proposal::TaskProposal;
pub use session_context::*;
pub use session_link::*;
pub use types::*;

use chrono::{DateTime, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

use crate::entities::{ArtifactId, IdeationSessionId, ProjectId, TaskId};

pub const PLAN_CONTRACT_V1: i32 = 1;
pub const PLAN_CONTRACT_V2: i32 = 2;

const fn default_plan_contract_version() -> i32 {
    PLAN_CONTRACT_V2
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanArtifactBundle {
    pub overview_id: ArtifactId,
    pub blueprint_id: Option<ArtifactId>,
    pub contract_version: i32,
    pub is_inherited: bool,
}

impl PlanArtifactBundle {
    pub fn action_target_id(&self) -> String {
        match &self.blueprint_id {
            Some(blueprint_id) if self.contract_version >= PLAN_CONTRACT_V2 => format!(
                "plan_bundle:v2:{}:{}",
                self.overview_id.as_str(),
                blueprint_id.as_str()
            ),
            _ => self.overview_id.to_string(),
        }
    }
}

/// An ideation session - a brainstorming conversation that produces task proposals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdeationSession {
    /// Unique identifier for this session
    pub id: IdeationSessionId,
    /// Project this session belongs to
    pub project_id: ProjectId,
    /// Human-readable title (auto-generated or user-defined)
    pub title: Option<String>,
    /// Current status of the session
    pub status: IdeationSessionStatus,
    /// The implementation plan artifact for this session (owned by this session)
    pub plan_artifact_id: Option<ArtifactId>,
    /// Detailed implementation blueprint paired with the owned plan overview.
    #[serde(default)]
    pub plan_blueprint_artifact_id: Option<ArtifactId>,
    /// Exact linked plan artifact version most recently proven implementation-ready.
    #[serde(default)]
    pub verified_plan_artifact_id: Option<ArtifactId>,
    /// Exact blueprint version most recently proven with the current overview.
    #[serde(default)]
    pub verified_plan_blueprint_artifact_id: Option<ArtifactId>,
    /// Agent run whose bounded completion operation recorded the current proof.
    #[serde(default)]
    pub verified_plan_agent_run_id: Option<String>,
    /// Plan artifact inherited from parent session (read-only; child cannot modify)
    pub inherited_plan_artifact_id: Option<ArtifactId>,
    /// Blueprint inherited with the read-only overview.
    #[serde(default)]
    pub inherited_plan_blueprint_artifact_id: Option<ArtifactId>,
    /// v1 is grandfathered overview-only; v2 requires an exact overview/blueprint pair.
    #[serde(default = "default_plan_contract_version")]
    pub plan_contract_version: i32,
    /// Optional reference to a draft task that seeded this session
    pub seed_task_id: Option<TaskId>,
    /// Optional parent session for session linking (follow-on work, etc.)
    pub parent_session_id: Option<IdeationSessionId>,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// When the session was last updated
    pub updated_at: DateTime<Utc>,
    /// When the session was archived (if applicable)
    pub archived_at: Option<DateTime<Utc>>,
    /// When all proposals were converted to tasks (if applicable)
    pub converted_at: Option<DateTime<Utc>>,
    /// Title source: "auto" (ralphx-utility-session-namer) | "user" (manual rename). None treated as "auto".
    pub title_source: Option<String>,
    /// Verification status of this session's plan
    #[serde(default)]
    pub verification_status: VerificationStatus,
    /// Whether a verification loop is currently active
    #[serde(default)]
    pub verification_in_progress: bool,
    /// Generation counter for zombie protection (incremented on each auto-verify trigger)
    #[serde(default)]
    pub verification_generation: i32,
    /// Denormalized current round for the active/latest verification generation.
    pub verification_current_round: Option<u32>,
    /// Denormalized max round budget for the active/latest verification generation.
    pub verification_max_rounds: Option<u32>,
    /// Denormalized unresolved gap count for the active/latest verification generation.
    #[serde(default)]
    pub verification_gap_count: u32,
    /// Denormalized unresolved gap score for the active/latest verification generation.
    pub verification_gap_score: Option<u32>,
    /// Denormalized convergence reason for the active/latest verification generation.
    pub verification_convergence_reason: Option<String>,
    /// Source project ID when this session was imported from another project
    pub source_project_id: Option<String>,
    /// Source session ID when this session was imported from another project
    pub source_session_id: Option<String>,
    /// Source task ID when this session was spawned from execution/review/merge work
    pub source_task_id: Option<TaskId>,
    /// Originating non-ideation context type (task_execution, review, merge, research, etc.)
    pub source_context_type: Option<String>,
    /// Originating non-ideation context ID
    pub source_context_id: Option<String>,
    /// Reason this session was spawned (out_of_scope_failure, review_followup, etc.)
    pub spawn_reason: Option<String>,
    /// Stable dedupe key for a specific blocker carried by this follow-up session.
    pub blocker_fingerprint: Option<String>,
    /// Purpose of this session: General (default) or Verification (ralphx-plan-verifier child)
    #[serde(default)]
    pub session_purpose: SessionPurpose,
    /// User-facing flow for this ideation-family session.
    #[serde(default)]
    pub session_flow: IdeationSessionFlow,
    /// Whether cross_project_guide has been called on this session's plan.
    /// False = proposal creation is blocked until cross_project_guide sets it.
    /// Default false for new sessions; existing DB rows default to true via migration.
    #[serde(default)]
    pub cross_project_checked: bool,
    /// The plan artifact version that was last read by the agent for this session.
    /// Used by the stale plan guard to detect if the agent's in-memory plan is outdated.
    /// None = agent has not read the plan yet (or pre-v75 row).
    pub plan_version_last_read: Option<i32>,
    /// Blueprint version last acknowledged by the planning agent.
    pub blueprint_version_last_read: Option<i32>,
    /// Origin of this session: Internal (default) or External (created via External MCP API).
    /// External sessions cannot skip plan verification.
    #[serde(default)]
    pub origin: SessionOrigin,
    /// Expected number of proposals for auto-accept gating. None = no expectation set (gating disabled).
    pub expected_proposal_count: Option<u32>,
    /// Status of the auto-accept pipeline: null/pending/success/failed
    pub auto_accept_status: Option<String>,
    /// ISO timestamp when auto-accept was triggered
    pub auto_accept_started_at: Option<String>,
    /// API key that created this external session (NULL for internal sessions)
    pub api_key_id: Option<String>,
    /// Client-provided idempotency key for safe retries (NULL if not provided)
    pub idempotency_key: Option<String>,
    /// External session lifecycle phase (NULL for internal sessions)
    /// Values: "created" | "planning" | "proposing" | "verifying" | "ready" | "error" | "stalled"
    pub external_activity_phase: Option<String>,
    /// Last message ID the external agent fetched (NULL = never read)
    pub external_last_read_message_id: Option<String>,
    /// Whether the agent has explicitly acknowledged cross-proposal dependencies.
    /// False = finalize_proposals will block if inter-proposal dependencies exist.
    #[serde(default)]
    pub dependencies_acknowledged: bool,
    /// Initial prompt to auto-launch when capacity becomes available.
    /// Set when spawn_child_orchestration fails due to ideation capacity limits.
    /// Cleared to NULL by the drain service after successful launch.
    pub pending_initial_prompt: Option<String>,
    /// Acceptance status for the finalize confirmation gate.
    /// None = gate not triggered. Some(Pending) = awaiting user confirmation.
    /// Some(Accepted) = user accepted. Some(Rejected) = user rejected.
    pub acceptance_status: Option<AcceptanceStatus>,
    /// Verification confirmation status for the post-verification user confirmation gate.
    /// None = gate not triggered. Some(Pending) = awaiting user confirmation.
    /// Some(Accepted) = user confirmed the verified plan. Some(Rejected) = user rejected.
    pub verification_confirmation_status: Option<VerificationConfirmationStatus>,
    /// Session-scoped analysis base and workspace used by ideation-family agents.
    #[serde(default)]
    pub analysis: IdeationAnalysisState,
    /// The last effective Claude model ID used when spawning an agent for this session.
    /// Set after each successful agent spawn. Used to display the model label in the UI.
    pub last_effective_model: Option<String>,
}

/// Builder for creating IdeationSession instances
#[derive(Debug, Default)]
pub struct IdeationSessionBuilder {
    id: Option<IdeationSessionId>,
    project_id: Option<ProjectId>,
    title: Option<String>,
    status: Option<IdeationSessionStatus>,
    plan_artifact_id: Option<ArtifactId>,
    plan_blueprint_artifact_id: Option<ArtifactId>,
    verified_plan_artifact_id: Option<ArtifactId>,
    verified_plan_blueprint_artifact_id: Option<ArtifactId>,
    verified_plan_agent_run_id: Option<String>,
    inherited_plan_artifact_id: Option<ArtifactId>,
    inherited_plan_blueprint_artifact_id: Option<ArtifactId>,
    plan_contract_version: Option<i32>,
    seed_task_id: Option<TaskId>,
    parent_session_id: Option<IdeationSessionId>,
    created_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
    archived_at: Option<DateTime<Utc>>,
    converted_at: Option<DateTime<Utc>>,
    title_source: Option<String>,
    verification_status: Option<VerificationStatus>,
    verification_in_progress: Option<bool>,
    verification_generation: Option<i32>,
    verification_current_round: Option<u32>,
    verification_max_rounds: Option<u32>,
    verification_gap_count: Option<u32>,
    verification_gap_score: Option<u32>,
    verification_convergence_reason: Option<String>,
    source_project_id: Option<String>,
    source_session_id: Option<String>,
    source_task_id: Option<TaskId>,
    source_context_type: Option<String>,
    source_context_id: Option<String>,
    spawn_reason: Option<String>,
    blocker_fingerprint: Option<String>,
    session_purpose: Option<SessionPurpose>,
    session_flow: Option<IdeationSessionFlow>,
    cross_project_checked: Option<bool>,
    plan_version_last_read: Option<i32>,
    blueprint_version_last_read: Option<i32>,
    origin: Option<SessionOrigin>,
    expected_proposal_count: Option<u32>,
    auto_accept_status: Option<String>,
    auto_accept_started_at: Option<String>,
    api_key_id: Option<String>,
    idempotency_key: Option<String>,
    external_activity_phase: Option<String>,
    external_last_read_message_id: Option<String>,
    dependencies_acknowledged: Option<bool>,
    pending_initial_prompt: Option<String>,
    acceptance_status: Option<AcceptanceStatus>,
    verification_confirmation_status: Option<VerificationConfirmationStatus>,
    analysis: Option<IdeationAnalysisState>,
}

impl IdeationSessionBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the session ID
    pub fn id(mut self, id: IdeationSessionId) -> Self {
        self.id = Some(id);
        self
    }

    /// Set the project ID
    pub fn project_id(mut self, project_id: ProjectId) -> Self {
        self.project_id = Some(project_id);
        self
    }

    /// Set the title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the status
    pub fn status(mut self, status: IdeationSessionStatus) -> Self {
        self.status = Some(status);
        self
    }

    /// Set the plan artifact ID
    pub fn plan_artifact_id(mut self, plan_artifact_id: ArtifactId) -> Self {
        self.plan_artifact_id = Some(plan_artifact_id);
        self
    }

    pub fn plan_blueprint_artifact_id(mut self, artifact_id: ArtifactId) -> Self {
        self.plan_blueprint_artifact_id = Some(artifact_id);
        self
    }

    pub fn verified_plan_artifact_id(mut self, artifact_id: ArtifactId) -> Self {
        self.verified_plan_artifact_id = Some(artifact_id);
        self
    }

    pub fn verified_plan_blueprint_artifact_id(mut self, artifact_id: ArtifactId) -> Self {
        self.verified_plan_blueprint_artifact_id = Some(artifact_id);
        self
    }

    /// Set the inherited plan artifact ID (read-only, from parent session)
    pub fn inherited_plan_artifact_id(mut self, inherited_plan_artifact_id: ArtifactId) -> Self {
        self.inherited_plan_artifact_id = Some(inherited_plan_artifact_id);
        self
    }

    pub fn inherited_plan_blueprint_artifact_id(mut self, artifact_id: ArtifactId) -> Self {
        self.inherited_plan_blueprint_artifact_id = Some(artifact_id);
        self
    }

    pub fn plan_contract_version(mut self, version: i32) -> Self {
        self.plan_contract_version = Some(version);
        self
    }

    /// Set the seed task ID
    pub fn seed_task_id(mut self, seed_task_id: TaskId) -> Self {
        self.seed_task_id = Some(seed_task_id);
        self
    }

    /// Set the parent session ID
    pub fn parent_session_id(mut self, parent_session_id: IdeationSessionId) -> Self {
        self.parent_session_id = Some(parent_session_id);
        self
    }

    /// Set the created_at timestamp
    pub fn created_at(mut self, created_at: DateTime<Utc>) -> Self {
        self.created_at = Some(created_at);
        self
    }

    /// Set the updated_at timestamp
    pub fn updated_at(mut self, updated_at: DateTime<Utc>) -> Self {
        self.updated_at = Some(updated_at);
        self
    }

    /// Set the archived_at timestamp
    pub fn archived_at(mut self, archived_at: DateTime<Utc>) -> Self {
        self.archived_at = Some(archived_at);
        self
    }

    /// Set the converted_at timestamp
    pub fn converted_at(mut self, converted_at: DateTime<Utc>) -> Self {
        self.converted_at = Some(converted_at);
        self
    }

    /// Set the title source
    pub fn title_source(mut self, title_source: impl Into<String>) -> Self {
        self.title_source = Some(title_source.into());
        self
    }

    /// Set the verification status
    pub fn verification_status(mut self, verification_status: VerificationStatus) -> Self {
        self.verification_status = Some(verification_status);
        self
    }

    /// Set the verification generation counter
    pub fn verification_generation(mut self, generation: i32) -> Self {
        self.verification_generation = Some(generation);
        self
    }

    /// Set the source project ID (cross-project import provenance)
    pub fn source_project_id(mut self, source_project_id: impl Into<String>) -> Self {
        self.source_project_id = Some(source_project_id.into());
        self
    }

    /// Set the source session ID (cross-project import provenance)
    pub fn source_session_id(mut self, source_session_id: impl Into<String>) -> Self {
        self.source_session_id = Some(source_session_id.into());
        self
    }

    /// Set the source task ID
    pub fn source_task_id(mut self, source_task_id: TaskId) -> Self {
        self.source_task_id = Some(source_task_id);
        self
    }

    /// Set the source context type
    pub fn source_context_type(mut self, source_context_type: impl Into<String>) -> Self {
        self.source_context_type = Some(source_context_type.into());
        self
    }

    /// Set the source context ID
    pub fn source_context_id(mut self, source_context_id: impl Into<String>) -> Self {
        self.source_context_id = Some(source_context_id.into());
        self
    }

    /// Set the spawn reason
    pub fn spawn_reason(mut self, spawn_reason: impl Into<String>) -> Self {
        self.spawn_reason = Some(spawn_reason.into());
        self
    }

    /// Set the blocker fingerprint
    pub fn blocker_fingerprint(mut self, blocker_fingerprint: impl Into<String>) -> Self {
        self.blocker_fingerprint = Some(blocker_fingerprint.into());
        self
    }

    /// Set the session purpose
    pub fn session_purpose(mut self, session_purpose: SessionPurpose) -> Self {
        self.session_purpose = Some(session_purpose);
        self
    }

    /// Set the user-facing session flow
    pub fn session_flow(mut self, session_flow: IdeationSessionFlow) -> Self {
        self.session_flow = Some(session_flow);
        self
    }

    /// Set whether cross-project guide has been called
    pub fn cross_project_checked(mut self, checked: bool) -> Self {
        self.cross_project_checked = Some(checked);
        self
    }

    /// Set the plan version last read by the agent
    pub fn plan_version_last_read(mut self, version: i32) -> Self {
        self.plan_version_last_read = Some(version);
        self
    }

    pub fn blueprint_version_last_read(mut self, version: i32) -> Self {
        self.blueprint_version_last_read = Some(version);
        self
    }

    /// Set the session origin (Internal or External)
    pub fn origin(mut self, origin: SessionOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// Set the expected proposal count for auto-accept gating
    pub fn expected_proposal_count(mut self, count: u32) -> Self {
        self.expected_proposal_count = Some(count);
        self
    }

    /// Set the API key ID (for external sessions)
    pub fn api_key_id(mut self, api_key_id: impl Into<String>) -> Self {
        self.api_key_id = Some(api_key_id.into());
        self
    }

    /// Set the idempotency key (for safe retries)
    pub fn idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Set the external activity phase
    pub fn external_activity_phase(mut self, phase: impl Into<String>) -> Self {
        self.external_activity_phase = Some(phase.into());
        self
    }

    /// Set the external last read message ID
    pub fn external_last_read_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.external_last_read_message_id = Some(message_id.into());
        self
    }

    /// Set whether dependencies have been acknowledged
    pub fn dependencies_acknowledged(mut self, acknowledged: bool) -> Self {
        self.dependencies_acknowledged = Some(acknowledged);
        self
    }

    /// Set the pending initial prompt for deferred session launch
    pub fn pending_initial_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.pending_initial_prompt = Some(prompt.into());
        self
    }

    /// Set the acceptance status for the finalize confirmation gate
    pub fn acceptance_status(mut self, status: AcceptanceStatus) -> Self {
        self.acceptance_status = Some(status);
        self
    }

    /// Set the verification confirmation status for the post-verification user confirmation gate
    pub fn verification_confirmation_status(
        mut self,
        status: VerificationConfirmationStatus,
    ) -> Self {
        self.verification_confirmation_status = Some(status);
        self
    }

    /// Set the session-scoped analysis base/workspace metadata.
    pub fn analysis(mut self, analysis: IdeationAnalysisState) -> Self {
        self.analysis = Some(analysis);
        self
    }

    /// Build the IdeationSession
    /// Panics if project_id is not set
    pub fn build(self) -> IdeationSession {
        let now = Utc::now();
        IdeationSession {
            id: self.id.unwrap_or_default(),
            project_id: self.project_id.expect("project_id is required"),
            title: self.title,
            status: self.status.unwrap_or_default(),
            plan_artifact_id: self.plan_artifact_id,
            plan_blueprint_artifact_id: self.plan_blueprint_artifact_id,
            verified_plan_artifact_id: self.verified_plan_artifact_id,
            verified_plan_blueprint_artifact_id: self.verified_plan_blueprint_artifact_id,
            verified_plan_agent_run_id: self.verified_plan_agent_run_id,
            inherited_plan_artifact_id: self.inherited_plan_artifact_id,
            inherited_plan_blueprint_artifact_id: self.inherited_plan_blueprint_artifact_id,
            plan_contract_version: self.plan_contract_version.unwrap_or(PLAN_CONTRACT_V2),
            seed_task_id: self.seed_task_id,
            parent_session_id: self.parent_session_id,
            created_at: self.created_at.unwrap_or(now),
            updated_at: self.updated_at.unwrap_or(now),
            archived_at: self.archived_at,
            converted_at: self.converted_at,
            title_source: self.title_source,
            verification_status: self.verification_status.unwrap_or_default(),
            verification_in_progress: self.verification_in_progress.unwrap_or(false),
            verification_generation: self.verification_generation.unwrap_or(0),
            verification_current_round: self.verification_current_round,
            verification_max_rounds: self.verification_max_rounds,
            verification_gap_count: self.verification_gap_count.unwrap_or(0),
            verification_gap_score: self.verification_gap_score,
            verification_convergence_reason: self.verification_convergence_reason,
            source_project_id: self.source_project_id,
            source_session_id: self.source_session_id,
            source_task_id: self.source_task_id,
            source_context_type: self.source_context_type,
            source_context_id: self.source_context_id,
            spawn_reason: self.spawn_reason,
            blocker_fingerprint: self.blocker_fingerprint,
            session_purpose: self.session_purpose.unwrap_or_default(),
            session_flow: self.session_flow.unwrap_or_default(),
            cross_project_checked: self.cross_project_checked.unwrap_or(false),
            plan_version_last_read: self.plan_version_last_read,
            blueprint_version_last_read: self.blueprint_version_last_read,
            origin: self.origin.unwrap_or_default(),
            expected_proposal_count: self.expected_proposal_count,
            auto_accept_status: self.auto_accept_status,
            auto_accept_started_at: self.auto_accept_started_at,
            api_key_id: self.api_key_id,
            idempotency_key: self.idempotency_key,
            external_activity_phase: self.external_activity_phase,
            external_last_read_message_id: self.external_last_read_message_id,
            dependencies_acknowledged: self.dependencies_acknowledged.unwrap_or(false),
            pending_initial_prompt: self.pending_initial_prompt,
            acceptance_status: self.acceptance_status,
            verification_confirmation_status: self.verification_confirmation_status,
            analysis: self.analysis.unwrap_or_default(),
            last_effective_model: None,
        }
    }
}

impl IdeationSession {
    /// Creates a new active session for a project
    pub fn new(project_id: ProjectId) -> Self {
        IdeationSessionBuilder::new().project_id(project_id).build()
    }

    /// Creates a new active session with a title
    pub fn new_with_title(project_id: ProjectId, title: impl Into<String>) -> Self {
        IdeationSessionBuilder::new()
            .project_id(project_id)
            .title(title)
            .build()
    }

    /// Creates a builder for more complex session creation
    pub fn builder() -> IdeationSessionBuilder {
        IdeationSessionBuilder::new()
    }

    /// Returns true if the session is active
    pub fn is_active(&self) -> bool {
        self.status == IdeationSessionStatus::Active
    }

    /// Returns true if the session has been archived
    pub fn is_archived(&self) -> bool {
        self.status == IdeationSessionStatus::Archived
    }

    /// Returns true if all proposals have been accepted and applied
    pub fn is_accepted(&self) -> bool {
        self.status == IdeationSessionStatus::Accepted
    }

    /// Archives the session
    pub fn archive(&mut self) {
        let now = Utc::now();
        self.status = IdeationSessionStatus::Archived;
        self.archived_at = Some(now);
        self.updated_at = now;
    }

    /// Marks the session as accepted (all proposals applied)
    pub fn mark_accepted(&mut self) {
        let now = Utc::now();
        self.status = IdeationSessionStatus::Accepted;
        self.converted_at = Some(now);
        self.updated_at = now;
    }

    /// Updates the updated_at timestamp to now
    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    pub fn plan_artifact_bundle(&self) -> Option<PlanArtifactBundle> {
        let (overview_id, blueprint_id, is_inherited) =
            if let Some(overview_id) = self.plan_artifact_id.clone() {
                (overview_id, self.plan_blueprint_artifact_id.clone(), false)
            } else {
                (
                    self.inherited_plan_artifact_id.clone()?,
                    self.inherited_plan_blueprint_artifact_id.clone(),
                    true,
                )
            };

        if self.plan_contract_version >= PLAN_CONTRACT_V2 && blueprint_id.is_none() {
            return None;
        }

        Some(PlanArtifactBundle {
            overview_id,
            blueprint_id,
            contract_version: self.plan_contract_version,
            is_inherited,
        })
    }

    pub fn has_exact_plan_verification(&self) -> bool {
        let Some(bundle) = self.plan_artifact_bundle() else {
            return false;
        };
        if self.verified_plan_artifact_id.as_ref() != Some(&bundle.overview_id) {
            return false;
        }
        match bundle.blueprint_id {
            Some(blueprint_id) => {
                self.verified_plan_blueprint_artifact_id.as_ref() == Some(&blueprint_id)
            }
            None => true,
        }
    }

    /// Validates that setting a parent session ID won't create a circular reference
    /// This is a domain validation that checks the proposed parent chain
    /// In practice, this would be called before persisting and would need access to a repository
    /// to walk the parent chain. The actual database-backed validation happens in the repository layer.
    pub fn validate_no_circular_parent(&self, proposed_parent_id: &IdeationSessionId) -> bool {
        // Self-reference is always invalid
        if self.id == *proposed_parent_id {
            return false;
        }

        // This method validates the logical constraint.
        // The actual parent chain walk happens at the repository level
        // where we have access to fetch parent sessions from the database.
        true
    }

    /// Deserialize an IdeationSession from a SQLite row
    /// Expects columns: id, project_id, title, status, plan_artifact_id, seed_task_id, parent_session_id, created_at, updated_at, archived_at, converted_at
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: IdeationSessionId::from_string(row.get::<_, String>("id")?),
            project_id: ProjectId::from_string(row.get::<_, String>("project_id")?),
            title: row.get("title")?,
            status: row
                .get::<_, String>("status")?
                .parse()
                .unwrap_or(IdeationSessionStatus::Active),
            plan_artifact_id: row
                .get::<_, Option<String>>("plan_artifact_id")?
                .map(ArtifactId::from_string),
            plan_blueprint_artifact_id: row
                .get::<_, Option<String>>("plan_blueprint_artifact_id")
                .unwrap_or(None)
                .map(ArtifactId::from_string),
            verified_plan_artifact_id: row
                .get::<_, Option<String>>("verified_plan_artifact_id")
                .unwrap_or(None)
                .map(ArtifactId::from_string),
            verified_plan_blueprint_artifact_id: row
                .get::<_, Option<String>>("verified_plan_blueprint_artifact_id")
                .unwrap_or(None)
                .map(ArtifactId::from_string),
            verified_plan_agent_run_id: row
                .get::<_, Option<String>>("verified_plan_agent_run_id")
                .unwrap_or(None),
            inherited_plan_artifact_id: row
                .get::<_, Option<String>>("inherited_plan_artifact_id")?
                .map(ArtifactId::from_string),
            inherited_plan_blueprint_artifact_id: row
                .get::<_, Option<String>>("inherited_plan_blueprint_artifact_id")
                .unwrap_or(None)
                .map(ArtifactId::from_string),
            plan_contract_version: row
                .get::<_, Option<i64>>("plan_contract_version")
                .unwrap_or(Some(i64::from(PLAN_CONTRACT_V1)))
                .unwrap_or(i64::from(PLAN_CONTRACT_V1)) as i32,
            seed_task_id: row
                .get::<_, Option<String>>("seed_task_id")?
                .map(TaskId::from_string),
            parent_session_id: row
                .get::<_, Option<String>>("parent_session_id")?
                .map(IdeationSessionId::from_string),
            created_at: Self::parse_datetime(row.get("created_at")?),
            updated_at: Self::parse_datetime(row.get("updated_at")?),
            archived_at: row
                .get::<_, Option<String>>("archived_at")?
                .map(Self::parse_datetime),
            converted_at: row
                .get::<_, Option<String>>("converted_at")?
                .map(Self::parse_datetime),
            title_source: row.get::<_, Option<String>>("title_source").unwrap_or(None),
            verification_status: row
                .get::<_, Option<String>>("verification_status")
                .unwrap_or(None)
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            verification_in_progress: row
                .get::<_, Option<i64>>("verification_in_progress")
                .unwrap_or(None)
                .map(|v| v != 0)
                .unwrap_or(false),
            verification_generation: row
                .get::<_, Option<i64>>("verification_generation")
                .unwrap_or(None)
                .map(|v| v as i32)
                .unwrap_or(0),
            verification_current_round: row
                .get::<_, Option<i64>>("verification_current_round")
                .unwrap_or(None)
                .map(|v| v as u32),
            verification_max_rounds: row
                .get::<_, Option<i64>>("verification_max_rounds")
                .unwrap_or(None)
                .map(|v| v as u32),
            verification_gap_count: row
                .get::<_, Option<i64>>("verification_gap_count")
                .unwrap_or(None)
                .map(|v| v as u32)
                .unwrap_or(0),
            verification_gap_score: row
                .get::<_, Option<i64>>("verification_gap_score")
                .unwrap_or(None)
                .map(|v| v as u32),
            verification_convergence_reason: row
                .get::<_, Option<String>>("verification_convergence_reason")
                .unwrap_or(None),
            source_project_id: row
                .get::<_, Option<String>>("source_project_id")
                .unwrap_or(None),
            source_session_id: row
                .get::<_, Option<String>>("source_session_id")
                .unwrap_or(None),
            source_task_id: row
                .get::<_, Option<String>>("source_task_id")
                .unwrap_or(None)
                .map(TaskId::from_string),
            source_context_type: row
                .get::<_, Option<String>>("source_context_type")
                .unwrap_or(None),
            source_context_id: row
                .get::<_, Option<String>>("source_context_id")
                .unwrap_or(None),
            spawn_reason: row.get::<_, Option<String>>("spawn_reason").unwrap_or(None),
            blocker_fingerprint: row
                .get::<_, Option<String>>("blocker_fingerprint")
                .unwrap_or(None),
            session_purpose: row
                .get::<_, Option<String>>("session_purpose")
                .unwrap_or(None)
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            session_flow: row
                .get::<_, Option<String>>("session_flow")
                .unwrap_or(None)
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            cross_project_checked: row
                .get::<_, Option<i64>>("cross_project_checked")
                .unwrap_or(None)
                .map(|v| v != 0)
                .unwrap_or(false),
            plan_version_last_read: row
                .get::<_, Option<i64>>("plan_version_last_read")
                .unwrap_or(None)
                .map(|v| v as i32),
            blueprint_version_last_read: row
                .get::<_, Option<i64>>("blueprint_version_last_read")
                .unwrap_or(None)
                .map(|v| v as i32),
            origin: row
                .get::<_, Option<String>>("origin")
                .unwrap_or(None)
                .as_deref()
                .and_then(|s| s.parse().ok())
                .unwrap_or_default(),
            expected_proposal_count: row
                .get::<_, Option<i64>>("expected_proposal_count")
                .unwrap_or(None)
                .map(|v| v as u32),
            auto_accept_status: row
                .get::<_, Option<String>>("auto_accept_status")
                .unwrap_or(None),
            auto_accept_started_at: row
                .get::<_, Option<String>>("auto_accept_started_at")
                .unwrap_or(None),
            api_key_id: row.get::<_, Option<String>>("api_key_id").unwrap_or(None),
            idempotency_key: row
                .get::<_, Option<String>>("idempotency_key")
                .unwrap_or(None),
            external_activity_phase: row
                .get::<_, Option<String>>("external_activity_phase")
                .unwrap_or(None),
            external_last_read_message_id: row
                .get::<_, Option<String>>("external_last_read_message_id")
                .unwrap_or(None),
            dependencies_acknowledged: row
                .get::<_, Option<i64>>("dependencies_acknowledged")
                .unwrap_or(None)
                .map(|v| v != 0)
                .unwrap_or(false),
            pending_initial_prompt: row
                .get::<_, Option<String>>("pending_initial_prompt")
                .unwrap_or(None),
            acceptance_status: row
                .get::<_, Option<String>>("acceptance_status")
                .unwrap_or(None)
                .as_deref()
                .and_then(|s| s.parse().ok()),
            verification_confirmation_status: row
                .get::<_, Option<String>>("verification_confirmation_status")
                .unwrap_or(None)
                .as_deref()
                .and_then(|s| s.parse().ok()),
            analysis: IdeationAnalysisState {
                base_ref_kind: row
                    .get::<_, Option<String>>("analysis_base_ref_kind")
                    .unwrap_or(None)
                    .as_deref()
                    .and_then(|s| s.parse().ok()),
                base_ref: row
                    .get::<_, Option<String>>("analysis_base_ref")
                    .unwrap_or(None),
                base_display_name: row
                    .get::<_, Option<String>>("analysis_base_display_name")
                    .unwrap_or(None),
                workspace_kind: row
                    .get::<_, Option<String>>("analysis_workspace_kind")
                    .unwrap_or(None)
                    .as_deref()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_default(),
                workspace_path: row
                    .get::<_, Option<String>>("analysis_workspace_path")
                    .unwrap_or(None),
                base_commit: row
                    .get::<_, Option<String>>("analysis_base_commit")
                    .unwrap_or(None),
                base_locked_at: row
                    .get::<_, Option<String>>("analysis_base_locked_at")
                    .unwrap_or(None)
                    .map(Self::parse_datetime),
            },
            last_effective_model: row
                .get::<_, Option<String>>("last_effective_model")
                .unwrap_or(None),
        })
    }

    /// Parse a datetime string from SQLite into a DateTime<Utc>
    /// Handles both RFC3339 format and SQLite's CURRENT_TIMESTAMP format
    fn parse_datetime(s: String) -> DateTime<Utc> {
        parse_datetime_helper(s)
    }
}
