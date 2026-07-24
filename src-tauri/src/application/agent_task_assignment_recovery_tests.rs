use std::sync::Arc;

use super::AgentTaskAssignmentRecoveryService;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentTaskCreate, AgentTaskScope, AgentTaskState, ChatConversation,
    DelegatedSession, ProjectId,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentTaskRepository, ChatConversationRepository, DelegatedSessionRepository,
};
use crate::domain::services::running_agent_registry::MemoryRunningAgentRegistry;
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryAgentTaskRepository, MemoryChatConversationRepository,
    MemoryDelegatedSessionRepository,
};

#[tokio::test]
async fn recovery_releases_orphaned_reservations_and_bound_running_attempts() {
    let task_repo = Arc::new(MemoryAgentTaskRepository::new());
    let session_repo = Arc::new(MemoryDelegatedSessionRepository::new());
    let conversation_repo = Arc::new(MemoryChatConversationRepository::new());
    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());
    let scope = AgentTaskScope::new("conversation", "parent-conversation");
    for title in ["Reserved task", "Bound task", "Sibling"] {
        task_repo
            .create_task(
                &scope,
                AgentTaskCreate {
                    title: title.to_string(),
                    details: title.to_string(),
                    active_label: None,
                    owner_agent: Some("orchestrator".to_string()),
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .unwrap();
    }
    let session_one = session_repo
        .create(DelegatedSession::new(
            ProjectId::new(),
            "project".to_string(),
            "project-1".to_string(),
            "worker".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .unwrap();
    let session_two = session_repo
        .create(DelegatedSession::new(
            ProjectId::new(),
            "project".to_string(),
            "project-1".to_string(),
            "worker".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .unwrap();
    task_repo
        .reserve_assignment(&scope, "1", &session_one.id, &AgentRunId::new(), "worker")
        .await
        .unwrap();
    let session_two_reservation = task_repo
        .reserve_assignment(&scope, "2", &session_two.id, &AgentRunId::new(), "worker")
        .await
        .unwrap()
        .unwrap();
    let conversation = conversation_repo
        .create(ChatConversation::new_delegation(session_two.id.clone()))
        .await
        .unwrap();
    let run = run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .unwrap();
    task_repo
        .bind_assignment_run(
            &session_two_reservation.assignment.assignment.id,
            &session_two.id,
            &run.id,
        )
        .await
        .unwrap();

    let service = AgentTaskAssignmentRecoveryService::new(
        task_repo.clone(),
        session_repo,
        conversation_repo,
        run_repo,
        running_registry,
    );
    let report = service.recover().await.unwrap();
    assert_eq!(report.inspected, 2);
    assert_eq!(report.settled, 2);
    assert_eq!(report.retained_running, 0);
    assert_eq!(
        task_repo
            .get_task(&scope, "1")
            .await
            .unwrap()
            .unwrap()
            .state,
        AgentTaskState::Open
    );
    assert_eq!(
        task_repo
            .get_task(&scope, "2")
            .await
            .unwrap()
            .unwrap()
            .state,
        AgentTaskState::Open
    );
}
