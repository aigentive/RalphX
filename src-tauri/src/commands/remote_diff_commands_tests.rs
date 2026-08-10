use crate::application::DiffRefKind;
use crate::commands::remote_diff_commands::{
    get_remote_agent_conversation_workspace_change_summary,
    get_remote_agent_conversation_workspace_commit_file_diff,
    get_remote_agent_conversation_workspace_cumulative_file_diff,
    get_remote_agent_conversation_workspace_file_diff,
    get_remote_agent_conversation_workspace_file_diff_page,
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

    let file_diff = get_remote_agent_conversation_workspace_file_diff(
        uuid::Uuid::new_v4().to_string(),
        "src/main.rs".to_string(),
    )
    .await;
    let commit_diff = get_remote_agent_conversation_workspace_commit_file_diff(
        uuid::Uuid::new_v4().to_string(),
        "abc123".to_string(),
        "src/main.rs".to_string(),
    )
    .await;
    let cumulative_diff = get_remote_agent_conversation_workspace_cumulative_file_diff(
        uuid::Uuid::new_v4().to_string(),
        "src/main.rs".to_string(),
    )
    .await;
    let page = get_remote_agent_conversation_workspace_file_diff_page(
        uuid::Uuid::new_v4().to_string(),
        "src/main.rs".to_string(),
        DiffRefKind::Head,
        200,
        100,
    )
    .await;

    for envelope in [file_diff, commit_diff, cumulative_diff] {
        assert!(envelope.snapshot.is_none());
        assert!(envelope.captured_at.is_none());
        assert!(envelope.cache_version.is_none());
        assert!(envelope.context_source.is_none());
    }
    assert!(page.snapshot.is_none());
    assert!(page.captured_at.is_none());
    assert!(page.cache_version.is_none());
    assert!(page.context_source.is_none());
}
