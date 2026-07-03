use crate::domain::entities::{
    AgentConversationIssue, ChatConversationId, ProjectId,
    AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED, AGENT_CONVERSATION_ISSUE_STATUS_OPEN,
    AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED,
};
use crate::domain::repositories::AgentConversationIssueRepository;
use crate::testing::SqliteTestDb;

use super::SqliteAgentConversationIssueRepository;

fn make_issue(conversation_id: ChatConversationId) -> AgentConversationIssue {
    AgentConversationIssue::new(
        ProjectId::from_string("project-1".to_string()),
        conversation_id,
        Some("task-1".to_string()),
        Some("review".to_string()),
        Some("task-1".to_string()),
        Some("ralphx-execution-reviewer".to_string()),
        "plan_drift".to_string(),
        "high".to_string(),
        "followup_only".to_string(),
        "Plan drift found".to_string(),
        "Reviewer found unrelated work outside the accepted plan.".to_string(),
        Some("Touched unrelated file src/unrelated.rs".to_string()),
        Some("Create a follow-up branch for the unrelated failure.".to_string()),
        Some("scope-drift:task-1:src/unrelated.rs".to_string()),
        Some("Investigate unrelated failure".to_string()),
        Some("Inspect and plan the unrelated failure separately.".to_string()),
        true,
    )
}

#[tokio::test]
async fn issue_lifecycle_round_trips() {
    let db = SqliteTestDb::new("sqlite_agent_conversation_issue_repo_tests");
    let repo = SqliteAgentConversationIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let issue = repo
        .save(&make_issue(conversation_id.clone()))
        .await
        .unwrap();

    let loaded = repo
        .find_open_by_fingerprint(
            &conversation_id,
            Some("task-1"),
            "plan_drift",
            "scope-drift:task-1:src/unrelated.rs",
        )
        .await
        .unwrap()
        .expect("issue should be found by dedupe fingerprint");
    assert_eq!(loaded.id, issue.id);
    assert_eq!(loaded.status, AGENT_CONVERSATION_ISSUE_STATUS_OPEN);

    let followup_id = ChatConversationId::from_string("followup-conversation-1");
    let linked = repo
        .link_followup_conversation(&issue.id, &followup_id)
        .await
        .unwrap()
        .expect("issue should still exist");
    assert_eq!(linked.linked_followup_conversation_id, Some(followup_id));

    repo.update_status(&issue.id, AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED)
        .await
        .unwrap();
    let open_issues = repo
        .list_by_conversation(&conversation_id, false)
        .await
        .unwrap();
    assert!(open_issues.is_empty());

    let all_issues = repo
        .list_by_conversation(&conversation_id, true)
        .await
        .unwrap();
    assert_eq!(all_issues.len(), 1);
    assert_eq!(
        all_issues[0].status,
        AGENT_CONVERSATION_ISSUE_STATUS_DISMISSED
    );
}

#[tokio::test]
async fn issue_status_reopen_and_missing_mutations_round_trip() {
    let db = SqliteTestDb::new("sqlite_agent_conversation_issue_repo_edge_tests");
    let repo = SqliteAgentConversationIssueRepository::new(db.new_connection());
    let conversation_id = ChatConversationId::from_string("conversation-1");
    let issue = repo
        .save(&make_issue(conversation_id.clone()))
        .await
        .unwrap();

    let resolved = repo
        .update_status(&issue.id, AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED)
        .await
        .unwrap()
        .expect("issue should exist");
    assert_eq!(resolved.status, AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED);
    assert!(resolved.resolved_at.is_some());

    let reopened = repo
        .update_status(&issue.id, AGENT_CONVERSATION_ISSUE_STATUS_OPEN)
        .await
        .unwrap()
        .expect("issue should still exist");
    assert_eq!(reopened.status, AGENT_CONVERSATION_ISSUE_STATUS_OPEN);
    assert!(reopened.resolved_at.is_none());

    assert!(repo.get_by_id("missing").await.unwrap().is_none());
    assert!(repo
        .update_status("missing", AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .link_followup_conversation("missing", &ChatConversationId::from_string("followup-1"))
        .await
        .unwrap()
        .is_none());

    let missing_fingerprint = repo
        .find_open_by_fingerprint(
            &conversation_id,
            Some("other-task"),
            "plan_drift",
            "scope-drift:task-1:src/unrelated.rs",
        )
        .await
        .unwrap();
    assert!(missing_fingerprint.is_none());
}
