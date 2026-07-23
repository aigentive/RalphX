use crate::application::agent_workspace_merge_classification::{
    classify_merged_workspace_outcome, classify_merged_workspace_outcome_from_github,
    MergedWorkspaceOutcome,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::{GithubServiceTrait, PrStatus, PrSyncState};
use crate::error::AppError;
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;
use crate::tests::mock_github_service::MockGithubService;
use std::path::Path;
use std::sync::Arc;

fn merged_sync_state(head_ref_oid: Option<&str>, merge_commit_sha: Option<&str>) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Merged {
            merge_commit_sha: merge_commit_sha.map(str::to_string),
            merged_at: Some("2026-07-23T12:00:00Z".to_string()),
        },
        merge_state_status: None,
        mergeable: None,
        is_draft: false,
        head_ref_name: "feature/d4".to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: head_ref_oid.map(str::to_string),
        base_ref_oid: Some("base-sha".to_string()),
    }
}

#[test]
fn matching_authoritative_head_is_merged_clean() {
    assert_eq!(
        classify_merged_workspace_outcome(
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(&merged_sync_state(
                Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                Some("cccccccccccccccccccccccccccccccccccccccc"),
            )),
        ),
        MergedWorkspaceOutcome::Clean
    );
}

#[test]
fn changed_authoritative_head_is_merged_with_followups() {
    assert_eq!(
        classify_merged_workspace_outcome(
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(&merged_sync_state(
                Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            )),
        ),
        MergedWorkspaceOutcome::WithFollowups
    );
}

#[test]
fn squash_ambiguous_changed_head_stays_coarse() {
    assert_eq!(
        classify_merged_workspace_outcome(
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(&merged_sync_state(
                Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                Some("cccccccccccccccccccccccccccccccccccccccc"),
            )),
        ),
        MergedWorkspaceOutcome::Merged
    );
}

#[test]
fn absent_invalid_or_non_merged_evidence_stays_coarse() {
    assert_eq!(
        classify_merged_workspace_outcome(None, Some(&merged_sync_state(Some("head"), None))),
        MergedWorkspaceOutcome::Merged
    );
    assert_eq!(
        classify_merged_workspace_outcome(
            Some("stale"),
            Some(&merged_sync_state(
                Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
                Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            )),
        ),
        MergedWorkspaceOutcome::Merged
    );
    assert_eq!(
        classify_merged_workspace_outcome(
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            Some(&PrSyncState {
                status: PrStatus::Open,
                ..merged_sync_state(Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), None,)
            }),
        ),
        MergedWorkspaceOutcome::Merged
    );
}

#[tokio::test]
async fn github_read_failure_stays_coarse_through_the_production_classifier() {
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("merge-classification-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "feature/d4".to_string(),
        "/tmp/merge-classification".to_string(),
    );
    workspace.publication_pushed_sha = Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string());
    let workspace_repo: Arc<dyn AgentConversationWorkspaceRepository> =
        Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should be inserted");
    let github = Arc::new(MockGithubService::new());
    github.state().check_pr_sync_state_result =
        Some(Err(AppError::Infrastructure("gh unavailable".to_string())));
    let github_trait: Arc<dyn GithubServiceTrait> = github;

    let outcome = classify_merged_workspace_outcome_from_github(
        &workspace_repo,
        &github_trait,
        &conversation_id,
        Path::new("/tmp"),
        42,
    )
    .await;

    assert_eq!(outcome, MergedWorkspaceOutcome::Merged);
}
