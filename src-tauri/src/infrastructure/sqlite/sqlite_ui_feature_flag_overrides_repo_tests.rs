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
    assert_eq!(defaults.composer_folder_references, None);
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
async fn composer_folder_reference_override_round_trips() {
    let repository = SqliteUiFeatureFlagOverridesRepository::new(connection());
    repository
        .set_composer_folder_references(Some(true))
        .await
        .expect("enable folder references");
    assert_eq!(
        repository
            .get()
            .await
            .expect("read folder reference override")
            .composer_folder_references,
        Some(true)
    );
}

#[tokio::test]
async fn capability_updates_preserve_omitted_values_and_persona_override() {
    let repository =
        SqliteUiFeatureFlagOverridesRepository::from_shared(Arc::new(Mutex::new(connection())));

    repository
        .set_composer_folder_references(Some(true))
        .await
        .expect("enable folder references");

    let team_enabled = repository
        .update_agent_capabilities(Some(true), None, None)
        .await
        .expect("enable team capability");
    assert!(team_enabled.agent_conversation_team);
    assert!(!team_enabled.agent_conversation_workflows);
    assert_eq!(team_enabled.agent_personas, None);
    assert_eq!(team_enabled.composer_folder_references, Some(true));

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
    assert_eq!(workflows_enabled.composer_folder_references, Some(true));

    let team_disabled = repository
        .update_agent_capabilities(Some(false), None, None)
        .await
        .expect("disable team capability");
    assert!(!team_disabled.agent_conversation_team);
    assert!(team_disabled.agent_conversation_workflows);
    assert_eq!(team_disabled.agent_personas, Some(true));
    assert_eq!(team_disabled.composer_folder_references, Some(true));
    assert_eq!(
        repository.get().await.expect("read final overrides"),
        team_disabled
    );
}
