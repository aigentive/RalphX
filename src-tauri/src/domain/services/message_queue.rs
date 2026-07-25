// Unified Message Queue Service
//
// Generic message queue that handles all chat context types.
// Keyed by (ChatContextType, context_id) instead of just TaskId.
//
// This is a consolidation of ExecutionMessageQueue to support
// queueing messages for all context types, not just TaskExecution.

use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{ChatAttachmentId, ChatContextType, PersonaDirective, TaskId};
use crate::domain::services::ComposerSelectionSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn is_false(value: &bool) -> bool {
    !*value
}

/// User-selected project reference metadata that must survive queue replay.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComposerProjectReferenceKind {
    File,
    Directory,
}

/// A project file/folder reference selected in the chat composer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposerProjectReference {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ComposerProjectReferenceKind>,
}

/// An external integration reference selected in the chat composer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposerIntegrationReference {
    pub provider: String,
    pub kind: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_transcript: Option<bool>,
}

/// An artifact reference selected in the chat composer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposerArtifactReference {
    pub artifact_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// A bounded plain-text excerpt selected from an artifact pane source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ComposerExcerptReference {
    pub source_kind: String,
    pub source_id: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

/// Key for the message queue - combines context type and ID
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct QueueKey {
    pub context_type: ChatContextType,
    pub context_id: String,
}

impl QueueKey {
    pub fn new(context_type: ChatContextType, context_id: impl Into<String>) -> Self {
        Self {
            context_type,
            context_id: context_id.into(),
        }
    }

    /// Create a key for task execution context (convenience method)
    pub fn task_execution(task_id: &TaskId) -> Self {
        Self::new(ChatContextType::TaskExecution, task_id.as_str())
    }

    /// Create a key for ideation context
    pub fn ideation(session_id: &str) -> Self {
        Self::new(ChatContextType::Ideation, session_id)
    }

    /// Create a key for task context
    pub fn task(task_id: &str) -> Self {
        Self::new(ChatContextType::Task, task_id)
    }

    /// Create a key for project context
    pub fn project(project_id: &str) -> Self {
        Self::new(ChatContextType::Project, project_id)
    }
}

/// A queued message waiting to be sent to an agent
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedMessage {
    pub id: String,
    pub content: String,
    pub created_at: String,
    pub is_editing: bool,
    /// Optional metadata JSON to apply when persisting this message (survives queue round-trip)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_override: Option<String>,
    /// Optional RFC3339 timestamp override (preserves trigger time through queue)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_override: Option<String>,
    /// Optional runtime harness override to preserve relaunch/recovery provider continuity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness_override: Option<AgentHarnessKind>,
    /// Optional canonical agent override selected when this message was queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name_override: Option<String>,
    /// Persona intent selected when this message was queued.
    #[serde(default)]
    pub persona_directive: PersonaDirective,
    /// Optional model override selected when this message was queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_override: Option<String>,
    /// Optional provider-neutral effort override selected when this message was queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort_override: Option<LogicalEffort>,
    /// Optional provider service-tier override selected when this message was queued.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier_override: Option<String>,
    /// Keep the parent conversation provider-session ref unchanged on replay.
    #[serde(default, skip_serializing_if = "is_false")]
    pub preserve_conversation_provider_session_ref: bool,
    /// Whether queue replay must start a fresh provider-native session.
    #[serde(default, skip_serializing_if = "is_false")]
    pub force_new_provider_session: bool,
    /// Optional composer project references used for runtime-only prompt expansion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composer_project_references: Vec<ComposerProjectReference>,
    /// Optional external integration references used for runtime-only prompt expansion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composer_integration_references: Vec<ComposerIntegrationReference>,
    /// Optional artifact references used for runtime-only prompt expansion.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composer_artifact_references: Vec<ComposerArtifactReference>,
    /// Optional immutable artifact/ticket excerpt used for this queued turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer_selection_snapshot: Option<ComposerSelectionSnapshot>,
    /// Optional selected excerpts used for runtime-only prompt context.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub composer_excerpt_references: Vec<ComposerExcerptReference>,
    /// Optional chat attachments selected by the composer for this queued turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<ChatAttachmentId>,
}

impl QueuedMessage {
    /// Create a new queued message with generated ID and timestamp
    pub fn new(content: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            created_at: chrono::Utc::now().to_rfc3339(),
            is_editing: false,
            metadata_override: None,
            created_at_override: None,
            harness_override: None,
            agent_name_override: None,
            persona_directive: PersonaDirective::Inherit,
            model_override: None,
            logical_effort_override: None,
            service_tier_override: None,
            preserve_conversation_provider_session_ref: false,
            force_new_provider_session: false,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
            composer_selection_snapshot: None,
            composer_excerpt_references: Vec::new(),
            attachment_ids: Vec::new(),
        }
    }

    /// Create a new queued message with a client-provided ID
    /// This allows the frontend to track the message with its own ID
    pub fn with_id(id: String, content: String) -> Self {
        Self {
            id,
            content,
            created_at: chrono::Utc::now().to_rfc3339(),
            is_editing: false,
            metadata_override: None,
            created_at_override: None,
            harness_override: None,
            agent_name_override: None,
            persona_directive: PersonaDirective::Inherit,
            model_override: None,
            logical_effort_override: None,
            service_tier_override: None,
            preserve_conversation_provider_session_ref: false,
            force_new_provider_session: false,
            composer_project_references: Vec::new(),
            composer_integration_references: Vec::new(),
            composer_artifact_references: Vec::new(),
            composer_selection_snapshot: None,
            composer_excerpt_references: Vec::new(),
            attachment_ids: Vec::new(),
        }
    }
}

/// Unified in-memory queue for chat messages
///
/// Stores queued messages per (context_type, context_id) pair.
/// This is the live process buffer; durable restart-safe ownership lives in
/// `QueuedMessageRepository` implementations.
#[derive(Debug, Clone)]
pub struct MessageQueue {
    queues: Arc<Mutex<HashMap<QueueKey, Vec<QueuedMessage>>>>,
}

impl MessageQueue {
    /// Create a new empty queue
    pub fn new() -> Self {
        Self {
            queues: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Queue a message for a context
    pub fn queue(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        content: String,
    ) -> QueuedMessage {
        let key = QueueKey::new(context_type, context_id);
        let message = QueuedMessage::new(content);
        let mut queues = self.queues.lock().unwrap();
        queues.entry(key).or_default().push(message.clone());
        message
    }

    /// Queue a message at the front of the queue (high priority).
    ///
    /// Used by session swap recovery to inject conversation history before
    /// any pending user messages in the queue.
    pub fn queue_front(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        content: String,
    ) -> QueuedMessage {
        let key = QueueKey::new(context_type, context_id);
        let message = QueuedMessage::new(content);
        let mut queues = self.queues.lock().unwrap();
        queues.entry(key).or_default().insert(0, message.clone());
        message
    }

    /// Re-insert an existing queued message at the front of the queue.
    ///
    /// Used when queue processing has already popped a message but a runtime
    /// barrier (for example global pause/stop) prevents launching it right now.
    pub fn queue_front_existing(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        message: QueuedMessage,
    ) {
        let key = QueueKey::new(context_type, context_id);
        let mut queues = self.queues.lock().unwrap();
        queues.entry(key).or_default().insert(0, message);
    }

    /// Re-insert an existing queued message at the back of the queue.
    ///
    /// Message IDs are stable queue identities. Re-enqueuing one replaces its
    /// earlier occurrence and leaves all unrelated messages in their order.
    pub fn queue_back_existing(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        message: QueuedMessage,
    ) {
        let key = QueueKey::new(context_type, context_id);
        let mut queues = self.queues.lock().unwrap();
        queues.retain(|_, queue| {
            queue.retain(|queued| queued.id != message.id);
            !queue.is_empty()
        });
        queues.entry(key).or_default().push(message);
    }

    /// Queue a message using a QueueKey
    pub fn queue_with_key(&self, key: QueueKey, content: String) -> QueuedMessage {
        let message = QueuedMessage::new(content);
        let mut queues = self.queues.lock().unwrap();
        queues.entry(key).or_default().push(message.clone());
        message
    }

    /// Queue a message with a client-provided ID
    /// This allows frontend and backend to use the same ID for tracking
    pub fn queue_with_client_id(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        content: String,
        client_id: String,
    ) -> QueuedMessage {
        let key = QueueKey::new(context_type, context_id);
        let message = QueuedMessage::with_id(client_id, content);
        let mut queues = self.queues.lock().unwrap();
        queues.entry(key).or_default().push(message.clone());
        message
    }

    /// Queue a message with metadata, timestamp, and runtime harness overrides.
    ///
    /// Used by Gate 2 when auto-verification or other send_message callers
    /// pass SendMessageOptions — the overrides must survive the queue round-trip.
    pub fn queue_with_overrides(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        content: String,
        metadata_override: Option<String>,
        created_at_override: Option<String>,
        harness_override: Option<AgentHarnessKind>,
    ) -> QueuedMessage {
        self.queue_with_overrides_and_project_references(
            context_type,
            context_id,
            content,
            metadata_override,
            created_at_override,
            harness_override,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
        )
    }

    /// Queue a message with send overrides and composer project references.
    pub fn queue_with_overrides_and_project_references(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        content: String,
        metadata_override: Option<String>,
        created_at_override: Option<String>,
        harness_override: Option<AgentHarnessKind>,
        composer_project_references: Vec<ComposerProjectReference>,
        composer_integration_references: Vec<ComposerIntegrationReference>,
        composer_artifact_references: Vec<ComposerArtifactReference>,
        composer_selection_snapshot: Option<ComposerSelectionSnapshot>,
        composer_excerpt_references: Vec<ComposerExcerptReference>,
        attachment_ids: Vec<ChatAttachmentId>,
    ) -> QueuedMessage {
        self.queue_with_runtime_overrides_and_project_references(
            context_type,
            context_id,
            content,
            metadata_override,
            created_at_override,
            harness_override,
            None,
            PersonaDirective::Inherit,
            None,
            None,
            None,
            false,
            composer_project_references,
            composer_integration_references,
            composer_artifact_references,
            composer_selection_snapshot,
            composer_excerpt_references,
            attachment_ids,
        )
    }

    /// Queue a message with the full runtime selection captured at enqueue time.
    #[allow(clippy::too_many_arguments)]
    pub fn queue_with_runtime_overrides_and_project_references(
        &self,
        context_type: ChatContextType,
        context_id: impl Into<String>,
        content: String,
        metadata_override: Option<String>,
        created_at_override: Option<String>,
        harness_override: Option<AgentHarnessKind>,
        agent_name_override: Option<String>,
        persona_directive: PersonaDirective,
        model_override: Option<String>,
        logical_effort_override: Option<LogicalEffort>,
        service_tier_override: Option<String>,
        force_new_provider_session: bool,
        composer_project_references: Vec<ComposerProjectReference>,
        composer_integration_references: Vec<ComposerIntegrationReference>,
        composer_artifact_references: Vec<ComposerArtifactReference>,
        composer_selection_snapshot: Option<ComposerSelectionSnapshot>,
        composer_excerpt_references: Vec<ComposerExcerptReference>,
        attachment_ids: Vec<ChatAttachmentId>,
    ) -> QueuedMessage {
        let key = QueueKey::new(context_type, context_id);
        let mut message = QueuedMessage::new(content);
        message.metadata_override = metadata_override;
        message.created_at_override = created_at_override;
        message.harness_override = harness_override;
        message.agent_name_override = agent_name_override;
        message.persona_directive = persona_directive;
        message.model_override = model_override;
        message.logical_effort_override = logical_effort_override;
        message.service_tier_override = service_tier_override;
        message.force_new_provider_session = force_new_provider_session;
        message.composer_project_references = composer_project_references;
        message.composer_integration_references = composer_integration_references;
        message.composer_artifact_references = composer_artifact_references;
        message.composer_selection_snapshot = composer_selection_snapshot;
        message.composer_excerpt_references = composer_excerpt_references;
        message.attachment_ids = attachment_ids;
        let mut queues = self.queues.lock().unwrap();
        queues.entry(key).or_default().push(message.clone());
        message
    }

    /// Pop the next message from the queue (FIFO)
    pub fn pop(&self, context_type: ChatContextType, context_id: &str) -> Option<QueuedMessage> {
        let key = QueueKey::new(context_type, context_id.to_string());
        let mut queues = self.queues.lock().unwrap();
        queues.get_mut(&key).and_then(|queue| {
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
            }
        })
    }

    /// Pop using a QueueKey
    pub fn pop_with_key(&self, key: &QueueKey) -> Option<QueuedMessage> {
        let mut queues = self.queues.lock().unwrap();
        queues.get_mut(key).and_then(|queue| {
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
            }
        })
    }

    /// Remove and return a specific queued message by ID.
    pub fn take(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message_id: &str,
    ) -> Option<QueuedMessage> {
        let key = QueueKey::new(context_type, context_id.to_string());
        let mut queues = self.queues.lock().unwrap();
        let queue = queues.get_mut(&key)?;
        let pos = queue.iter().position(|m| m.id == message_id)?;
        let message = queue.remove(pos);
        if queue.is_empty() {
            queues.remove(&key);
        }
        Some(message)
    }

    /// Get all queued messages for a context (without removing them)
    pub fn get_queued(
        &self,
        context_type: ChatContextType,
        context_id: &str,
    ) -> Vec<QueuedMessage> {
        let key = QueueKey::new(context_type, context_id.to_string());
        let queues = self.queues.lock().unwrap();
        queues.get(&key).cloned().unwrap_or_default()
    }

    /// Get queued messages using a QueueKey
    pub fn get_queued_with_key(&self, key: &QueueKey) -> Vec<QueuedMessage> {
        let queues = self.queues.lock().unwrap();
        queues.get(key).cloned().unwrap_or_default()
    }

    /// List all queue keys that currently have one or more queued messages.
    pub fn list_keys(&self) -> Vec<QueueKey> {
        let queues = self.queues.lock().unwrap();
        queues
            .iter()
            .filter_map(|(key, queue)| {
                if queue.is_empty() {
                    None
                } else {
                    Some(key.clone())
                }
            })
            .collect()
    }

    /// Clear all queued messages for a context
    pub fn clear(&self, context_type: ChatContextType, context_id: &str) {
        let key = QueueKey::new(context_type, context_id.to_string());
        let mut queues = self.queues.lock().unwrap();
        queues.remove(&key);
    }

    /// Clear using a QueueKey
    pub fn clear_with_key(&self, key: &QueueKey) {
        let mut queues = self.queues.lock().unwrap();
        queues.remove(key);
    }

    /// Delete a specific queued message by ID
    pub fn delete(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        message_id: &str,
    ) -> bool {
        let key = QueueKey::new(context_type, context_id.to_string());
        let mut queues = self.queues.lock().unwrap();
        if let Some(queue) = queues.get_mut(&key) {
            if let Some(pos) = queue.iter().position(|m| m.id == message_id) {
                queue.remove(pos);
                return true;
            }
        }
        false
    }

    /// Delete using a QueueKey
    pub fn delete_with_key(&self, key: &QueueKey, message_id: &str) -> bool {
        let mut queues = self.queues.lock().unwrap();
        if let Some(queue) = queues.get_mut(key) {
            if let Some(pos) = queue.iter().position(|m| m.id == message_id) {
                queue.remove(pos);
                return true;
            }
        }
        false
    }

    /// Remove messages older than `threshold_secs` seconds from the queue.
    ///
    /// Returns the list of dropped messages so callers can emit warnings.
    /// Messages with unparseable timestamps are retained (safe default).
    /// Rehydration messages injected by `queue_front` are freshly created and
    /// will always be within the threshold, so no special handling is needed.
    pub fn remove_stale(
        &self,
        context_type: ChatContextType,
        context_id: &str,
        threshold_secs: u64,
    ) -> Vec<QueuedMessage> {
        let key = QueueKey::new(context_type, context_id.to_string());
        let mut queues = self.queues.lock().unwrap();
        let queue = match queues.get_mut(&key) {
            Some(q) => q,
            None => return vec![],
        };

        let now = chrono::Utc::now();
        let mut dropped = vec![];
        queue.retain(|msg| {
            let is_stale = chrono::DateTime::parse_from_rfc3339(&msg.created_at)
                .map(|ts| {
                    let age = now.signed_duration_since(ts.with_timezone(&chrono::Utc));
                    age.num_seconds() > threshold_secs as i64
                })
                .unwrap_or(false); // unparseable → retain (safe default)
            if is_stale {
                dropped.push(msg.clone());
            }
            !is_stale
        });
        dropped
    }

    // =========================================================================
    // Backwards-compatible methods for TaskId (used by existing code)
    // =========================================================================

    /// Queue a message for a task execution (backwards compatibility)
    pub fn queue_for_task(&self, task_id: TaskId, content: String) -> QueuedMessage {
        self.queue(ChatContextType::TaskExecution, task_id.as_str(), content)
    }

    /// Pop the next message for a task execution (backwards compatibility)
    pub fn pop_for_task(&self, task_id: &TaskId) -> Option<QueuedMessage> {
        self.pop(ChatContextType::TaskExecution, task_id.as_str())
    }

    /// Get all queued messages for a task execution (backwards compatibility)
    pub fn get_queued_for_task(&self, task_id: &TaskId) -> Vec<QueuedMessage> {
        self.get_queued(ChatContextType::TaskExecution, task_id.as_str())
    }

    /// Clear all queued messages for a task execution (backwards compatibility)
    pub fn clear_for_task(&self, task_id: &TaskId) {
        self.clear(ChatContextType::TaskExecution, task_id.as_str())
    }

    /// Delete a queued message for a task execution (backwards compatibility)
    pub fn delete_for_task(&self, task_id: &TaskId, message_id: &str) -> bool {
        self.delete(ChatContextType::TaskExecution, task_id.as_str(), message_id)
    }

    /// Count the number of queued messages for a given context.
    ///
    /// Used by the queue depth cap check and status response enrichment.
    pub fn count_for_context(&self, context_type: &str, context_id: &str) -> usize {
        let Ok(ctx_type) = context_type.parse::<ChatContextType>() else {
            return 0;
        };
        let key = QueueKey::new(ctx_type, context_id);
        let queues = self.queues.lock().unwrap();
        queues.get(&key).map(|v| v.len()).unwrap_or(0)
    }
}

impl Default for MessageQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "message_queue_tests.rs"]
mod tests;
