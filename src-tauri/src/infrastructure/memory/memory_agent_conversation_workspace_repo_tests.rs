use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, ChatConversationId, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;

#[tokio::test]
async fn pr_review_auto_approve_settings_and_claim_round_trip() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let project_id = ProjectId::from_string("project-1".to_string());

    repo.upsert_pr_review_monitor(AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project_id.clone(),
        702,
        Some("head-a".to_string()),
    ))
    .await
    .expect("insert monitor");

    let updated = repo
        .set_pr_review_auto_approve_enabled(&conversation_id, false)
        .await
        .expect("disable auto approve");
    assert!(!updated.auto_approve_enabled);
    assert!(!updated.first_action_resolved);

    let resolved = repo
        .mark_pr_review_first_action_resolved(&conversation_id)
        .await
        .expect("mark first action resolved");
    assert!(resolved.first_action_resolved);

    repo.upsert_pr_review_monitor(AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project_id,
        702,
        Some("head-b".to_string()),
    ))
    .await
    .expect("upsert monitor preserves auto approve preferences");

    let preserved = repo
        .get_pr_review_monitor(&conversation_id)
        .await
        .expect("load monitor")
        .expect("monitor exists");
    assert!(!preserved.auto_approve_enabled);
    assert!(preserved.first_action_resolved);
    assert_eq!(preserved.last_seen_head_sha.as_deref(), Some("head-b"));

    let action = AgentWorkspacePrReviewAction::new(
        conversation_id,
        702,
        "head-b".to_string(),
        AgentWorkspacePrReviewActionKind::Approve,
        "passes".to_string(),
        "approved".to_string(),
        None,
        Some("review-run-1".to_string()),
    );
    let action_id = action.id.clone();
    repo.create_or_update_pr_review_action(action)
        .await
        .expect("insert action");

    assert!(repo
        .claim_pending_pr_review_action(&action_id)
        .await
        .expect("claim pending action"));
    assert!(!repo
        .claim_pending_pr_review_action(&action_id)
        .await
        .expect("do not claim non-pending action"));
    assert!(!repo
        .claim_pending_pr_review_action("missing-action")
        .await
        .expect("missing action is not claimed"));

    let claimed = repo
        .get_pr_review_action(&action_id)
        .await
        .expect("load action")
        .expect("action exists");
    assert_eq!(
        claimed.status,
        AgentWorkspacePrReviewActionStatus::Submitting
    );
}

#[tokio::test]
async fn pr_review_monitor_rejects_stale_disabled_upserts_after_pause_and_restart() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::from_string("conversation-paused");
    let project_id = ProjectId::from_string("project-paused".to_string());
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project_id,
        703,
        Some("head-a".to_string()),
    );
    monitor.monitor_enabled = true;
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.last_seen_head_sha = Some("authoritative-head".to_string());
    monitor.last_reviewed_head_sha = Some("authoritative-reviewed-head".to_string());
    monitor.last_review_outcome = Some("authoritative-outcome".to_string());
    monitor.review_artifact_head_sha = Some("authoritative-artifact-head".to_string());
    monitor.review_artifact_version = Some(2);
    monitor.updated_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    repo.upsert_pr_review_monitor(monitor.clone())
        .await
        .expect("insert monitor");

    let mut stale_disabled_callback = monitor;
    stale_disabled_callback.monitor_enabled = false;
    stale_disabled_callback.status = AgentWorkspacePrReviewMonitorStatus::Paused;
    stale_disabled_callback.last_seen_head_sha = Some("stale-head".to_string());
    stale_disabled_callback.last_reviewed_head_sha = Some("stale-reviewed-head".to_string());
    stale_disabled_callback.last_review_outcome = Some("stale-outcome".to_string());
    stale_disabled_callback.review_artifact_head_sha = Some("stale-artifact-head".to_string());
    stale_disabled_callback.review_artifact_version = Some(1);

    repo.set_pr_review_monitor_enabled(&conversation_id, false)
        .await
        .expect("pause monitor");

    let stale_write = repo
        .upsert_pr_review_monitor(stale_disabled_callback.clone())
        .await
        .expect("stale callback write");
    assert!(!stale_write.monitor_enabled);
    assert_eq!(
        stale_write.status,
        AgentWorkspacePrReviewMonitorStatus::Paused
    );
    assert_eq!(
        stale_write.last_seen_head_sha.as_deref(),
        Some("authoritative-head")
    );
    assert_eq!(
        stale_write.last_reviewed_head_sha.as_deref(),
        Some("authoritative-reviewed-head")
    );
    assert_eq!(
        stale_write.last_review_outcome.as_deref(),
        Some("authoritative-outcome")
    );
    assert_eq!(
        stale_write.review_artifact_head_sha.as_deref(),
        Some("authoritative-artifact-head")
    );
    assert_eq!(stale_write.review_artifact_version, Some(2));

    let restarted = repo
        .set_pr_review_monitor_enabled(&conversation_id, true)
        .await
        .expect("explicit restart");
    assert!(restarted.monitor_enabled);
    assert_eq!(
        restarted.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );

    let stale_after_restart = repo
        .upsert_pr_review_monitor(stale_disabled_callback)
        .await
        .expect("stale callback after restart");
    assert!(stale_after_restart.monitor_enabled);
    assert_eq!(
        stale_after_restart.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
    assert_eq!(
        stale_after_restart.last_seen_head_sha.as_deref(),
        Some("authoritative-head")
    );
    assert_eq!(
        stale_after_restart.last_reviewed_head_sha.as_deref(),
        Some("authoritative-reviewed-head")
    );
    assert_eq!(
        stale_after_restart.last_review_outcome.as_deref(),
        Some("authoritative-outcome")
    );
    assert_eq!(
        stale_after_restart.review_artifact_head_sha.as_deref(),
        Some("authoritative-artifact-head")
    );
    assert_eq!(stale_after_restart.review_artifact_version, Some(2));
}
