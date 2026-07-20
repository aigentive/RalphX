use std::sync::Arc;

use super::delegation::{persist_terminal_projection, DelegationJobSnapshot};
use crate::domain::entities::{ChatConversation, ProjectId};
use crate::domain::repositories::{ChatConversationRepository, ChatTimelineRepository};
use crate::infrastructure::sqlite::{
    SqliteChatConversationRepository, SqliteChatTimelineRepository,
};
use crate::testing::SqliteTestDb;

#[tokio::test]
async fn terminal_projection_is_idempotent_and_fully_hydrated() {
    let db = SqliteTestDb::new("delegation-terminal-projection");
    let conversation_repo = SqliteChatConversationRepository::from_shared(db.shared_conn());
    let timeline_repo: Arc<dyn ChatTimelineRepository> =
        Arc::new(SqliteChatTimelineRepository::from_shared(db.shared_conn()));
    let conversation_id = conversation_repo
        .create(ChatConversation::new_project(ProjectId::new()))
        .await
        .expect("create parent conversation")
        .id;
    let snapshot = DelegationJobSnapshot {
        job_id: "job-terminal-projection".to_string(),
        parent_context_type: "project".to_string(),
        parent_context_id: "project-1".to_string(),
        parent_turn_id: None,
        parent_message_id: None,
        parent_conversation_id: Some(conversation_id.to_string()),
        parent_tool_use_id: Some("call-delegate-start".to_string()),
        delegated_session_id: "delegated-session".to_string(),
        delegated_conversation_id: Some("delegated-conversation".to_string()),
        delegated_agent_run_id: Some("delegated-run".to_string()),
        agent_name: "reviewer".to_string(),
        harness: "codex".to_string(),
        provider_session_id: Some("thread-1".to_string()),
        upstream_provider: Some("openai".to_string()),
        provider_profile: Some("openai".to_string()),
        logical_model: Some("gpt-5.4".to_string()),
        effective_model_id: Some("gpt-5.4-2026-04-01".to_string()),
        logical_effort: Some("high".to_string()),
        effective_effort: Some("high".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("workspace-write".to_string()),
        status: "completed".to_string(),
        content: Some("x".repeat(2_000)),
        error: None,
        started_at: "2026-04-12T10:00:00Z".to_string(),
        completed_at: Some("2026-04-12T10:00:05Z".to_string()),
        history: Vec::new(),
        delegated_status: None,
    };

    persist_terminal_projection(&timeline_repo, &snapshot, None)
        .await
        .expect("persist terminal projection");
    persist_terminal_projection(&timeline_repo, &snapshot, None)
        .await
        .expect("repeat terminal projection");

    assert_eq!(
        timeline_repo
            .count_by_conversation(&conversation_id)
            .await
            .expect("count projection rows"),
        1
    );
    let page = timeline_repo
        .get_page(&conversation_id, 10, None)
        .await
        .expect("load terminal projection");
    let terminal = page.items.first().expect("terminal projection row");
    assert_eq!(terminal.tool_name.as_deref(), Some("delegate_terminal"));
    let result: serde_json::Value = serde_json::from_str(
        terminal
            .result_json
            .as_deref()
            .expect("fully hydrated terminal result"),
    )
    .expect("terminal result json");
    assert_eq!(result["job_id"], "job-terminal-projection");
    assert_eq!(result["status"], "completed");
    assert_eq!(result["content"].as_str().map(str::len), Some(2_000));
}
