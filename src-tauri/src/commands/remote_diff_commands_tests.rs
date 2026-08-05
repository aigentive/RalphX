use crate::commands::remote_diff_commands::{
    get_remote_agent_conversation_workspace_change_summary,
    get_remote_agent_conversation_workspace_review,
};

#[tokio::test]
async fn remote_diff_reads_preserve_uncaptured_snapshot_absence() {
    let conversation_id = uuid::Uuid::new_v4().to_string();

    let summary =
        get_remote_agent_conversation_workspace_change_summary(conversation_id.clone()).await;
    let review = get_remote_agent_conversation_workspace_review(conversation_id).await;

    assert!(summary.snapshot.is_none());
    assert!(summary.captured_at.is_none());
    assert!(summary.cache_version.is_none());
    assert!(summary.context_source.is_none());
    assert!(review.snapshot.is_none());
    assert!(review.captured_at.is_none());
    assert!(review.cache_version.is_none());
    assert!(review.context_source.is_none());
}
