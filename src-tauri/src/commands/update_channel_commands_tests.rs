use async_trait::async_trait;
use std::sync::Arc;
use tauri::Manager;

use super::update_channel_commands::{get_update_channel, set_update_channel};
use crate::domain::entities::app_state::{AppSettings, ExecutionHaltMode, UpdateChannel};
use crate::domain::entities::ProjectId;
use crate::domain::repositories::AppStateRepository;
use crate::AppState;

fn test_app() -> tauri::App<tauri::test::MockRuntime> {
    test_app_with_app_state_repo(Arc::new(
        crate::infrastructure::memory::MemoryAppStateRepository::new(),
    ))
}

fn test_app_with_app_state_repo(
    app_state_repo: Arc<dyn AppStateRepository>,
) -> tauri::App<tauri::test::MockRuntime> {
    let mut state = AppState::new_test();
    state.app_state_repo = app_state_repo;

    tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build update channel test app")
}

struct FailingAppStateRepository {
    read_error: Option<&'static str>,
    write_error: Option<&'static str>,
}

#[async_trait]
impl AppStateRepository for FailingAppStateRepository {
    async fn get(&self) -> Result<AppSettings, Box<dyn std::error::Error>> {
        match self.read_error {
            Some(error) => Err(std::io::Error::other(error).into()),
            None => Ok(AppSettings::default()),
        }
    }

    async fn set_active_project(
        &self,
        _project_id: Option<&ProjectId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_execution_halt_mode(
        &self,
        _halt_mode: ExecutionHaltMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_update_channel(
        &self,
        _update_channel: UpdateChannel,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self.write_error {
            Some(error) => Err(std::io::Error::other(error).into()),
            None => Ok(()),
        }
    }

    async fn set_last_seen_release_notes_version(
        &self,
        _version: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    async fn set_remove_inherited_github_cli_tokens(
        &self,
        _enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }
}

#[test]
fn update_channels_serialize_as_snake_case() {
    assert_eq!(
        serde_json::to_string(&UpdateChannel::Stable).unwrap(),
        "\"stable\""
    );
    assert_eq!(
        serde_json::to_string(&UpdateChannel::Nightly).unwrap(),
        "\"nightly\""
    );
}

#[test]
fn update_channels_reject_unknown_wire_values() {
    assert!(serde_json::from_str::<UpdateChannel>(r#""canary""#).is_err());
}

#[tokio::test]
async fn get_update_channel_defaults_to_stable() {
    let app = test_app();

    assert_eq!(
        get_update_channel(app.state::<AppState>()).await.unwrap(),
        UpdateChannel::Stable
    );
}

#[tokio::test]
async fn get_update_channel_surfaces_repository_read_errors() {
    let app = test_app_with_app_state_repo(Arc::new(FailingAppStateRepository {
        read_error: Some("injected update channel read failure"),
        write_error: None,
    }));

    let error = get_update_channel(app.state::<AppState>())
        .await
        .expect_err("repository read failure should reach the command caller");

    assert_eq!(error, "injected update channel read failure");
}

#[tokio::test]
async fn set_update_channel_persists_both_supported_values() {
    let app = test_app();

    assert_eq!(
        set_update_channel(UpdateChannel::Nightly, app.state::<AppState>())
            .await
            .unwrap(),
        UpdateChannel::Nightly
    );
    assert_eq!(
        get_update_channel(app.state::<AppState>()).await.unwrap(),
        UpdateChannel::Nightly
    );
    assert_eq!(
        set_update_channel(UpdateChannel::Stable, app.state::<AppState>())
            .await
            .unwrap(),
        UpdateChannel::Stable
    );
    assert_eq!(
        get_update_channel(app.state::<AppState>()).await.unwrap(),
        UpdateChannel::Stable
    );
}

#[tokio::test]
async fn set_update_channel_surfaces_repository_write_errors() {
    let app = test_app_with_app_state_repo(Arc::new(FailingAppStateRepository {
        read_error: None,
        write_error: Some("injected update channel write failure"),
    }));

    let error = set_update_channel(UpdateChannel::Nightly, app.state::<AppState>())
        .await
        .expect_err("repository write failure should reach the command caller");

    assert_eq!(error, "injected update channel write failure");
}
