use chrono::Utc;

use crate::domain::entities::{
    AgentConversationGranolaNoteLink, AgentConversationGranolaRefreshStatus, ChatConversationId,
    ProjectId,
};
use crate::domain::repositories::AgentConversationGranolaNoteRepository;

use super::MemoryAgentConversationGranolaNoteRepository;

fn link(conversation_id: &ChatConversationId, note_id: &str) -> AgentConversationGranolaNoteLink {
    AgentConversationGranolaNoteLink::new(
        conversation_id.clone(),
        ProjectId::from_string("project-1".to_string()),
        note_id.to_string(),
        Utc::now(),
    )
}

#[tokio::test]
async fn memory_granola_repo_upserts_fetches_and_clears() {
    let repo = MemoryAgentConversationGranolaNoteRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-1".to_string());
    let first = repo
        .upsert(link(&conversation_id, "not_1234567890ABCD"))
        .await
        .unwrap();
    let mut replacement = link(&conversation_id, "not_ABCDEFGHIJKLMN");
    replacement.refresh_status = AgentConversationGranolaRefreshStatus::Loaded;

    let updated = repo.upsert(replacement).await.unwrap();
    assert_eq!(updated.created_at, first.created_at);
    assert_eq!(updated.note_id, "not_ABCDEFGHIJKLMN");

    let fetched = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fetched.refresh_status,
        AgentConversationGranolaRefreshStatus::Loaded
    );

    repo.clear(&conversation_id).await.unwrap();
    assert!(repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memory_granola_repo_insert_if_absent_preserves_existing_link() {
    let repo = MemoryAgentConversationGranolaNoteRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-1".to_string());

    repo.insert_if_absent(link(&conversation_id, "not_1234567890ABCD"))
        .await
        .unwrap();
    let kept = repo
        .insert_if_absent(link(&conversation_id, "not_ABCDEFGHIJKLMN"))
        .await
        .unwrap();

    assert_eq!(kept.note_id, "not_1234567890ABCD");
}
