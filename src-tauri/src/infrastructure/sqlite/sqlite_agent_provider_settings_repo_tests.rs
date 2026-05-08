use chrono::Utc;
use rusqlite::Connection;

use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings, LogicalEffort};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::infrastructure::sqlite::run_migrations;
use crate::testing::SqliteTestDb;

use super::{
    fetch_many, fetch_optional, parse_datetime, select_columns,
    SqliteAgentProviderSettingsRepository,
};

fn setup_repo() -> (SqliteTestDb, SqliteAgentProviderSettingsRepository) {
    let db = SqliteTestDb::new("sqlite-agent-provider-settings-repo");
    let repo = SqliteAgentProviderSettingsRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn upsert_and_get_provider_settings() {
    let (_db, repo) = setup_repo();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    settings.enabled = true;
    settings.is_default = true;

    repo.upsert(&settings).await.unwrap();
    let row = repo
        .get(AgentHarnessKind::Codex)
        .await
        .unwrap()
        .expect("codex settings");

    assert!(row.enabled);
    assert!(row.is_default);
    assert_eq!(row.model.as_deref(), Some("gpt-5.5"));
    assert_eq!(row.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn upsert_clears_prior_default_provider() {
    let (_db, repo) = setup_repo();
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.is_default = true;
    repo.upsert(&claude).await.unwrap();

    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.unwrap();

    let default = repo.get_default().await.unwrap().expect("default provider");
    let claude = repo.get(AgentHarnessKind::Claude).await.unwrap().unwrap();

    assert_eq!(default.provider, AgentHarnessKind::Codex);
    assert!(!claude.is_default);
}

#[tokio::test]
async fn list_returns_stored_provider_settings() {
    let (_db, repo) = setup_repo();
    let mut claude = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    claude.enabled = true;
    claude.effort = Some(LogicalEffort::High);
    repo.upsert(&claude).await.unwrap();

    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    codex.is_default = true;
    repo.upsert(&codex).await.unwrap();

    let rows = repo.list().await.unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].provider, AgentHarnessKind::Claude);
    assert_eq!(rows[0].effort, Some(LogicalEffort::High));
    assert_eq!(rows[1].provider, AgentHarnessKind::Codex);
    assert!(rows[1].is_default);
}

#[tokio::test]
async fn get_default_returns_none_without_default_row() {
    let (_db, repo) = setup_repo();
    let mut codex = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex);
    codex.enabled = true;
    repo.upsert(&codex).await.unwrap();

    assert!(repo.get_default().await.unwrap().is_none());
}

#[tokio::test]
async fn new_repository_constructor_uses_owned_connection() {
    let db = SqliteTestDb::new("sqlite-agent-provider-settings-owned-conn");
    let repo = SqliteAgentProviderSettingsRepository::new(db.new_connection());

    assert!(repo.list().await.unwrap().is_empty());
}

#[test]
fn parses_rfc3339_legacy_and_invalid_datetimes() {
    let rfc3339 = parse_datetime("2026-05-08T10:30:00+00:00");
    let legacy = parse_datetime("2026-05-08 10:30:00");
    let before_invalid = Utc::now();
    let invalid = parse_datetime("not a datetime");
    let after_invalid = Utc::now();

    assert_eq!(rfc3339, legacy);
    assert!(invalid >= before_invalid);
    assert!(invalid <= after_invalid);
}

#[test]
fn low_level_fetch_helpers_map_rows_and_missing_rows() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO agent_provider_settings (
            provider, enabled, is_default, model, effort, approval_policy,
            sandbox_mode, claude_permission_mode,
            claude_dangerously_skip_permissions,
            claude_allow_dangerously_skip_permissions, updated_at
        ) VALUES (?1, 1, 1, ?2, ?3, ?4, ?5, ?6, 1, 0, ?7)",
        rusqlite::params![
            AgentHarnessKind::Claude.to_string(),
            "sonnet",
            "high",
            "auto",
            "workspace-write",
            "bypassPermissions",
            "2026-05-08 10:30:00",
        ],
    )
    .unwrap();

    let selected = fetch_optional(
        &conn,
        &format!(
            "SELECT {} FROM agent_provider_settings WHERE provider = ?1",
            select_columns()
        ),
        rusqlite::params![AgentHarnessKind::Claude.to_string()],
    )
    .unwrap()
    .expect("provider row");
    let missing = fetch_optional(
        &conn,
        &format!(
            "SELECT {} FROM agent_provider_settings WHERE provider = ?1",
            select_columns()
        ),
        rusqlite::params![AgentHarnessKind::Codex.to_string()],
    )
    .unwrap();
    let rows = fetch_many(
        &conn,
        &format!(
            "SELECT {} FROM agent_provider_settings ORDER BY provider",
            select_columns()
        ),
        [],
    )
    .unwrap();

    assert_eq!(selected.provider, AgentHarnessKind::Claude);
    assert_eq!(selected.effort, Some(LogicalEffort::High));
    assert_eq!(
        selected.claude_permission_mode.as_deref(),
        Some("bypassPermissions")
    );
    assert!(selected.claude_dangerously_skip_permissions);
    assert!(!selected.claude_allow_dangerously_skip_permissions);
    assert!(missing.is_none());
    assert_eq!(rows.len(), 1);
}

#[test]
fn low_level_fetch_helpers_return_database_errors_for_invalid_rows() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    conn.execute(
        "INSERT INTO agent_provider_settings (
            provider, enabled, is_default, model, effort, approval_policy,
            sandbox_mode, claude_permission_mode,
            claude_dangerously_skip_permissions,
            claude_allow_dangerously_skip_permissions, updated_at
        ) VALUES (?1, 1, 0, NULL, ?2, NULL, NULL, NULL, 0, 0, ?3)",
        rusqlite::params!["claude", "not-effort", "2026-05-08 10:30:00"],
    )
    .unwrap();

    let optional_error = fetch_optional(
        &conn,
        &format!(
            "SELECT {} FROM agent_provider_settings WHERE provider = ?1",
            select_columns()
        ),
        rusqlite::params![AgentHarnessKind::Claude.to_string()],
    )
    .expect_err("invalid effort should map to database error");
    let many_error = fetch_many(
        &conn,
        &format!(
            "SELECT {} FROM agent_provider_settings ORDER BY provider",
            select_columns()
        ),
        [],
    )
    .expect_err("invalid effort should map to database error");

    assert!(optional_error.to_string().contains("not-effort"));
    assert!(many_error.to_string().contains("not-effort"));
}
