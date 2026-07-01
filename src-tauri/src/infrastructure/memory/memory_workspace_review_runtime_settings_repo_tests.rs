use super::*;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, WorkspaceReviewRuntimeSettings};

fn codex_settings(model: &str) -> WorkspaceReviewRuntimeSettings {
    WorkspaceReviewRuntimeSettings {
        model: Some(model.to_string()),
        effort: Some(LogicalEffort::High),
    }
}

#[tokio::test]
async fn test_upsert_and_get_global_workspace_review_settings() {
    let repo = MemoryWorkspaceReviewRuntimeSettingsRepository::new();

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
    let repo = MemoryWorkspaceReviewRuntimeSettingsRepository::new();

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
async fn test_upsert_reuses_existing_row_id() {
    let repo = MemoryWorkspaceReviewRuntimeSettingsRepository::new();

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
async fn test_missing_reads_return_none() {
    let repo = MemoryWorkspaceReviewRuntimeSettingsRepository::new();

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
async fn test_lists_sort_and_filter_by_scope() {
    let repo = MemoryWorkspaceReviewRuntimeSettingsRepository::new();

    repo.upsert_global(AgentHarnessKind::Codex, &codex_settings("gpt-5.4"))
        .await
        .unwrap();
    repo.upsert_global(AgentHarnessKind::Claude, &codex_settings("haiku"))
        .await
        .unwrap();
    repo.upsert_for_project(
        "project-1",
        AgentHarnessKind::Codex,
        &codex_settings("gpt-5.4-mini"),
    )
    .await
    .unwrap();
    repo.upsert_for_project(
        "project-2",
        AgentHarnessKind::Claude,
        &codex_settings("sonnet"),
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
    assert_eq!(project_rows[0].provider, AgentHarnessKind::Codex);
}

#[tokio::test]
async fn test_project_upsert_reuses_existing_row_id() {
    let repo = MemoryWorkspaceReviewRuntimeSettingsRepository::new();

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
            &codex_settings("gpt-5.4"),
        )
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(second.settings.model.as_deref(), Some("gpt-5.4"));
}
