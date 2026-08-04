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
    session.job_id = Some("delegation-job".to_string());
    session.parent_agent_run_id = Some("parent-run".to_string());
    let id = session.id.clone();

    repo.create(session).await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert!(!found.delegate_context_authorized);
    assert_eq!(
        found.caller_conversation_id.as_deref(),
        Some("caller-conversation")
    );
    assert_eq!(found.job_id.as_deref(), Some("delegation-job"));
    assert_eq!(found.parent_agent_run_id.as_deref(), Some("parent-run"));
}

#[tokio::test]
async fn test_list_active_by_caller_conversation_excludes_terminal_sessions() {
    let repo = MemoryDelegatedSessionRepository::new();
    let project_id = ProjectId::from_string("project-1".to_string());

    let mut active = DelegatedSession::new(
        project_id.clone(),
        "conversation",
        "parent-context",
        "ralphx-general-worker",
        AgentHarnessKind::Codex,
    );
    active.caller_conversation_id = Some("caller-conversation".to_string());
    let active_id = active.id.clone();
    repo.create(active).await.unwrap();

    for status in ["completed", "failed", "cancelled"] {
        let mut terminal = DelegatedSession::new(
            project_id.clone(),
            "conversation",
            "parent-context",
            "ralphx-general-worker",
            AgentHarnessKind::Codex,
        );
        terminal.caller_conversation_id = Some("caller-conversation".to_string());
        let terminal_id = terminal.id.clone();
        repo.create(terminal).await.unwrap();
        repo.update_status(&terminal_id, status, None, Some(chrono::Utc::now()))
            .await
            .unwrap();
    }

    let mut other_caller = DelegatedSession::new(
        project_id,
        "conversation",
        "parent-context",
        "ralphx-general-worker",
        AgentHarnessKind::Codex,
    );
    other_caller.caller_conversation_id = Some("another-caller".to_string());
    repo.create(other_caller).await.unwrap();

    let sessions = repo
        .list_active_by_caller_conversation("caller-conversation")
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, active_id);
    assert!(sessions[0].completed_at.is_none());
}

#[tokio::test]
async fn test_update_runtime_fields() {
    let repo = MemoryDelegatedSessionRepository::new();
    let mut session = DelegatedSession::new(
        ProjectId::from_string("project-1".to_string()),
        "task_execution",
        "task-1",
        "ralphx-execution-coder",
        AgentHarnessKind::Codex,
    );
    session.caller_conversation_id = Some("original-caller".to_string());
    let id = session.id.clone();
    repo.create(session).await.unwrap();

    repo.update_job_identity(
        &id,
        "job-123".to_string(),
        Some("parent-run-123".to_string()),
    )
    .await
    .unwrap();
    repo.update_provider_session_id(&id, Some("provider-123".to_string()))
        .await
        .unwrap();
    repo.update_status(&id, "completed", None, Some(chrono::Utc::now()))
        .await
        .unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.job_id.as_deref(), Some("job-123"));
    assert_eq!(found.parent_agent_run_id.as_deref(), Some("parent-run-123"));
    assert_eq!(
        found.caller_conversation_id.as_deref(),
        Some("original-caller")
    );
    assert_eq!(found.provider_session_id.as_deref(), Some("provider-123"));
    assert_eq!(found.status, "completed");
    assert!(found.completed_at.is_some());
}
