use std::sync::Arc;

use crate::application::agent_workspace_publish_repair_state::{
    claim_agent_workspace_repair, complete_agent_workspace_repair_claim,
    current_agent_workspace_repair_claim_for_completion, reconcile_active_agent_workspace_repair,
    repair_event_authorizes_active_run, settle_agent_workspace_repair_failure,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, ChatConversationId,
    IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, AgentRunRepository};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
};

fn repair_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        ProjectId::from_string("repair-state-project".to_string()),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base".to_string()),
        "ralphx/repair-state".to_string(),
        "/tmp/ralphx-repair-state".to_string(),
    );
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace
}

#[tokio::test]
async fn claim_is_atomic_idempotent_and_stale_failure_cannot_downgrade_new_claim() {
    let repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-claim-1");
    repo.create_or_update(repair_workspace(conversation_id.clone()))
        .await
        .unwrap();

    let first = claim_agent_workspace_repair(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "Repair requested.",
        None,
    )
    .await
    .unwrap()
    .expect("first claim");
    assert!(claim_agent_workspace_repair(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "Duplicate repair.",
        None,
    )
    .await
    .unwrap()
    .is_none());
    assert!(settle_agent_workspace_repair_failure(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &first,
        "Dispatch failed.",
    )
    .await
    .unwrap());

    let second = claim_agent_workspace_repair(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &conversation_id,
        "Retry requested.",
        None,
    )
    .await
    .unwrap()
    .expect("second claim");
    assert!(!settle_agent_workspace_repair_failure(
        Arc::clone(&repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &first,
        "Late first failure.",
    )
    .await
    .unwrap());
    let current = repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.pr_supervision_status.as_deref(), Some("fixing"));
    assert_eq!(
        current.pr_supervision_updated_at,
        second.guard.pr_supervision_updated_at
    );
}

#[tokio::test]
async fn active_reconciliation_requires_current_successful_lifecycle_evidence() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-reconcile-1");
    let workspace = repair_workspace(conversation_id.clone());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let active_run = run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .unwrap();

    let mut old_event = AgentConversationWorkspacePublicationEvent::new(
        conversation_id.clone(),
        "repair_sent",
        "succeeded",
        "Old repair",
        Some("agent_fixable".to_string()),
    );
    old_event.created_at = active_run.started_at - chrono::Duration::seconds(1);
    assert!(!repair_event_authorizes_active_run(
        &[old_event.clone()],
        &active_run
    ));
    workspace_repo
        .append_publication_event(old_event)
        .await
        .unwrap();
    assert!(!reconcile_active_agent_workspace_repair(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &workspace,
    )
    .await
    .unwrap());

    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_sent",
            "succeeded",
            "Current repair",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();
    assert!(reconcile_active_agent_workspace_repair(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &workspace,
    )
    .await
    .unwrap());
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .unwrap()
            .unwrap()
            .pr_supervision_status
            .as_deref(),
        Some("fixing")
    );
}

#[tokio::test]
async fn stale_completion_claim_cannot_overwrite_a_failed_attempt() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = ChatConversationId::from_string("repair-completion-1");
    let mut workspace = repair_workspace(conversation_id.clone());
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace.pr_supervision_updated_at = Some(chrono::Utc::now());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .unwrap();
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "repair_sent",
            "succeeded",
            "Current repair",
            Some("agent_fixable".to_string()),
        ))
        .await
        .unwrap();
    let claim = current_agent_workspace_repair_claim_for_completion(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&run_repo) as Arc<dyn AgentRunRepository>,
        &workspace,
    )
    .await
    .unwrap()
    .expect("current completion claim");
    assert!(settle_agent_workspace_repair_failure(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "Dispatch failed",
    )
    .await
    .unwrap());
    assert!(!complete_agent_workspace_repair_claim(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        &claim,
        "new-base",
        Some("monitoring"),
        Some("Repair completed"),
    )
    .await
    .unwrap());

    let current = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(current.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(current.base_commit.as_deref(), Some("base"));
}
