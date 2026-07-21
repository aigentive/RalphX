use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

use super::SqliteUiFeatureFlagOverridesRepository;
use crate::domain::repositories::UiFeatureFlagOverridesRepository;

fn connection() -> Connection {
    let connection = Connection::open_in_memory().expect("open feature flag database");
    connection
        .execute_batch(
            "CREATE TABLE ui_feature_flag_overrides (
                id INTEGER PRIMARY KEY CHECK(id = 1),
                agent_personas INTEGER NULL,
                composer_folder_references INTEGER NULL,
                agent_conversation_team INTEGER NOT NULL DEFAULT 0,
                agent_conversation_workflows INTEGER NOT NULL DEFAULT 0,
                agent_conversation_autopilot INTEGER NOT NULL DEFAULT 0
             );",
        )
        .expect("create feature flag schema");
    connection
}

#[tokio::test]
async fn missing_overrides_default_and_persona_values_round_trip() {
    let repository = SqliteUiFeatureFlagOverridesRepository::new(connection());

    let defaults = repository.get().await.expect("read missing override row");
    assert_eq!(defaults.agent_personas, None);
    assert!(!defaults.agent_conversation_team);
    assert!(!defaults.agent_conversation_workflows);
    assert!(!defaults.agent_conversation_autopilot);

    repository
        .set_agent_personas(Some(true))
        .await
        .expect("enable personas");
    assert_eq!(
        repository
            .get()
            .await
            .expect("read enabled personas")
            .agent_personas,
        Some(true)
    );

    repository
        .set_agent_personas(Some(false))
        .await
        .expect("disable personas");
    assert_eq!(
        repository
            .get()
            .await
            .expect("read disabled personas")
            .agent_personas,
        Some(false)
    );

    repository
        .set_agent_personas(None)
        .await
        .expect("clear persona override");
    assert_eq!(
        repository
            .get()
            .await
            .expect("read cleared personas")
            .agent_personas,
        None
    );
}

#[tokio::test]
async fn capability_updates_preserve_persona_override_and_inert_legacy_folder_column() {
    let connection = connection();
    connection
        .execute(
            "INSERT INTO ui_feature_flag_overrides (id, composer_folder_references) VALUES (1, 1)",
            [],
        )
        .expect("seed retired folder flag column");
    let shared = Arc::new(Mutex::new(connection));
    let repository = SqliteUiFeatureFlagOverridesRepository::from_shared(Arc::clone(&shared));

    let team_enabled = repository
        .update_agent_capabilities(Some(true), None, None)
        .await
        .expect("enable team capability");
    assert!(team_enabled.agent_conversation_team);
    assert!(!team_enabled.agent_conversation_workflows);
    assert_eq!(team_enabled.agent_personas, None);

    repository
        .set_agent_personas(Some(true))
        .await
        .expect("enable personas");
    let workflows_enabled = repository
        .update_agent_capabilities(None, Some(true), Some(true))
        .await
        .expect("enable workflow capability");
    assert!(workflows_enabled.agent_conversation_team);
    assert!(workflows_enabled.agent_conversation_workflows);
    assert!(workflows_enabled.agent_conversation_autopilot);
    assert_eq!(workflows_enabled.agent_personas, Some(true));

    let team_disabled = repository
        .update_agent_capabilities(Some(false), None, None)
        .await
        .expect("disable team capability");
    assert!(!team_disabled.agent_conversation_team);
    assert!(team_disabled.agent_conversation_workflows);
    assert_eq!(team_disabled.agent_personas, Some(true));
    assert_eq!(
        repository.get().await.expect("read final overrides"),
        team_disabled
    );
    let retired_value = shared
        .lock()
        .await
        .query_row(
            "SELECT composer_folder_references FROM ui_feature_flag_overrides WHERE id = 1",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )
        .expect("read retired folder flag column");
    assert_eq!(retired_value, Some(1));
}
