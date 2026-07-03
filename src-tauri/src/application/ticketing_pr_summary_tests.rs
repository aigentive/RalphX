use super::ticketing_pr_summary::{has_open_pr, open_pr_count, ticket_pr_branch_summary};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::infrastructure::memory::MemoryAgentConversationWorkspaceRepository;

fn workspace(
    project_id: &ProjectId,
    conversation_id: ChatConversationId,
    branch: &str,
    pr_number: Option<i64>,
    pr_status: Option<&str>,
) -> AgentConversationWorkspace {
    let worktree_path = format!("/tmp/{}", conversation_id.as_str());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        None,
        None,
        branch.to_string(),
        worktree_path,
    );
    workspace.publication_pr_number = pr_number;
    workspace.publication_pr_url = pr_number.map(|n| format!("https://github.com/x/y/pull/{n}"));
    workspace.publication_pr_status = pr_status.map(str::to_string);
    workspace
}

#[tokio::test]
async fn summarizes_only_linked_conversations_with_correct_open_state() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let project = ProjectId::from_string("11111111-1111-1111-1111-111111111111".to_string());

    let c_open = ChatConversationId::new();
    let c_draft = ChatConversationId::new();
    let c_merged = ChatConversationId::new();
    let c_branch = ChatConversationId::new();
    let c_unlinked = ChatConversationId::new();

    for ws in [
        workspace(&project, c_open, "ralphx/p/a1", Some(7), Some("open")),
        workspace(&project, c_draft, "ralphx/p/a2", Some(8), Some("draft")),
        workspace(&project, c_merged, "ralphx/p/a3", Some(9), Some("merged")),
        workspace(&project, c_branch, "ralphx/p/a4", None, None),
        // Not linked to the ticket — must be excluded even though it has an open PR.
        workspace(&project, c_unlinked, "ralphx/p/a5", Some(99), Some("open")),
    ] {
        repo.create_or_update(ws).await.expect("seed workspace");
    }

    let linked = [c_open, c_draft, c_merged, c_branch];
    let summaries = ticket_pr_branch_summary(&repo, &project, &linked)
        .await
        .expect("summary");

    assert_eq!(summaries.len(), 4, "unlinked conversation is excluded");
    let by_id = |id: &ChatConversationId| {
        summaries
            .iter()
            .find(|summary| summary.conversation_id == id.as_str())
            .expect("summary for linked conversation")
    };
    assert!(by_id(&c_open).is_open);
    assert!(by_id(&c_draft).is_open, "draft PR counts as open");
    assert!(!by_id(&c_merged).is_open, "merged PR is terminal");
    assert!(!by_id(&c_branch).is_open, "no PR is not open");
    assert_eq!(by_id(&c_open).pr_number, Some(7));
    assert_eq!(open_pr_count(&summaries), 2);
    assert!(has_open_pr(&summaries));
}

#[tokio::test]
async fn empty_conversation_ids_returns_empty_without_query() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let project = ProjectId::from_string("11111111-1111-1111-1111-111111111111".to_string());

    let summaries = ticket_pr_branch_summary(&repo, &project, &[])
        .await
        .expect("summary");

    assert!(summaries.is_empty());
    assert_eq!(open_pr_count(&summaries), 0);
    assert!(!has_open_pr(&summaries));
}
