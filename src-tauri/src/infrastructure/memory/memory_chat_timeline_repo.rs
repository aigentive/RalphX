use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{
    ChatConversationId, ChatMessageId, ChatTimelineItem, ChatTimelineItemId, ChatTimelineItemKind,
    ChatTimelineItemStatus, ChatTimelinePage,
};
use crate::domain::repositories::ChatTimelineRepository;
use crate::error::AppResult;

pub struct MemoryChatTimelineRepository {
    items: RwLock<HashMap<String, ChatTimelineItem>>,
}

const RALPHX_TOOL_NAME_PREFIXES: [&str; 6] = [
    "mcp__ralphx__",
    "mcp__ralphx_internal__",
    "ralphx::",
    "ralphx_internal::",
    "ralphx:",
    "ralphx_internal:",
];
const DIFF_TOOL_NAMES: [&str; 2] = ["edit", "write"];
const ASK_USER_QUESTION_TOOL_NAME: &str = "ask_user_question";
const DELEGATION_TOOL_NAMES: [&str; 4] = [
    "delegate_start",
    "delegate_wait",
    "delegate_cancel",
    "delegate_terminal",
];

#[doc(hidden)]
pub(crate) fn retains_full_raw_tool_payload(tool_name: &str) -> bool {
    let normalized = tool_name.trim().to_ascii_lowercase();
    let normalized = RALPHX_TOOL_NAME_PREFIXES
        .iter()
        .find_map(|prefix| normalized.strip_prefix(prefix).map(str::to_string))
        .unwrap_or(normalized);
    let leaf_name = normalized.rsplit("::").next().unwrap_or(&normalized);
    DIFF_TOOL_NAMES.contains(&leaf_name)
        || normalized == ASK_USER_QUESTION_TOOL_NAME
        || DELEGATION_TOOL_NAMES.contains(&normalized.as_str())
}

impl MemoryChatTimelineRepository {
    pub fn new() -> Self {
        Self {
            items: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryChatTimelineRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatTimelineRepository for MemoryChatTimelineRepository {
    async fn upsert_item(&self, mut item: ChatTimelineItem) -> AppResult<ChatTimelineItem> {
        if item.kind != ChatTimelineItemKind::ToolUse
            || !item
                .tool_name
                .as_deref()
                .is_some_and(retains_full_raw_tool_payload)
        {
            item.raw_block_json = None;
        }
        let mut items = self.items.write().unwrap();
        if let Some(existing) = items.get(item.id.as_str()) {
            item.sequence = existing.sequence;
            item.created_at = existing.created_at;
        } else if item.sequence <= 0 {
            item.sequence = items
                .values()
                .filter(|existing| existing.conversation_id == item.conversation_id)
                .map(|existing| existing.sequence)
                .max()
                .unwrap_or(0)
                + 1;
        }
        items.insert(item.id.to_string(), item.clone());
        Ok(item)
    }

    async fn get_by_id(&self, id: &ChatTimelineItemId) -> AppResult<Option<ChatTimelineItem>> {
        Ok(self.items.read().unwrap().get(id.as_str()).cloned())
    }

    async fn get_page(
        &self,
        conversation_id: &ChatConversationId,
        limit: u32,
        before_sequence: Option<i64>,
    ) -> AppResult<ChatTimelinePage> {
        let normalized_limit = limit.clamp(1, 200);
        let mut conversation_items: Vec<_> = self
            .items
            .read()
            .unwrap()
            .values()
            .filter(|item| &item.conversation_id == conversation_id)
            .filter(|item| {
                before_sequence
                    .map(|before| item.sequence < before)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        conversation_items.sort_by_key(|item| item.sequence);

        let total_item_count = self.count_by_conversation(conversation_id).await?;
        let skip_count = conversation_items
            .len()
            .saturating_sub(normalized_limit as usize);
        let items = conversation_items
            .into_iter()
            .skip(skip_count)
            .collect::<Vec<_>>();
        let oldest_loaded_sequence = items.first().map(|item| item.sequence);
        let newest_loaded_sequence = items.last().map(|item| item.sequence);
        let has_older = oldest_loaded_sequence
            .map(|sequence| {
                self.items.read().unwrap().values().any(|item| {
                    &item.conversation_id == conversation_id && item.sequence < sequence
                })
            })
            .unwrap_or(false);

        Ok(ChatTimelinePage {
            items,
            limit: normalized_limit,
            before_sequence,
            total_item_count,
            has_older,
            oldest_loaded_sequence,
            newest_loaded_sequence,
        })
    }

    async fn count_by_conversation(&self, conversation_id: &ChatConversationId) -> AppResult<u32> {
        Ok(self
            .items
            .read()
            .unwrap()
            .values()
            .filter(|item| &item.conversation_id == conversation_id)
            .count() as u32)
    }

    async fn get_by_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ChatTimelineItem>> {
        let mut items: Vec<_> = self
            .items
            .read()
            .unwrap()
            .values()
            .filter(|item| &item.conversation_id == conversation_id)
            .cloned()
            .collect();
        items.sort_by_key(|item| item.sequence);
        Ok(items)
    }

    async fn latest_assistant_activity_at_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
        assistant_role: crate::domain::entities::MessageRole,
    ) -> AppResult<Option<chrono::DateTime<Utc>>> {
        Ok(self
            .items
            .read()
            .unwrap()
            .values()
            .filter(|item| &item.conversation_id == conversation_id && item.role == assistant_role)
            .map(|item| item.updated_at)
            .max())
    }

    async fn delete_message_items_except_block_indices(
        &self,
        message_id: &ChatMessageId,
        retained_block_indices: Vec<i64>,
    ) -> AppResult<()> {
        let retained_block_indices: std::collections::HashSet<i64> =
            retained_block_indices.into_iter().collect();
        self.items.write().unwrap().retain(|_, item| {
            item.message_id.as_ref() != Some(message_id)
                || retained_block_indices.contains(&item.block_index)
        });
        Ok(())
    }

    async fn mark_message_items_finalized(&self, message_id: &ChatMessageId) -> AppResult<()> {
        let mut items = self.items.write().unwrap();
        let now = Utc::now();
        for item in items.values_mut() {
            if item.message_id.as_ref() == Some(message_id) {
                item.status = ChatTimelineItemStatus::Finalized;
                item.updated_at = now;
                item.finalized_at.get_or_insert(now);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::entities::{ChatTimelineItemKind, MessageRole};

    use super::*;

    fn item(
        conversation_id: ChatConversationId,
        message_id: &str,
        block_index: i64,
        text: &str,
    ) -> ChatTimelineItem {
        let mut item = ChatTimelineItem::for_message_block(
            ChatMessageId::from_string(message_id),
            conversation_id,
            block_index,
            MessageRole::Orchestrator,
            ChatTimelineItemKind::Text,
        );
        item.text = Some(text.to_string());
        item
    }

    #[tokio::test]
    async fn page_uses_tail_window_and_before_sequence_cursor() {
        let repo = MemoryChatTimelineRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation");

        let first = repo
            .upsert_item(item(conversation_id, "message-1", 0, "first"))
            .await
            .expect("insert first");
        let second = repo
            .upsert_item(item(conversation_id, "message-1", 1, "second"))
            .await
            .expect("insert second");
        let third = repo
            .upsert_item(item(conversation_id, "message-2", 0, "third"))
            .await
            .expect("insert third");

        assert_eq!((first.sequence, second.sequence, third.sequence), (1, 2, 3));

        let newest = repo
            .get_page(&conversation_id, 2, None)
            .await
            .expect("newest page");
        assert_eq!(
            newest
                .items
                .iter()
                .map(|item| item.text.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("second"), Some("third")]
        );
        assert!(newest.has_older);
        assert_eq!(newest.total_item_count, 3);

        let older = repo
            .get_page(&conversation_id, 2, newest.oldest_loaded_sequence)
            .await
            .expect("older page");
        assert_eq!(older.items.len(), 1);
        assert_eq!(older.items[0].text.as_deref(), Some("first"));
        assert!(!older.has_older);
    }

    #[tokio::test]
    async fn upsert_preserves_sequence_and_finalize_marks_matching_message_only() {
        let repo = MemoryChatTimelineRepository::new();
        let conversation_id = ChatConversationId::from_string("conversation");
        let message_id = ChatMessageId::from_string("message-1");
        let other_message_id = ChatMessageId::from_string("message-2");

        let original = repo
            .upsert_item(item(conversation_id, message_id.as_str(), 0, "draft"))
            .await
            .expect("insert original");
        let mut updated = item(conversation_id, message_id.as_str(), 0, "final");
        updated.status = ChatTimelineItemStatus::Error;
        let updated = repo.upsert_item(updated).await.expect("update original");
        let other = repo
            .upsert_item(item(conversation_id, other_message_id.as_str(), 0, "other"))
            .await
            .expect("insert other");

        assert_eq!(updated.sequence, original.sequence);
        assert_eq!(updated.created_at, original.created_at);
        assert_eq!(other.sequence, original.sequence + 1);

        repo.mark_message_items_finalized(&message_id)
            .await
            .expect("finalize message");

        let finalized = repo
            .get_by_id(&updated.id)
            .await
            .expect("load finalized")
            .expect("finalized item");
        let untouched = repo
            .get_by_id(&other.id)
            .await
            .expect("load untouched")
            .expect("untouched item");

        assert_eq!(finalized.status, ChatTimelineItemStatus::Finalized);
        assert!(finalized.finalized_at.is_some());
        assert_eq!(untouched.status, ChatTimelineItemStatus::Streaming);
        assert!(untouched.finalized_at.is_none());
    }
}
