use ralphx_lib::domain::repositories::UiFeatureFlagOverridesRepository;
use ralphx_lib::infrastructure::sqlite::SqliteUiFeatureFlagOverridesRepository;
use ralphx_lib::testing::SqliteTestDb;

#[tokio::test]
async fn persona_flag_override_sqlite_repo_defaults_to_no_override() {
    let db = SqliteTestDb::new("persona-flag-override-default");
    let repo = SqliteUiFeatureFlagOverridesRepository::from_shared(db.shared_conn());

    let overrides = repo
        .get()
        .await
        .expect("fresh override read should succeed");

    assert_eq!(overrides.agent_personas, None);
}

#[tokio::test]
async fn persona_flag_override_sqlite_repo_persists_set_and_clear() {
    let db = SqliteTestDb::new("persona-flag-override-set-clear");
    let repo = SqliteUiFeatureFlagOverridesRepository::from_shared(db.shared_conn());

    repo.set_agent_personas(Some(true))
        .await
        .expect("set true should succeed");
    assert_eq!(
        repo.get()
            .await
            .expect("persisted override read should succeed")
            .agent_personas,
        Some(true)
    );

    repo.set_agent_personas(None)
        .await
        .expect("clear should succeed");
    assert_eq!(
        repo.get()
            .await
            .expect("cleared override read should succeed")
            .agent_personas,
        None
    );
}

#[tokio::test]
async fn persona_flag_override_sqlite_repo_recreates_deleted_singleton_row() {
    let db = SqliteTestDb::new("persona-flag-override-recreate-row");
    let repo = SqliteUiFeatureFlagOverridesRepository::from_shared(db.shared_conn());
    db.with_connection(|conn| {
        conn.execute("DELETE FROM ui_feature_flag_overrides WHERE id = 1", [])
            .expect("fixture singleton row should delete");
    });

    repo.set_agent_personas(Some(false))
        .await
        .expect("set should recreate the singleton row");

    assert_eq!(
        repo.get()
            .await
            .expect("recreated override read should succeed")
            .agent_personas,
        Some(false)
    );
}
