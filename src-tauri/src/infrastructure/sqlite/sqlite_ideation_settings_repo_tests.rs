use super::*;
use crate::testing::SqliteTestDb;
use rusqlite::Connection;

fn setup_tasks_authorization_db() -> Connection {
    let conn = Connection::open_in_memory().expect("open authorization test database");
    conn.execute_batch(
        "
        CREATE TABLE ideation_settings (
            id INTEGER PRIMARY KEY,
            plan_mode TEXT NOT NULL DEFAULT 'optional',
            require_plan_approval INTEGER NOT NULL DEFAULT 0,
            suggest_plans_for_complex INTEGER NOT NULL DEFAULT 1,
            auto_link_proposals INTEGER NOT NULL DEFAULT 1,
            require_verification_for_accept INTEGER NOT NULL DEFAULT 0,
            require_verification_for_proposals INTEGER NOT NULL DEFAULT 0,
            require_accept_for_finalize INTEGER,
            ext_require_verification_for_accept INTEGER,
            ext_require_verification_for_proposals INTEGER,
            ext_require_accept_for_finalize INTEGER,
            auto_verify_plans INTEGER NOT NULL DEFAULT 0,
            ext_auto_verify_plans INTEGER,
            tasks_enabled INTEGER NOT NULL DEFAULT 0
        );
        INSERT INTO ideation_settings (id, tasks_enabled) VALUES (1, 0);
        CREATE TABLE ideation_sessions (id TEXT PRIMARY KEY, project_id TEXT NOT NULL);
        CREATE TABLE agent_conversation_workspaces (
            conversation_id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            task_pipeline_session_id TEXT,
            status TEXT NOT NULL
        );
        ",
    )
    .expect("create authorization test tables");
    conn
}

#[tokio::test]
async fn test_get_default_settings() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-default");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    let settings = repo.get_settings().await.unwrap();
    assert!(!settings.tasks_enabled);
    assert_eq!(settings.plan_mode, IdeationPlanMode::Optional);
    assert!(!settings.require_plan_approval);
    assert!(settings.suggest_plans_for_complex);
    assert!(settings.auto_link_proposals);
    assert!(!settings.require_verification_for_accept);
    assert!(!settings.require_verification_for_proposals);
}

#[tokio::test]
async fn test_update_settings() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-update");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    let new_settings = IdeationSettings {
        tasks_enabled: true,
        plan_mode: IdeationPlanMode::Required,
        require_plan_approval: true,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        ..Default::default()
    };

    let updated = repo.update_settings(&new_settings).await.unwrap();
    assert_eq!(updated.plan_mode, IdeationPlanMode::Required);
    assert!(updated.tasks_enabled);
    assert!(updated.require_plan_approval);
    assert!(!updated.suggest_plans_for_complex);
    assert!(!updated.auto_link_proposals);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.plan_mode, IdeationPlanMode::Required);
    assert!(retrieved.tasks_enabled);
    assert!(retrieved.require_plan_approval);
    assert!(!retrieved.suggest_plans_for_complex);
    assert!(!retrieved.auto_link_proposals);
}

#[tokio::test]
async fn test_update_settings_all_modes() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-all-modes");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    // Test Required mode
    let required_settings = IdeationSettings {
        plan_mode: IdeationPlanMode::Required,
        ..Default::default()
    };
    repo.update_settings(&required_settings).await.unwrap();
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.plan_mode, IdeationPlanMode::Required);

    // Test Optional mode
    let optional_settings = IdeationSettings {
        plan_mode: IdeationPlanMode::Optional,
        ..Default::default()
    };
    repo.update_settings(&optional_settings).await.unwrap();
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.plan_mode, IdeationPlanMode::Optional);

    // Test Parallel mode
    let parallel_settings = IdeationSettings {
        plan_mode: IdeationPlanMode::Parallel,
        ..Default::default()
    };
    repo.update_settings(&parallel_settings).await.unwrap();
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.plan_mode, IdeationPlanMode::Parallel);
}

// ─── from_shared ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_from_shared_returns_defaults() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-shared");
    let shared = db.shared_conn();
    let repo = SqliteIdeationSettingsRepository::from_shared(Arc::clone(&shared));

    let settings = repo.get_settings().await.unwrap();
    assert_eq!(settings.plan_mode, IdeationPlanMode::Optional);
    assert!(!settings.require_plan_approval);
}

// ─── fallback when no row ────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_settings_fallback_when_no_row() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-fallback");
    // Remove the default row (if any) seeded by migrations
    db.with_connection(|conn| {
        conn.execute("DELETE FROM ideation_settings", []).unwrap();
    });
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    let settings = repo.get_settings().await.unwrap();
    // Must return defaults without error
    assert_eq!(settings.plan_mode, IdeationPlanMode::Optional);
    assert!(!settings.require_plan_approval);
    assert!(settings.suggest_plans_for_complex);
    assert!(settings.auto_link_proposals);
}

// ─── second update overrides first ───────────────────────────────────────────

#[tokio::test]
async fn test_update_overrides_previous_update() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-override");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    repo.update_settings(&IdeationSettings {
        plan_mode: IdeationPlanMode::Required,
        require_plan_approval: true,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        ..Default::default()
    })
    .await
    .unwrap();

    repo.update_settings(&IdeationSettings {
        plan_mode: IdeationPlanMode::Parallel,
        require_plan_approval: false,
        suggest_plans_for_complex: true,
        auto_link_proposals: true,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        ..Default::default()
    })
    .await
    .unwrap();

    let s = repo.get_settings().await.unwrap();
    assert_eq!(s.plan_mode, IdeationPlanMode::Parallel);
    assert!(!s.require_plan_approval);
    assert!(s.suggest_plans_for_complex);
    assert!(s.auto_link_proposals);
}

// ─── boolean fields toggle independently ────────────────────────────────────

#[tokio::test]
async fn test_boolean_fields_toggle_independently() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-boolean-toggle");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    // Enable only require_plan_approval, disable the rest
    repo.update_settings(&IdeationSettings {
        plan_mode: IdeationPlanMode::Optional,
        require_plan_approval: true,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        ..Default::default()
    })
    .await
    .unwrap();

    let s = repo.get_settings().await.unwrap();
    assert!(s.require_plan_approval);
    assert!(!s.suggest_plans_for_complex);
    assert!(!s.auto_link_proposals);

    // Flip: disable require_plan_approval, enable the other two
    repo.update_settings(&IdeationSettings {
        plan_mode: IdeationPlanMode::Optional,
        require_plan_approval: false,
        suggest_plans_for_complex: true,
        auto_link_proposals: true,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        ..Default::default()
    })
    .await
    .unwrap();

    let s2 = repo.get_settings().await.unwrap();
    assert!(!s2.require_plan_approval);
    assert!(s2.suggest_plans_for_complex);
    assert!(s2.auto_link_proposals);
}

// ─── verification fields roundtrip ───────────────────────────────────────────

#[tokio::test]
async fn test_require_verification_for_accept_roundtrip() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-verify-accept");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    repo.update_settings(&IdeationSettings {
        require_verification_for_accept: true,
        ..Default::default()
    })
    .await
    .unwrap();

    let s = repo.get_settings().await.unwrap();
    assert!(s.require_verification_for_accept);
    assert!(!s.require_verification_for_proposals);

    // Toggle back off
    repo.update_settings(&IdeationSettings {
        require_verification_for_accept: false,
        ..Default::default()
    })
    .await
    .unwrap();

    let s2 = repo.get_settings().await.unwrap();
    assert!(!s2.require_verification_for_accept);
}

#[tokio::test]
async fn test_require_verification_for_proposals_roundtrip() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-verify-proposals");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    repo.update_settings(&IdeationSettings {
        require_verification_for_proposals: true,
        ..Default::default()
    })
    .await
    .unwrap();

    let s = repo.get_settings().await.unwrap();
    assert!(!s.require_verification_for_accept);
    assert!(s.require_verification_for_proposals);

    // Toggle back off
    repo.update_settings(&IdeationSettings {
        require_verification_for_proposals: false,
        ..Default::default()
    })
    .await
    .unwrap();

    let s2 = repo.get_settings().await.unwrap();
    assert!(!s2.require_verification_for_proposals);
}

#[tokio::test]
async fn test_both_verification_fields_toggle_independently() {
    let db = SqliteTestDb::new("sqlite_ideation_settings_repo_tests-verify-both");
    let repo = SqliteIdeationSettingsRepository::from_shared(db.shared_conn());

    // Enable accept only
    repo.update_settings(&IdeationSettings {
        require_verification_for_accept: true,
        require_verification_for_proposals: false,
        ..Default::default()
    })
    .await
    .unwrap();
    let s = repo.get_settings().await.unwrap();
    assert!(s.require_verification_for_accept);
    assert!(!s.require_verification_for_proposals);

    // Enable proposals only
    repo.update_settings(&IdeationSettings {
        require_verification_for_accept: false,
        require_verification_for_proposals: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let s2 = repo.get_settings().await.unwrap();
    assert!(!s2.require_verification_for_accept);
    assert!(s2.require_verification_for_proposals);

    // Enable both
    repo.update_settings(&IdeationSettings {
        require_verification_for_accept: true,
        require_verification_for_proposals: true,
        ..Default::default()
    })
    .await
    .unwrap();
    let s3 = repo.get_settings().await.unwrap();
    assert!(s3.require_verification_for_accept);
    assert!(s3.require_verification_for_proposals);
}

#[test]
fn tasks_authorization_allows_any_task_when_enabled() {
    let conn = setup_tasks_authorization_db();
    conn.execute("UPDATE ideation_settings SET tasks_enabled = 1", [])
        .unwrap();

    authorize_tasks_session_sync(&conn, None)
        .expect("enabled Tasks must allow standalone work without a pipeline session");
}

#[test]
fn tasks_authorization_rejects_standalone_and_unentitled_sessions_when_disabled() {
    let conn = setup_tasks_authorization_db();

    let standalone_error = authorize_tasks_session_sync(&conn, None)
        .expect_err("disabled Tasks must reject standalone work");
    assert!(standalone_error.to_string().contains("standalone Task"));

    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('session-1', 'project-1')",
        [],
    )
    .unwrap();
    let missing_workspace_error = authorize_tasks_session_sync(&conn, Some("session-1"))
        .expect_err("a pipeline session without an active workspace is not entitled");
    assert!(missing_workspace_error
        .to_string()
        .contains("active Agent pipeline"));
}

#[test]
fn tasks_authorization_requires_an_active_workspace_for_the_session_project() {
    let conn = setup_tasks_authorization_db();
    conn.execute(
        "INSERT INTO ideation_sessions (id, project_id) VALUES ('session-1', 'project-1')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO agent_conversation_workspaces
         (conversation_id, project_id, task_pipeline_session_id, status)
         VALUES ('workspace-1', 'project-2', 'session-1', 'active')",
        [],
    )
    .unwrap();

    assert!(authorize_tasks_session_sync(&conn, Some("session-1")).is_err());

    conn.execute(
        "UPDATE agent_conversation_workspaces SET project_id = 'project-1', status = 'archived'",
        [],
    )
    .unwrap();
    assert!(authorize_tasks_session_sync(&conn, Some("session-1")).is_err());

    conn.execute(
        "UPDATE agent_conversation_workspaces SET status = 'active'",
        [],
    )
    .unwrap();
    authorize_tasks_session_sync(&conn, Some("session-1"))
        .expect("an active workspace linked to the session's project is entitled");
}
