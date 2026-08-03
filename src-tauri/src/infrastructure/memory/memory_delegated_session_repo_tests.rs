use super::*;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{DelegatedSession, ProjectId};
use crate::domain::repositories::DelegatedSessionRepository;

#[tokio::test]
async fn test_create_and_get_by_id() {
    let repo = MemoryDelegatedSessionRepository::new();
    let session = DelegatedSession::new(
        ProjectId::from_string("project-1".to_string()),
        "review",
        "review-1",
        "ralphx-execution-reviewer",
        AgentHarnessKind::Codex,
    );
    let id = session.id.clone();

    repo.create(session).await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.parent_context_type, "review");
    assert_eq!(found.parent_context_id, "review-1");
    assert!(found.delegate_context_authorized);
    assert!(found.caller_conversation_id.is_none());
}

#[tokio::test]
async fn test_create_round_trips_delegate_context_authorization_and_caller_link() {
    let repo = MemoryDelegatedSessionRepository::new();
    let mut session = DelegatedSession::new(
        ProjectId::from_string("project-1".to_string()),
        "conversation",
        "delegated-conversation",
        "ralphx-general-worker",
        AgentHarnessKind::Codex,
    );
    session.delegate_context_authorized = false;
    session.caller_conversation_id = Some("caller-conversation".to_string());
    let id = session.id.clone();

    repo.create(session).await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert!(!found.delegate_context_authorized);
    assert_eq!(
        found.caller_conversation_id.as_deref(),
        Some("caller-conversation")
    );
}

#[tokio::test]
async fn test_update_runtime_fields() {
    let repo = MemoryDelegatedSessionRepository::new();
    let session = DelegatedSession::new(
        ProjectId::from_string("project-1".to_string()),
        "task_execution",
        "task-1",
        "ralphx-execution-coder",
        AgentHarnessKind::Codex,
    );
    let id = session.id.clone();
    repo.create(session).await.unwrap();

    repo.update_provider_session_id(&id, Some("provider-123".to_string()))
        .await
        .unwrap();
    repo.update_status(&id, "completed", None, Some(chrono::Utc::now()))
        .await
        .unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.provider_session_id.as_deref(), Some("provider-123"));
    assert_eq!(found.status, "completed");
    assert!(found.completed_at.is_some());
}
