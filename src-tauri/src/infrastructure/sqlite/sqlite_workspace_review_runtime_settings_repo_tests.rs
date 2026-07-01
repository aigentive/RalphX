use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, WorkspaceReviewRuntimeSettings};
use crate::testing::SqliteTestDb;

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
async fn test_list_scoped_workspace_review_settings() {
    let (_db, repo) = setup_repo();

    repo.upsert_global(AgentHarnessKind::Codex, &codex_settings("gpt-5.4"))
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

    assert_eq!(repo.list_global().await.unwrap().len(), 1);
    assert_eq!(repo.list_for_project("project-1").await.unwrap().len(), 1);
}
