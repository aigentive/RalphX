use crate::domain::entities::{
    canonicalize_agent_conversation_issue, AgentConversationIssue,
    AgentConversationIssueCanonicalIdentity, AgentConversationIssueCanonicalInput,
    AgentConversationIssueOccurrence, ChatConversationId, ProjectId,
    AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW, AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED,
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

fn canonical_identity(fingerprint: &str) -> AgentConversationIssueCanonicalIdentity {
    AgentConversationIssueCanonicalIdentity {
        fingerprint: fingerprint.to_string(),
        scope_kind: "project".to_string(),
        scope_subject: "frontend-package".to_string(),
        family: "setup".to_string(),
        candidate_match_eligible: true,
    }
}

#[test]
fn entity_refresh_and_occurrence_snapshot_cover_identity_helpers() {
    let conversation_id = ChatConversationId::from_string("conversation-entity-1");
    let mut issue = make_issue(conversation_id);
    issue.apply_canonical_identity(&canonical_identity(
        "v1:setup:project:frontend-package:missing-frontend-dependency",
    ));
    issue.status = AGENT_CONVERSATION_ISSUE_STATUS_RESOLVED.to_string();
    issue.resolved_at = Some(chrono::Utc::now());

    let mut incoming = make_issue(ChatConversationId::from_string("conversation-entity-1"));
    incoming.severity = "critical".to_string();
    incoming.blocking_scope = "current_task".to_string();
    incoming.title = "Updated setup blocker".to_string();
    incoming.summary = "Updated summary from a later agent report.".to_string();
    incoming.evidence = Some("updated evidence".to_string());
    incoming.recommendation = Some("reuse the existing issue".to_string());
    incoming.blocker_fingerprint = Some("frontend-package:missing-node-modules".to_string());
    incoming.followup_title = Some("Updated follow-up".to_string());
    incoming.followup_prompt = Some("Updated prompt".to_string());
    incoming.auto_followup_eligible = false;
    incoming.apply_canonical_identity(&canonical_identity(
        "v1:setup:project:frontend-package:missing-node-modules",
    ));

    issue.refresh_from(incoming);

    assert_eq!(issue.severity, "critical");
    assert_eq!(issue.blocking_scope, "current_task");
    assert_eq!(issue.status, AGENT_CONVERSATION_ISSUE_STATUS_OPEN);
    assert!(issue.resolved_at.is_none());
    assert_eq!(
        issue.canonical_fingerprint.as_deref(),
        Some("v1:setup:project:frontend-package:missing-node-modules")
    );
    assert_eq!(issue.canonical_scope_kind.as_deref(), Some("project"));
    assert_eq!(
        issue.canonical_scope_subject.as_deref(),
        Some("frontend-package")
    );
    assert_eq!(issue.canonical_family.as_deref(), Some("setup"));
    assert_eq!(issue.followup_title.as_deref(), Some("Updated follow-up"));
    assert!(!issue.auto_followup_eligible);

    let occurrence = AgentConversationIssueOccurrence::from_issue(
        &issue,
        AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW,
    );
    assert_eq!(occurrence.issue_id, issue.id);
    assert_eq!(occurrence.project_id.as_str(), issue.project_id.as_str());
    assert_eq!(occurrence.conversation_id, issue.conversation_id);
    assert_eq!(occurrence.source_task_id.as_deref(), Some("task-1"));
    assert_eq!(occurrence.title, "Updated setup blocker");
    assert_eq!(
        occurrence.canonical_fingerprint.as_deref(),
        Some("v1:setup:project:frontend-package:missing-node-modules")
    );
    assert_eq!(
        occurrence.dedupe_decision.as_deref(),
        Some(AGENT_CONVERSATION_ISSUE_DEDUPE_CONFIRMED_NEW)
    );
}

#[test]
fn canonical_identity_covers_merge_hook_scope_and_unknown_fallbacks() {
    let merge_hook = canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
        issue_kind: "environment",
        blocking_scope: "project",
        title: "Merge hook environment failed",
        summary: "The merge hook could not find the required binary on PATH.",
        evidence: Some("merge hook: command not found"),
        recommendation: Some("fix the merge hook environment"),
        blocker_fingerprint: None,
        source_task_id: None,
    });
    assert_eq!(
        merge_hook.fingerprint,
        "v1:merge-hook:project:merge-hook-environment:environment-failure"
    );
    assert!(merge_hook.candidate_match_eligible);

    let task_fallback =
        canonicalize_agent_conversation_issue(&AgentConversationIssueCanonicalInput {
            issue_kind: "",
            blocking_scope: "merge",
            title: "!!!",
            summary: "\u{1b}[31mNew issue\u{1b}[0m with non-slug title",
            evidence: None,
            recommendation: None,
            blocker_fingerprint: None,
            source_task_id: None,
        });
    assert_eq!(
        task_fallback.fingerprint,
        "v1:unknown:task:unknown-task:unknown:untitled"
    );
    assert_eq!(task_fallback.scope_kind, "task");
    assert_eq!(task_fallback.scope_subject, "unknown-task");
    assert!(!task_fallback.candidate_match_eligible);
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
async fn canonical_lookup_candidates_and_occurrences_round_trip() {
    let db = SqliteTestDb::new("sqlite_agent_conversation_issue_repo_identity_tests");
    let repo = SqliteAgentConversationIssueRepository::from_shared(db.shared_conn());
    let conversation_id = ChatConversationId::from_string("conversation-identity-1");

    let mut exact_issue = make_issue(conversation_id.clone());
    exact_issue.apply_canonical_identity(&canonical_identity(
        "v1:setup:project:frontend-package:missing-frontend-dependency",
    ));
    let exact_issue = repo.save(&exact_issue).await.unwrap();

    let found = repo
        .find_open_by_canonical_fingerprint(
            &conversation_id,
            "v1:setup:project:frontend-package:missing-frontend-dependency",
        )
        .await
        .unwrap()
        .expect("canonical lookup should find exact issue");
    assert_eq!(found.id, exact_issue.id);

    let mut candidate_issue = make_issue(conversation_id.clone());
    candidate_issue.apply_canonical_identity(&canonical_identity(
        "v1:setup:project:frontend-package:other-setup-failure",
    ));
    candidate_issue.title = "Other setup failure".to_string();
    let candidate_issue = repo.save(&candidate_issue).await.unwrap();

    let candidates = repo
        .list_open_candidates_by_identity(
            &conversation_id,
            "project",
            "frontend-package",
            "setup",
            "v1:setup:project:frontend-package:missing-frontend-dependency",
            5,
        )
        .await
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, candidate_issue.id);

    let occurrence = AgentConversationIssueOccurrence::from_issue(
        &exact_issue,
        AGENT_CONVERSATION_ISSUE_DEDUPE_CREATED,
    );
    let occurrence = repo.append_occurrence(&occurrence).await.unwrap();
    let occurrences = repo
        .list_occurrences_by_issue(&exact_issue.id)
        .await
        .unwrap();
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0].id, occurrence.id);
    assert_eq!(occurrences[0].issue_id, exact_issue.id);
    assert_eq!(
        occurrences[0].canonical_fingerprint.as_deref(),
        Some("v1:setup:project:frontend-package:missing-frontend-dependency")
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
