use super::agent_workspace_terminal_observation::{
    classify_merged_workspace_observation, MERGED_CLEAN_STATUS, MERGED_WITH_FOLLOWUPS_STATUS,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::services::{PrStatus, PrSyncState};

fn workspace() -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-cleanliness"),
        ProjectId::from_string("project-cleanliness".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/project/workspace".to_string(),
        "/tmp/unused-workspace".to_string(),
    );
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_pushed_sha = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    workspace
}

fn merged_sync(head_sha: Option<&str>) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Merged {
            merge_commit_sha: Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string()),
            merged_at: Some("2026-07-27T00:00:00Z".to_string()),
        },
        merge_state_status: None,
        mergeable: None,
        is_draft: false,
        head_ref_name: "ralphx/project/workspace".to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: head_sha.map(str::to_string),
        base_ref_oid: Some("cccccccccccccccccccccccccccccccccccccccc".to_string()),
    }
}

#[test]
fn exact_remote_head_proves_merged_clean_without_using_merge_commit_sha() {
    assert_eq!(
        classify_merged_workspace_observation(
            &workspace(),
            42,
            &merged_sync(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
        ),
        MERGED_CLEAN_STATUS
    );
}

#[test]
fn changed_remote_head_proves_merged_with_followups() {
    assert_eq!(
        classify_merged_workspace_observation(
            &workspace(),
            42,
            &merged_sync(Some("dddddddddddddddddddddddddddddddddddddddd")),
        ),
        MERGED_WITH_FOLLOWUPS_STATUS
    );
}

#[test]
fn missing_or_stale_authority_never_upgrades_plain_merged() {
    let mut wrong_pr = workspace();
    wrong_pr.publication_pr_number = Some(41);
    let mut wrong_head = merged_sync(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    wrong_head.head_ref_name = "other-branch".to_string();
    let mut wrong_base = merged_sync(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
    wrong_base.base_ref_name = "release".to_string();
    let mut malformed = workspace();
    malformed.publication_pushed_sha = Some("not-a-sha".to_string());

    assert_eq!(
        classify_merged_workspace_observation(&workspace(), 42, &merged_sync(None)),
        "merged"
    );
    assert_eq!(
        classify_merged_workspace_observation(
            &wrong_pr,
            42,
            &merged_sync(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
        ),
        "merged"
    );
    assert_eq!(
        classify_merged_workspace_observation(&workspace(), 42, &wrong_head),
        "merged"
    );
    assert_eq!(
        classify_merged_workspace_observation(&workspace(), 42, &wrong_base),
        "merged"
    );
    assert_eq!(
        classify_merged_workspace_observation(
            &malformed,
            42,
            &merged_sync(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
        ),
        "merged"
    );
}
