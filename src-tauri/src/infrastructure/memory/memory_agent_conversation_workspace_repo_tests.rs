use super::MemoryAgentConversationWorkspaceRepository;
use crate::domain::entities::{
    AgentWorkspacePrReviewAction, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor, ChatConversationId,
    ProjectId,
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
