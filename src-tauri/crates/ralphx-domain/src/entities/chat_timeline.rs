use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::agents::AgentHarnessKind;

use super::{AgentRunId, ChatConversationId, ChatMessageId, MessageRole};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChatTimelineItemId(pub String);

impl ChatTimelineItemId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for ChatTimelineItemId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChatTimelineItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTimelineItemKind {
    Text,
    Thinking,
    ToolUse,
    Task,
    SystemNotice,
    Error,
}

impl std::fmt::Display for ChatTimelineItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => write!(f, "text"),
            Self::Thinking => write!(f, "thinking"),
            Self::ToolUse => write!(f, "tool_use"),
            Self::Task => write!(f, "task"),
            Self::SystemNotice => write!(f, "system_notice"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl FromStr for ChatTimelineItemKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "text" => Ok(Self::Text),
            "thinking" => Ok(Self::Thinking),
            "tool_use" => Ok(Self::ToolUse),
            "task" => Ok(Self::Task),
            "system_notice" => Ok(Self::SystemNotice),
            "error" => Ok(Self::Error),
            _ => Err(format!("unknown timeline item kind: {value}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatTimelineItemStatus {
    Streaming,
    Finalized,
    Error,
}

impl std::fmt::Display for ChatTimelineItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Streaming => write!(f, "streaming"),
            Self::Finalized => write!(f, "finalized"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl FromStr for ChatTimelineItemStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "streaming" => Ok(Self::Streaming),
            "finalized" => Ok(Self::Finalized),
            "error" => Ok(Self::Error),
            _ => Err(format!("unknown timeline item status: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTimelineItem {
    pub id: ChatTimelineItemId,
    pub conversation_id: ChatConversationId,
    pub message_id: Option<ChatMessageId>,
    pub run_id: Option<AgentRunId>,
    pub sequence: i64,
    pub block_index: i64,
    pub role: MessageRole,
    pub kind: ChatTimelineItemKind,
    pub status: ChatTimelineItemStatus,
    pub text: Option<String>,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    pub tool_status: Option<String>,
    pub tool_input_preview: Option<String>,
    pub tool_result_preview: Option<String>,
    pub input_json: Option<String>,
    pub result_json: Option<String>,
    pub raw_block_json: Option<String>,
    pub metadata: Option<String>,
    pub provider_harness: Option<AgentHarnessKind>,
    pub provider_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
}

impl ChatTimelineItem {
    pub fn stable_message_block_id(
        message_id: &ChatMessageId,
        block_index: i64,
    ) -> ChatTimelineItemId {
        ChatTimelineItemId::from_string(format!("block:{}:{block_index}", message_id.as_str()))
    }

    pub fn for_message_block(
        message_id: ChatMessageId,
        conversation_id: ChatConversationId,
        block_index: i64,
        role: MessageRole,
        kind: ChatTimelineItemKind,
    ) -> Self {
        let id = Self::stable_message_block_id(&message_id, block_index);
        let now = Utc::now();
        Self {
            id,
            conversation_id,
            message_id: Some(message_id),
            run_id: None,
            sequence: 0,
            block_index,
            role,
            kind,
            status: ChatTimelineItemStatus::Streaming,
            text: None,
            tool_call_id: None,
            tool_name: None,
            tool_status: None,
            tool_input_preview: None,
            tool_result_preview: None,
            input_json: None,
            result_json: None,
            raw_block_json: None,
            metadata: None,
            provider_harness: None,
            provider_session_id: None,
            created_at: now,
            updated_at: now,
            finalized_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTimelinePage {
    pub items: Vec<ChatTimelineItem>,
    pub limit: u32,
    pub before_sequence: Option<i64>,
    pub total_item_count: u32,
    pub has_older: bool,
    pub oldest_loaded_sequence: Option<i64>,
    pub newest_loaded_sequence: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_status_parse_and_display_snake_case_values() {
        let kinds = [
            ("text", ChatTimelineItemKind::Text),
            ("tool_use", ChatTimelineItemKind::ToolUse),
            ("task", ChatTimelineItemKind::Task),
            ("system_notice", ChatTimelineItemKind::SystemNotice),
            ("error", ChatTimelineItemKind::Error),
        ];
        for (raw, kind) in kinds {
            assert_eq!(ChatTimelineItemKind::from_str(raw), Ok(kind));
            assert_eq!(kind.to_string(), raw);
        }

        let statuses = [
            ("streaming", ChatTimelineItemStatus::Streaming),
            ("finalized", ChatTimelineItemStatus::Finalized),
            ("error", ChatTimelineItemStatus::Error),
        ];
        for (raw, status) in statuses {
            assert_eq!(ChatTimelineItemStatus::from_str(raw), Ok(status));
            assert_eq!(status.to_string(), raw);
        }

        assert!(ChatTimelineItemKind::from_str("unknown").is_err());
        assert!(ChatTimelineItemStatus::from_str("unknown").is_err());
    }

    #[test]
    fn message_block_constructor_uses_stable_identity_and_streaming_defaults() {
        let message_id = ChatMessageId::from_string("assistant-message");
        let conversation_id = ChatConversationId::from_string("conversation");

        let item = ChatTimelineItem::for_message_block(
            message_id.clone(),
            conversation_id,
            7,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::ToolUse,
        );

        assert_eq!(item.id.as_str(), "block:assistant-message:7");
        assert_eq!(
            ChatTimelineItem::stable_message_block_id(&message_id, 7).as_str(),
            item.id.as_str()
        );
        assert_eq!(item.message_id.as_ref(), Some(&message_id));
        assert_eq!(item.block_index, 7);
        assert_eq!(item.sequence, 0);
        assert_eq!(item.role, MessageRole::Orchestrator);
        assert_eq!(item.kind, ChatTimelineItemKind::ToolUse);
        assert_eq!(item.status, ChatTimelineItemStatus::Streaming);
        assert!(item.finalized_at.is_none());
    }
}
