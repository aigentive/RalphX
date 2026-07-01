use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, WorkspaceReviewRuntimeSettings};
use crate::infrastructure::sqlite::run_migrations;
use crate::testing::SqliteTestDb;
use rusqlite::Connection;

fn setup_repo() -> (SqliteTestDb, SqliteWorkspaceReviewRuntimeSettingsRepository) {
    let db = SqliteTestDb::new("sqlite-workspace-review-runtime-settings-repo");
    let repo = SqliteWorkspaceReviewRuntimeSettingsRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn codex_settings(model: &str) -> WorkspaceReviewRuntimeSettings {
    WorkspaceReviewRuntimeSettings {
        model: Some(model.to_string()),
        effort: Some(LogicalEffort::High),
    }
}

#[test]
fn test_parse_datetime_accepts_stored_timestamp_formats() {
    let rfc3339 = parse_datetime("2026-07-01T12:34:56+00:00");
    assert_eq!(rfc3339.to_rfc3339(), "2026-07-01T12:34:56+00:00");

    let sqlite_naive = parse_datetime("2026-07-01 12:34:56");
    assert_eq!(sqlite_naive.to_rfc3339(), "2026-07-01T12:34:56+00:00");

    let before = chrono::Utc::now();
    let fallback = parse_datetime("not-a-date");
    assert!(fallback >= before);
}

#[tokio::test]
async fn test_new_constructor_uses_owned_connection() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    let repo = SqliteWorkspaceReviewRuntimeSettingsRepository::new(conn);

    repo.upsert_global(AgentHarnessKind::Codex, &codex_settings("gpt-5.4"))
        .await
        .unwrap();

    let row = repo
        .get_global(AgentHarnessKind::Codex)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.settings.model.as_deref(), Some("gpt-5.4"));
}

#[tokio::test]
async fn test_upsert_and_get_global_workspace_review_settings() {
    let (_db, repo) = setup_repo();

    repo.upsert_global(AgentHarnessKind::Codex, &codex_settings("gpt-5.4"))
        .await
        .unwrap();

    let row = repo
        .get_global(AgentHarnessKind::Codex)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.project_id, None);
    assert_eq!(row.provider, AgentHarnessKind::Codex);
    assert_eq!(row.settings.model.as_deref(), Some("gpt-5.4"));
}

#[tokio::test]
async fn test_missing_workspace_review_settings_return_none() {
    let (_db, repo) = setup_repo();

    assert!(repo
        .get_global(AgentHarnessKind::Codex)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .get_for_project("missing-project", AgentHarnessKind::Claude)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn test_upsert_and_get_project_workspace_review_settings() {
    let (_db, repo) = setup_repo();

    repo.upsert_for_project(
        "project-1",
        AgentHarnessKind::Codex,
        &codex_settings("gpt-5.4-mini"),
    )
    .await
    .unwrap();

    let row = repo
        .get_for_project("project-1", AgentHarnessKind::Codex)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.project_id.as_deref(), Some("project-1"));
    assert_eq!(row.settings.model.as_deref(), Some("gpt-5.4-mini"));
}

#[tokio::test]
async fn test_upsert_reuses_existing_global_row_id() {
    let (_db, repo) = setup_repo();

    let first = repo
        .upsert_global(AgentHarnessKind::Claude, &codex_settings("haiku"))
        .await
        .unwrap();
    let second = repo
        .upsert_global(AgentHarnessKind::Claude, &codex_settings("sonnet"))
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(second.settings.model.as_deref(), Some("sonnet"));
}

#[tokio::test]
async fn test_upsert_reuses_existing_project_row_id_and_allows_null_overrides() {
    let (_db, repo) = setup_repo();

    let first = repo
        .upsert_for_project(
            "project-1",
            AgentHarnessKind::Codex,
            &codex_settings("gpt-5.4-mini"),
        )
        .await
        .unwrap();
    let second = repo
        .upsert_for_project(
            "project-1",
            AgentHarnessKind::Codex,
            &WorkspaceReviewRuntimeSettings {
                model: None,
                effort: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(second.settings.model, None);
    assert_eq!(second.settings.effort, None);
}

#[tokio::test]
async fn test_list_scoped_workspace_review_settings() {
    let (_db, repo) = setup_repo();

    repo.upsert_global(AgentHarnessKind::Codex, &codex_settings("gpt-5.4"))
        .await
        .unwrap();
    repo.upsert_global(AgentHarnessKind::Claude, &codex_settings("haiku"))
        .await
        .unwrap();
    repo.upsert_for_project(
        "project-1",
        AgentHarnessKind::Claude,
        &WorkspaceReviewRuntimeSettings {
            model: Some("haiku".to_string()),
            effort: Some(LogicalEffort::Medium),
        },
    )
    .await
    .unwrap();
    repo.upsert_for_project(
        "project-2",
        AgentHarnessKind::Codex,
        &WorkspaceReviewRuntimeSettings {
            model: Some("gpt-5.4-mini".to_string()),
            effort: Some(LogicalEffort::Medium),
        },
    )
    .await
    .unwrap();

    let global_rows = repo.list_global().await.unwrap();
    assert_eq!(
        global_rows
            .iter()
            .map(|row| row.provider)
            .collect::<Vec<_>>(),
        vec![AgentHarnessKind::Claude, AgentHarnessKind::Codex]
    );

    let project_rows = repo.list_for_project("project-1").await.unwrap();
    assert_eq!(project_rows.len(), 1);
    assert_eq!(project_rows[0].project_id.as_deref(), Some("project-1"));
    assert_eq!(project_rows[0].provider, AgentHarnessKind::Claude);
}
