use super::sqlite_delegated_session_repo::SqliteDelegatedSessionRepository;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{DelegatedSession, DelegatedSessionId, Project};
use crate::domain::repositories::{DelegatedSessionRepository, ProjectRepository};
use crate::infrastructure::sqlite::SqliteProjectRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_delegated_session_repo_tests")
}

async fn create_project(db: &SqliteTestDb) -> crate::domain::entities::ProjectId {
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = Project::new(
        "Delegated Session Test Project".to_string(),
        "/tmp/ralphx-delegated-session-test".to_string(),
    );
    let project_id = project.id.clone();
    repo.create(project).await.unwrap();
    project_id
}

#[tokio::test]
async fn test_create_and_get_by_id() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

    let session = DelegatedSession::new(
        project_id,
        "task_execution",
        "task-1",
        "ralphx-execution-worker",
        AgentHarnessKind::Codex,
    );
    let id = session.id.clone();

    repo.create(session).await.unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.parent_context_type, "task_execution");
    assert_eq!(found.parent_context_id, "task-1");
    assert_eq!(found.harness, AgentHarnessKind::Codex);
    assert!(found.delegate_context_authorized);
    assert!(found.caller_conversation_id.is_none());
}

#[tokio::test]
async fn test_create_round_trips_delegate_context_authorization_and_caller_link() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;
    let mut session = DelegatedSession::new(
        project_id,
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
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

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
async fn test_get_by_parent_context_orders_latest_first() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

    let older = DelegatedSession::new(
        project_id.clone(),
        "review",
        "review-1",
        "ralphx-execution-reviewer",
        AgentHarnessKind::Claude,
    );
    let older_id = older.id.clone();
    repo.create(older).await.unwrap();
    repo.update_status(&older_id, "failed", Some("oops".to_string()), None)
        .await
        .unwrap();

    let newer = DelegatedSession::new(
        project_id,
        "review",
        "review-1",
        "ralphx-execution-reviewer",
        AgentHarnessKind::Codex,
    );
    let newer_id = newer.id.clone();
    repo.create(newer).await.unwrap();

    let sessions = repo
        .get_by_parent_context("review", "review-1")
        .await
        .unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].id, newer_id);
    assert_eq!(sessions[1].id, older_id);
}

#[tokio::test]
async fn test_update_runtime_fields() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());
    let project_id = create_project(&db).await;

    let mut session = DelegatedSession::new(
        project_id,
        "merge",
        "task-42",
        "ralphx-execution-merger",
        AgentHarnessKind::Codex,
    );
    session.caller_conversation_id = Some("original-caller".to_string());
    let id = session.id.clone();
    repo.create(session).await.unwrap();

    repo.update_job_identity(&id, "job-42".to_string(), Some("parent-run-42".to_string()))
        .await
        .unwrap();
    repo.update_provider_session_id(&id, Some("provider-42".to_string()))
        .await
        .unwrap();
    repo.update_status(&id, "completed", None, Some(chrono::Utc::now()))
        .await
        .unwrap();

    let found = repo.get_by_id(&id).await.unwrap().unwrap();
    assert_eq!(found.job_id.as_deref(), Some("job-42"));
    assert_eq!(found.parent_agent_run_id.as_deref(), Some("parent-run-42"));
    assert_eq!(
        found.caller_conversation_id.as_deref(),
        Some("original-caller")
    );
    assert_eq!(found.provider_session_id.as_deref(), Some("provider-42"));
    assert_eq!(found.status, "completed");
    assert!(found.completed_at.is_some());
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_missing_session() {
    let db = setup_test_db();
    let repo = SqliteDelegatedSessionRepository::from_shared(db.shared_conn());

    let missing = DelegatedSessionId::from_string("missing");
    assert!(repo.get_by_id(&missing).await.unwrap().is_none());
}
