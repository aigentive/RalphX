use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use crate::agents::{AgentHarnessKind, ProviderSessionRef};

use super::{
    AgentConversationWorkspaceMode, AutomationId, AutomationRunId, CoordinationMode,
    DelegatedSessionId, IdeationSessionId, ProjectId, TaskId,
};

/// Unique identifier for a chat conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChatConversationId(Uuid);

impl ChatConversationId {
    /// Create a new random conversation ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the underlying UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get as string
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    /// Create from string (for database deserialization)
    pub fn from_string(s: impl Into<String>) -> Self {
        let s = s.into();
        Self(Uuid::parse_str(&s).unwrap_or_else(|_| Uuid::nil()))
    }
}

impl Default for ChatConversationId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ChatConversationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for ChatConversationId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<ChatConversationId> for String {
    fn from(id: ChatConversationId) -> Self {
        id.0.to_string()
    }
}

impl std::str::FromStr for ChatConversationId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Type of context a conversation is associated with
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatContextType {
    /// Ideation session context
    Ideation,
    /// Delegated specialist session context
    Delegation,
    /// Task-specific context
    Task,
    /// Project-level context
    Project,
    /// Projectless conversation; its context_id is the conversation id.
    Standalone,
    /// Task execution context (worker output)
    #[serde(rename = "task_execution")]
    TaskExecution,
    /// Task review context (AI reviewer)
    Review,
    /// Merge conflict resolution context (merger agent)
    Merge,
    /// Branch synchronization and conflict-resolution context (branch updater)
    #[serde(rename = "branch_update")]
    BranchUpdate,
}

impl fmt::Display for ChatContextType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatContextType::Ideation => write!(f, "ideation"),
            ChatContextType::Delegation => write!(f, "delegation"),
            ChatContextType::Task => write!(f, "task"),
            ChatContextType::Project => write!(f, "project"),
            ChatContextType::Standalone => write!(f, "standalone"),
            ChatContextType::TaskExecution => write!(f, "task_execution"),
            ChatContextType::Review => write!(f, "review"),
            ChatContextType::Merge => write!(f, "merge"),
            ChatContextType::BranchUpdate => write!(f, "branch_update"),
        }
    }
}

impl std::str::FromStr for ChatContextType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ideation" => Ok(ChatContextType::Ideation),
            "delegation" => Ok(ChatContextType::Delegation),
            "task" => Ok(ChatContextType::Task),
            "project" => Ok(ChatContextType::Project),
            "standalone" => Ok(ChatContextType::Standalone),
            "task_execution" => Ok(ChatContextType::TaskExecution),
            "review" => Ok(ChatContextType::Review),
            "merge" => Ok(ChatContextType::Merge),
            "branch_update" => Ok(ChatContextType::BranchUpdate),
            _ => Err(format!("Invalid context type: {}", s)),
        }
    }
}

/// Status of historical attribution backfill for a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttributionBackfillStatus {
    Pending,
    Running,
    Completed,
    Partial,
    SessionNotFound,
    ParseFailed,
}

impl fmt::Display for AttributionBackfillStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttributionBackfillStatus::Pending => write!(f, "pending"),
            AttributionBackfillStatus::Running => write!(f, "running"),
            AttributionBackfillStatus::Completed => write!(f, "completed"),
            AttributionBackfillStatus::Partial => write!(f, "partial"),
            AttributionBackfillStatus::SessionNotFound => write!(f, "session_not_found"),
            AttributionBackfillStatus::ParseFailed => write!(f, "parse_failed"),
        }
    }
}

impl std::str::FromStr for AttributionBackfillStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(AttributionBackfillStatus::Pending),
            "running" => Ok(AttributionBackfillStatus::Running),
            "completed" => Ok(AttributionBackfillStatus::Completed),
            "partial" => Ok(AttributionBackfillStatus::Partial),
            "session_not_found" => Ok(AttributionBackfillStatus::SessionNotFound),
            "parse_failed" => Ok(AttributionBackfillStatus::ParseFailed),
            _ => Err(format!("Invalid attribution backfill status: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationAttributionBackfillState {
    pub status: Option<AttributionBackfillStatus>,
    pub source: Option<String>,
    pub source_path: Option<String>,
    pub last_attempted_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAttributionBackfillSummary {
    pub eligible_conversation_count: u64,
    pub pending_count: u64,
    pub running_count: u64,
    pub completed_count: u64,
    pub partial_count: u64,
    pub session_not_found_count: u64,
    pub parse_failed_count: u64,
}

impl ConversationAttributionBackfillSummary {
    pub fn remaining_count(&self) -> u64 {
        self.pending_count + self.running_count
    }

    pub fn attention_count(&self) -> u64 {
        self.partial_count + self.session_not_found_count + self.parse_failed_count
    }

    pub fn terminal_count(&self) -> u64 {
        self.completed_count + self.attention_count()
    }

    pub fn is_idle(&self) -> bool {
        self.running_count == 0
    }
}

/// A chat conversation linked to a context (ideation session, task, or project)
///
/// Multiple conversations can exist per context to support conversation history.
/// Each conversation tracks provider-neutral session metadata plus a temporary
/// Claude compatibility field during the migration window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConversation {
    /// Unique identifier for this conversation
    pub id: ChatConversationId,
    /// Type of context this conversation is for
    pub context_type: ChatContextType,
    /// ID of the context (session_id, task_id, or project_id)
    pub context_id: String,
    /// Claude CLI session UUID for --resume flag
    /// This is captured from Claude's response and enables conversation continuity
    pub claude_session_id: Option<String>,
    /// Provider-neutral session identifier used for both Claude and Codex.
    pub provider_session_id: Option<String>,
    /// Harness that owns the provider session.
    pub provider_harness: Option<AgentHarnessKind>,
    /// Upstream provider behind the selected harness.
    pub upstream_provider: Option<String>,
    /// Optional runtime/profile selector for the upstream provider.
    pub provider_profile: Option<String>,
    /// Current project-agent mode for Agents conversations.
    pub agent_mode: Option<AgentConversationWorkspaceMode>,
    /// Canonical specialist agent retained by a parented child conversation.
    #[serde(default)]
    pub bound_agent_name: Option<String>,
    /// Optional active persona bound to this conversation.
    pub persona_id: Option<String>,
    /// Optional persona draft owned by this builder conversation.
    pub builder_draft_id: Option<String>,
    /// Approved persona produced by this builder conversation.
    pub builder_result_persona_id: Option<String>,
    /// Conversation-level team coordination state.
    pub coordination_mode: CoordinationMode,
    /// Automation that owns this setup or run conversation.
    pub automation_id: Option<AutomationId>,
    /// Automation run that owns this run conversation.
    pub automation_run_id: Option<AutomationRunId>,
    /// Auto-generated or user-set title for this conversation
    pub title: Option<String>,
    /// Number of messages in this conversation
    pub message_count: i64,
    /// Timestamp of the last message
    pub last_message_at: Option<DateTime<Utc>>,
    /// When this conversation was created
    pub created_at: DateTime<Utc>,
    /// When this conversation was last updated
    pub updated_at: DateTime<Utc>,
    /// When this conversation was archived (soft-deleted). None = active.
    pub archived_at: Option<DateTime<Utc>>,
    /// ID of the prior execution's conversation (for TaskExecution re-runs only).
    /// Enables UI navigation between execution generations.
    pub parent_conversation_id: Option<String>,
    /// Historical attribution backfill state for compatibility imports.
    pub attribution_backfill_status: Option<AttributionBackfillStatus>,
    pub attribution_backfill_source: Option<String>,
    pub attribution_backfill_source_path: Option<String>,
    pub attribution_backfill_last_attempted_at: Option<DateTime<Utc>>,
    pub attribution_backfill_completed_at: Option<DateTime<Utc>>,
    pub attribution_backfill_error_summary: Option<String>,
}

impl ChatConversation {
    /// Create a new conversation for an ideation session
    pub fn new_ideation(session_id: IdeationSessionId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::Ideation,
            context_id: session_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new conversation for a task
    pub fn new_task(task_id: TaskId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::Task,
            context_id: task_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new conversation for a delegated specialist session
    pub fn new_delegation(session_id: DelegatedSessionId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::Delegation,
            context_id: session_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new conversation for a project
    pub fn new_project(project_id: ProjectId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::Project,
            context_id: project_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new self-keyed projectless conversation.
    ///
    /// Standalone conversations key their `context_id` to their own generated
    /// `id` (enforced invariant: `context_id == id`) since there is no
    /// external project/session/task row to key against.
    pub fn new_standalone() -> Self {
        let now = Utc::now();
        let id = ChatConversationId::new();
        Self {
            id,
            context_type: ChatContextType::Standalone,
            context_id: id.as_str(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Returns true iff this conversation satisfies the Standalone self-key
    /// invariant: `context_type == Standalone && context_id == id`.
    pub fn is_valid_standalone_self_key(&self) -> bool {
        self.context_type == ChatContextType::Standalone && self.context_id == self.id.as_str()
    }

    /// Returns true only for the two context types that own PersonaBuilder
    /// conversations and the PersonaBuilder workspace mode.
    pub const fn is_persona_builder_identity(
        context_type: ChatContextType,
        agent_mode: Option<AgentConversationWorkspaceMode>,
    ) -> bool {
        matches!(
            context_type,
            ChatContextType::Project | ChatContextType::Standalone
        ) && matches!(
            agent_mode,
            Some(AgentConversationWorkspaceMode::PersonaBuilder)
        )
    }

    /// Returns true when this row is a valid Project or Standalone PersonaBuilder.
    pub const fn is_persona_builder(&self) -> bool {
        Self::is_persona_builder_identity(self.context_type, self.agent_mode)
    }

    /// Create a new conversation for task execution (worker output).
    /// Pass `parent_id` when re-executing a task to link to the prior run's conversation.
    pub fn new_task_execution(task_id: TaskId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::TaskExecution,
            context_id: task_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new conversation for task review (reviewer agent)
    pub fn new_review(task_id: TaskId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::Review,
            context_id: task_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new conversation for merge conflict resolution (merger agent)
    pub fn new_merge(task_id: TaskId) -> Self {
        let now = Utc::now();
        Self {
            id: ChatConversationId::new(),
            context_type: ChatContextType::Merge,
            context_id: task_id.as_str().to_string(),
            claude_session_id: None,
            provider_session_id: None,
            provider_harness: None,
            upstream_provider: None,
            provider_profile: None,
            agent_mode: None,
            bound_agent_name: None,
            persona_id: None,
            builder_draft_id: None,
            builder_result_persona_id: None,
            coordination_mode: CoordinationMode::Solo,
            automation_id: None,
            automation_run_id: None,
            title: None,
            message_count: 0,
            last_message_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
            parent_conversation_id: None,
            attribution_backfill_status: None,
            attribution_backfill_source: None,
            attribution_backfill_source_path: None,
            attribution_backfill_last_attempted_at: None,
            attribution_backfill_completed_at: None,
            attribution_backfill_error_summary: None,
        }
    }

    /// Create a new conversation for dedicated branch-update conflict resolution.
    pub fn new_branch_update(task_id: TaskId) -> Self {
        let mut conversation = Self::new_review(task_id);
        conversation.context_type = ChatContextType::BranchUpdate;
        conversation
    }

    pub fn update_attribution_backfill_state(
        &mut self,
        state: ConversationAttributionBackfillState,
    ) {
        self.attribution_backfill_status = state.status;
        self.attribution_backfill_source = state.source;
        self.attribution_backfill_source_path = state.source_path;
        self.attribution_backfill_last_attempted_at = state.last_attempted_at;
        self.attribution_backfill_completed_at = state.completed_at;
        self.attribution_backfill_error_summary = state.error_summary;
        self.updated_at = Utc::now();
    }

    /// Update the canonical provider session reference.
    pub fn set_provider_session_ref(&mut self, session_ref: ProviderSessionRef) {
        let ProviderSessionRef {
            harness,
            provider_session_id,
        } = session_ref;
        let (claude_session_id, provider_session_id, provider_harness) =
            compatible_provider_session_fields_from_provider_ref(
                Some(harness),
                Some(provider_session_id),
            );
        self.claude_session_id = claude_session_id;
        self.provider_session_id = provider_session_id;
        self.provider_harness = provider_harness;
        self.updated_at = Utc::now();
    }

    /// Update the Claude session ID (after first message in conversation)
    pub fn set_claude_session_id(&mut self, session_id: impl Into<String>) {
        self.set_provider_session_ref(ProviderSessionRef {
            harness: AgentHarnessKind::Claude,
            provider_session_id: session_id.into(),
        });
    }

    /// Clear any provider session reference.
    pub fn clear_provider_session_ref(&mut self) {
        self.claude_session_id = None;
        self.provider_session_id = None;
        self.provider_harness = None;
        self.updated_at = Utc::now();
    }

    pub fn set_provider_origin(
        &mut self,
        upstream_provider: Option<String>,
        provider_profile: Option<String>,
    ) {
        self.upstream_provider = upstream_provider;
        self.provider_profile = provider_profile;
        self.updated_at = Utc::now();
    }

    pub fn set_agent_mode(&mut self, mode: Option<AgentConversationWorkspaceMode>) {
        self.agent_mode = mode;
        self.updated_at = Utc::now();
    }

    pub fn set_coordination_mode(&mut self, mode: CoordinationMode) {
        self.coordination_mode = mode;
        self.updated_at = Utc::now();
    }

    /// Set or update the conversation title
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
        self.updated_at = Utc::now();
    }

    /// Archive this conversation.
    pub fn archive(&mut self) {
        let now = Utc::now();
        self.archived_at = Some(now);
        self.updated_at = now;
    }

    /// Restore this conversation from archive.
    pub fn restore(&mut self) {
        self.archived_at = None;
        self.updated_at = Utc::now();
    }

    /// Returns true when this conversation is archived.
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// Check if this conversation has a Claude session (can use --resume)
    pub fn has_claude_session(&self) -> bool {
        self.claude_session_id.is_some()
            || matches!(self.provider_harness, Some(AgentHarnessKind::Claude))
                && self.provider_session_id.is_some()
    }

    /// Get the effective provider session reference for this conversation.
    pub fn provider_session_ref(&self) -> Option<ProviderSessionRef> {
        if let (Some(harness), Some(provider_session_id)) =
            (self.provider_harness, self.provider_session_id.clone())
        {
            return Some(ProviderSessionRef {
                harness,
                provider_session_id,
            });
        }

        self.claude_session_id
            .clone()
            .map(|provider_session_id| ProviderSessionRef {
                harness: AgentHarnessKind::Claude,
                provider_session_id,
            })
    }

    /// Get response-safe provider metadata with legacy compatibility restored.
    pub fn compatible_provider_session_fields(
        &self,
    ) -> (Option<String>, Option<String>, Option<AgentHarnessKind>) {
        normalize_provider_session_compatibility(
            self.claude_session_id.clone(),
            self.provider_session_id.clone(),
            self.provider_harness,
        )
    }

    /// Normalize stored provider metadata back into the canonical compatibility shape.
    pub fn normalize_provider_session_fields(&mut self) {
        let (claude_session_id, provider_session_id, provider_harness) =
            self.compatible_provider_session_fields();
        self.claude_session_id = claude_session_id;
        self.provider_session_id = provider_session_id;
        self.provider_harness = provider_harness;
    }

    /// Get a display title for this conversation
    pub fn display_title(&self) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| "Untitled conversation".to_string())
    }
}

pub fn legacy_claude_session_alias(
    harness: Option<AgentHarnessKind>,
    provider_session_id: Option<&str>,
) -> Option<String> {
    matches!(harness, Some(AgentHarnessKind::Claude))
        .then(|| provider_session_id.map(str::to_string))
        .flatten()
}

pub fn compatible_provider_session_fields_from_provider_ref(
    provider_harness: Option<AgentHarnessKind>,
    provider_session_id: Option<String>,
) -> (Option<String>, Option<String>, Option<AgentHarnessKind>) {
    let claude_session_id =
        legacy_claude_session_alias(provider_harness, provider_session_id.as_deref());
    (claude_session_id, provider_session_id, provider_harness)
}

pub fn normalize_provider_session_compatibility(
    claude_session_id: Option<String>,
    provider_session_id: Option<String>,
    provider_harness: Option<AgentHarnessKind>,
) -> (Option<String>, Option<String>, Option<AgentHarnessKind>) {
    let mut normalized_claude_session_id = claude_session_id;
    let mut normalized_provider_session_id = provider_session_id;
    let mut normalized_provider_harness = provider_harness;

    if normalized_provider_session_id.is_none() && normalized_claude_session_id.is_some() {
        normalized_provider_session_id = normalized_claude_session_id.clone();
        normalized_provider_harness = Some(AgentHarnessKind::Claude);
    }

    if normalized_claude_session_id.is_none() {
        normalized_claude_session_id = legacy_claude_session_alias(
            normalized_provider_harness,
            normalized_provider_session_id.as_deref(),
        );
    }

    (
        normalized_claude_session_id,
        normalized_provider_session_id,
        normalized_provider_harness,
    )
}

#[cfg(test)]
#[path = "chat_conversation_tests.rs"]
mod tests;
