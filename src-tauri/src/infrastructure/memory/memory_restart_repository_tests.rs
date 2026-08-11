use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind, IdeationSessionId, PlanBranch,
    PlanBranchId, ProjectId,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, PlanBranchRepository};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryPlanBranchRepository,
};

#[tokio::test]
async fn memory_workspace_restore_reactivates_links_and_clears_cleanup_marker() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        ProjectId::new(),
        AgentConversationWorkspaceMode::Ideation,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/test/agent".to_string(),
        "/owned/test/worktree".to_string(),
    );
    workspace.status = AgentConversationWorkspaceStatus::Missing;
    repo.create_or_update(workspace).await.unwrap();
    repo.mark_local_cleanup_status(&conversation_id, "cleaned", chrono::Utc::now())
        .await
        .unwrap();
    let session_id = IdeationSessionId::new();
    let plan_branch_id = PlanBranchId::new();

    repo.restore_after_restart(&conversation_id, &session_id, &plan_branch_id)
        .await
        .unwrap();

    let restored = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(restored.status, AgentConversationWorkspaceStatus::Active);
    assert_eq!(restored.linked_plan_branch_id, Some(plan_branch_id));
    assert_eq!(restored.linked_ideation_session_id, Some(session_id));
    assert_eq!(
        repo.get_local_cleanup_status(&conversation_id)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn restore_after_restart_rejects_a_missing_workspace_in_memory() {
    let repo = MemoryAgentConversationWorkspaceRepository::new();
    let error = repo
        .restore_after_restart(
            &ChatConversationId::new(),
            &IdeationSessionId::new(),
            &PlanBranchId::new(),
        )
        .await
        .expect_err("restart repair must not succeed without a workspace row");

    assert!(error.to_string().contains("Workspace not found"));
}

#[tokio::test]
async fn memory_plan_branch_cleanup_marker_can_be_cleared_after_restart() {
    let repo = MemoryPlanBranchRepository::new();
    let branch = PlanBranch::new(
        ArtifactId::new(),
        IdeationSessionId::new(),
        ProjectId::new(),
        "ralphx/test/plan".to_string(),
        "main".to_string(),
    );
    let branch_id = branch.id.clone();
    repo.create(branch).await.unwrap();
    repo.mark_local_cleanup_status(&branch_id, "cleaned", chrono::Utc::now())
        .await
        .unwrap();

    assert_eq!(
        repo.get_local_cleanup_status(&branch_id).await.unwrap(),
        Some("cleaned".to_string())
    );
    repo.clear_local_cleanup_status(&branch_id).await.unwrap();
    assert_eq!(
        repo.get_local_cleanup_status(&branch_id).await.unwrap(),
        None
    );
}
