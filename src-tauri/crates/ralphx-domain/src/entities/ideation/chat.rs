//! Chat message entities

use chrono::{DateTime, Utc};
use rusqlite::Row;
use serde::{Deserialize, Serialize};
use std::{io, str::FromStr};

use super::types::parse_datetime_helper;
use crate::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use crate::entities::{AgentRunUsage, ProviderUsageSnapshot, UsageCapture, UsageProvenance};
use crate::entities::{ChatConversationId, ChatMessageId, IdeationSessionId, ProjectId, TaskId};

fn parse_usage_provenance(value: Option<String>) -> rusqlite::Result<Option<UsageProvenance>> {
    value
        .map(|value| {
            value.parse::<UsageProvenance>().map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(io::Error::new(io::ErrorKind::InvalidData, error)),
                )
            })
        })
        .transpose()
}

// ============================================================================
// ChatMessage and Related Types
// ============================================================================

/// Role of the message sender in a chat conversation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    /// Message from the human user
    User,
    /// Message from the Orchestrator AI agent
    Orchestrator,
    /// System message (e.g., session started, context changed)
    System,
    /// Message from the Worker AI agent (task execution output)
    Worker,
    /// Message from the Reviewer AI agent (task review)
    Reviewer,
    /// Message from the Merger AI agent (merge conflict resolution)
    Merger,
}

impl Default for MessageRole {
    fn default() -> Self {
        Self::User
    }
}

impl std::fmt::Display for MessageRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageRole::User => write!(f, "user"),
            MessageRole::Orchestrator => write!(f, "orchestrator"),
            MessageRole::System => write!(f, "system"),
            MessageRole::Worker => write!(f, "worker"),
            MessageRole::Reviewer => write!(f, "reviewer"),
            MessageRole::Merger => write!(f, "merger"),
        }
    }
}

/// Error type for parsing MessageRole from a string
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseMessageRoleError {
    pub value: String,
}

impl std::fmt::Display for ParseMessageRoleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown message role: '{}'", self.value)
    }
}

impl std::error::Error for ParseMessageRoleError {}

impl FromStr for MessageRole {
    type Err = ParseMessageRoleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(MessageRole::User),
            "orchestrator" => Ok(MessageRole::Orchestrator),
            "system" => Ok(MessageRole::System),
            "worker" => Ok(MessageRole::Worker),
            "reviewer" => Ok(MessageRole::Reviewer),
            "merger" => Ok(MessageRole::Merger),
            _ => Err(ParseMessageRoleError {
                value: s.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessageAttribution {
    pub attribution_source: Option<String>,
    pub provider_harness: Option<AgentHarnessKind>,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub effective_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatMessageUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
    /// Semantics of the normalized usage tuple. `None` denotes legacy data.
    pub usage_provenance: Option<UsageProvenance>,
    /// Raw cumulative provider snapshot retained only for later delta derivation.
    pub raw_usage_snapshot: Option<ProviderUsageSnapshot>,
}

/// A chat message in an ideation session or project/task context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Unique identifier for this message
    pub id: ChatMessageId,
    /// Session this message belongs to (for ideation context)
    pub session_id: Option<IdeationSessionId>,
    /// Project this message belongs to (for project context without session)
    pub project_id: Option<ProjectId>,
    /// Task this message is about (for task-specific context)
    pub task_id: Option<TaskId>,
    /// Conversation this message belongs to (for context-aware chat)
    pub conversation_id: Option<ChatConversationId>,
    /// Who sent the message
    pub role: MessageRole,
    /// The message content (supports Markdown)
    pub content: String,
    /// Optional metadata (JSON) for additional context
    pub metadata: Option<String>,
    /// Parent message ID for threading (if applicable)
    pub parent_message_id: Option<ChatMessageId>,
    /// Tool calls made during this message (JSON array)
    /// Stores the tools that Claude called when generating this message
    pub tool_calls: Option<String>,
    /// Content blocks in order (text and tool calls interleaved, JSON array)
    /// When present, this preserves the order of text and tool calls
    pub content_blocks: Option<String>,
    /// Attribution source for this message (`native_runtime`, `historical_backfill`, etc.).
    pub attribution_source: Option<String>,
    /// Harness/model metadata for provider-backed assistant messages.
    pub provider_harness: Option<AgentHarnessKind>,
    pub provider_session_id: Option<String>,
    pub upstream_provider: Option<String>,
    pub provider_profile: Option<String>,
    pub logical_model: Option<String>,
    pub effective_model_id: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
    pub effective_effort: Option<String>,
    /// Usage/cost metadata captured for this assistant message.
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub estimated_usd: Option<f64>,
    /// Semantics of the normalized usage tuple. `None` denotes legacy data.
    pub usage_provenance: Option<UsageProvenance>,
    /// Raw cumulative provider snapshot retained only for later delta derivation.
    pub raw_usage_snapshot: Option<ProviderUsageSnapshot>,
    /// When the message was created
    pub created_at: DateTime<Utc>,
}

impl ChatMessage {
    /// Create a new user message in an ideation session
    pub fn user_in_session(session_id: IdeationSessionId, content: impl Into<String>) -> Self {
        Self {
            id: ChatMessageId::new(),
            session_id: Some(session_id),
            project_id: None,
            task_id: None,
            conversation_id: None,
            role: MessageRole::User,
            content: content.into(),
            metadata: None,
            parent_message_id: None,
            tool_calls: None,
            content_blocks: None,
            attribution_source: None,
            provider_harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            created_at: Utc::now(),
        }
    }

    /// Create a new orchestrator message in an ideation session
    pub fn orchestrator_in_session(
        session_id: IdeationSessionId,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: ChatMessageId::new(),
            session_id: Some(session_id),
            project_id: None,
            task_id: None,
            conversation_id: None,
            role: MessageRole::Orchestrator,
            content: content.into(),
            metadata: None,
            parent_message_id: None,
            tool_calls: None,
            content_blocks: None,
            attribution_source: None,
            provider_harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            created_at: Utc::now(),
        }
    }

    /// Create a new system message in an ideation session
    pub fn system_in_session(session_id: IdeationSessionId, content: impl Into<String>) -> Self {
        Self {
            id: ChatMessageId::new(),
            session_id: Some(session_id),
            project_id: None,
            task_id: None,
            conversation_id: None,
            role: MessageRole::System,
            content: content.into(),
            metadata: None,
            parent_message_id: None,
            tool_calls: None,
            content_blocks: None,
            attribution_source: None,
            provider_harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            created_at: Utc::now(),
        }
    }

    /// Create a new user message in a project context (no session)
    pub fn user_in_project(project_id: ProjectId, content: impl Into<String>) -> Self {
        Self {
            id: ChatMessageId::new(),
            session_id: None,
            project_id: Some(project_id),
            task_id: None,
            conversation_id: None,
            role: MessageRole::User,
            content: content.into(),
            metadata: None,
            parent_message_id: None,
            tool_calls: None,
            content_blocks: None,
            attribution_source: None,
            provider_harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            created_at: Utc::now(),
        }
    }

    /// Create a new user message about a specific task
    pub fn user_about_task(task_id: TaskId, content: impl Into<String>) -> Self {
        Self {
            id: ChatMessageId::new(),
            session_id: None,
            project_id: None,
            task_id: Some(task_id),
            conversation_id: None,
            role: MessageRole::User,
            content: content.into(),
            metadata: None,
            parent_message_id: None,
            tool_calls: None,
            content_blocks: None,
            attribution_source: None,
            provider_harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            created_at: Utc::now(),
        }
    }

    /// Set metadata on this message
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Set parent message for threading
    pub fn with_parent(mut self, parent_id: ChatMessageId) -> Self {
        self.parent_message_id = Some(parent_id);
        self
    }

    pub fn with_attribution(mut self, attribution: ChatMessageAttribution) -> Self {
        self.attribution_source = attribution.attribution_source;
        self.provider_harness = attribution.provider_harness;
        self.provider_session_id = attribution.provider_session_id;
        self.upstream_provider = attribution.upstream_provider;
        self.provider_profile = attribution.provider_profile;
        self.logical_model = attribution.logical_model;
        self.effective_model_id = attribution.effective_model_id;
        self.logical_effort = attribution.logical_effort;
        self.effective_effort = attribution.effective_effort;
        self
    }

    pub fn update_provider_session_ref(&mut self, session_ref: &ProviderSessionRef) {
        self.provider_harness = Some(session_ref.harness);
        self.provider_session_id = Some(session_ref.provider_session_id.clone());
    }

    pub fn apply_attribution(&mut self, attribution: &ChatMessageAttribution) {
        if let Some(value) = attribution.attribution_source.as_ref() {
            self.attribution_source = Some(value.clone());
        }
        if let Some(value) = attribution.provider_harness {
            self.provider_harness = Some(value);
        }
        if let Some(value) = attribution.provider_session_id.as_ref() {
            self.provider_session_id = Some(value.clone());
        }
        if let Some(value) = attribution.upstream_provider.as_ref() {
            self.upstream_provider = Some(value.clone());
        }
        if let Some(value) = attribution.provider_profile.as_ref() {
            self.provider_profile = Some(value.clone());
        }
        if let Some(value) = attribution.logical_model.as_ref() {
            self.logical_model = Some(value.clone());
        }
        if let Some(value) = attribution.effective_model_id.as_ref() {
            self.effective_model_id = Some(value.clone());
        }
        if let Some(value) = attribution.logical_effort {
            self.logical_effort = Some(value);
        }
        if let Some(value) = attribution.effective_effort.as_ref() {
            self.effective_effort = Some(value.clone());
        }
    }

    pub fn apply_usage(&mut self, usage: &AgentRunUsage) {
        if let Some(value) = usage.input_tokens {
            self.input_tokens = Some(value);
        }
        if let Some(value) = usage.output_tokens {
            self.output_tokens = Some(value);
        }
        if let Some(value) = usage.cache_creation_tokens {
            self.cache_creation_tokens = Some(value);
        }
        if let Some(value) = usage.cache_read_tokens {
            self.cache_read_tokens = Some(value);
        }
        if let Some(value) = usage.estimated_usd {
            self.estimated_usd = Some(value);
        }
    }

    pub fn replace_usage_capture(&mut self, capture: &UsageCapture) {
        self.input_tokens = capture.normalized.input_tokens;
        self.output_tokens = capture.normalized.output_tokens;
        self.cache_creation_tokens = capture.normalized.cache_creation_tokens;
        self.cache_read_tokens = capture.normalized.cache_read_tokens;
        self.estimated_usd = capture.normalized.estimated_usd;
        self.usage_provenance = Some(capture.provenance);
        self.raw_usage_snapshot = capture.raw_snapshot.clone();
    }

    /// Check if this is a user message
    pub fn is_user(&self) -> bool {
        self.role == MessageRole::User
    }

    /// Check if this is an orchestrator message
    pub fn is_orchestrator(&self) -> bool {
        self.role == MessageRole::Orchestrator
    }

    /// Check if this is a system message
    pub fn is_system(&self) -> bool {
        self.role == MessageRole::System
    }

    /// Create from a rusqlite Row
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let id: String = row.get("id")?;
        let session_id: Option<String> = row.get("session_id")?;
        let project_id: Option<String> = row.get("project_id")?;
        let task_id: Option<String> = row.get("task_id")?;
        let conversation_id: Option<String> = row.get("conversation_id").ok().flatten();
        let role: String = row.get("role")?;
        let content: String = row.get("content")?;
        let metadata: Option<String> = row.get("metadata")?;
        let parent_message_id: Option<String> = row.get("parent_message_id")?;
        let tool_calls: Option<String> = row.get("tool_calls").ok().flatten();
        let content_blocks: Option<String> = row.get("content_blocks").ok().flatten();
        let attribution_source: Option<String> = row.get("attribution_source").ok().flatten();
        let provider_harness = row
            .get::<_, Option<String>>("provider_harness")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<AgentHarnessKind>().ok());
        let provider_session_id: Option<String> = row.get("provider_session_id").ok().flatten();
        let upstream_provider: Option<String> = row.get("upstream_provider").ok().flatten();
        let provider_profile: Option<String> = row.get("provider_profile").ok().flatten();
        let logical_model: Option<String> = row.get("logical_model").ok().flatten();
        let effective_model_id: Option<String> = row.get("effective_model_id").ok().flatten();
        let logical_effort = row
            .get::<_, Option<String>>("logical_effort")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<LogicalEffort>().ok());
        let effective_effort: Option<String> = row.get("effective_effort").ok().flatten();
        let input_tokens: Option<u64> = row.get("input_tokens").ok().flatten();
        let output_tokens: Option<u64> = row.get("output_tokens").ok().flatten();
        let cache_creation_tokens: Option<u64> = row.get("cache_creation_tokens").ok().flatten();
        let cache_read_tokens: Option<u64> = row.get("cache_read_tokens").ok().flatten();
        let estimated_usd: Option<f64> = row.get("estimated_usd").ok().flatten();
        let usage_provenance = parse_usage_provenance(
            row.get::<_, Option<String>>("usage_provenance")
                .ok()
                .flatten(),
        )?;
        let raw_usage_snapshot = ProviderUsageSnapshot {
            input_tokens: row.get("raw_usage_input_tokens").ok().flatten(),
            output_tokens: row.get("raw_usage_output_tokens").ok().flatten(),
            cache_creation_tokens: row.get("raw_usage_cache_creation_tokens").ok().flatten(),
            cache_read_tokens: row.get("raw_usage_cache_read_tokens").ok().flatten(),
            estimated_usd: row.get("raw_usage_estimated_usd").ok().flatten(),
        };
        let raw_usage_snapshot =
            (!raw_usage_snapshot.as_usage().is_empty()).then_some(raw_usage_snapshot);
        let created_at_str: String = row.get("created_at")?;

        Ok(Self {
            id: ChatMessageId::from_string(id),
            session_id: session_id.map(IdeationSessionId::from_string),
            project_id: project_id.map(ProjectId::from_string),
            task_id: task_id.map(TaskId::from_string),
            conversation_id: conversation_id.map(ChatConversationId::from_string),
            role: MessageRole::from_str(&role).unwrap_or(MessageRole::User),
            content,
            metadata,
            parent_message_id: parent_message_id.map(ChatMessageId::from_string),
            tool_calls,
            content_blocks,
            attribution_source,
            provider_harness,
            provider_session_id,
            upstream_provider,
            provider_profile,
            logical_model,
            effective_model_id,
            logical_effort,
            effective_effort,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            estimated_usd,
            usage_provenance,
            raw_usage_snapshot,
            created_at: parse_datetime_helper(created_at_str),
        })
    }
}
