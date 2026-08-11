use super::*;
use crate::domain::entities::app_state::{ExecutionHaltMode, UpdateChannel};
use crate::domain::entities::ProjectId;
use crate::domain::repositories::AppStateRepository;

#[tokio::test]
async fn test_get_default_app_state() {
    let repo = MemoryAppStateRepository::new();

    let settings = repo.get().await.unwrap();
    assert!(settings.active_project_id.is_none());
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Running);
    assert_eq!(settings.update_channel, UpdateChannel::Stable);
    assert!(settings.last_seen_release_notes_version.is_none());
    assert!(settings.remove_inherited_github_cli_tokens);
}

#[tokio::test]
async fn update_channel_updates_for_both_supported_values() {
    let repo = MemoryAppStateRepository::new();

    repo.set_update_channel(UpdateChannel::Nightly)
        .await
        .unwrap();
    assert_eq!(
        repo.get().await.unwrap().update_channel,
        UpdateChannel::Nightly
    );

    repo.set_update_channel(UpdateChannel::Stable)
        .await
        .unwrap();
    assert_eq!(
        repo.get().await.unwrap().update_channel,
        UpdateChannel::Stable
    );
}

#[tokio::test]
async fn github_cli_token_environment_preference_updates() {
    let repo = MemoryAppStateRepository::new();

    repo.set_remove_inherited_github_cli_tokens(false)
        .await
        .unwrap();

    assert!(!repo.get().await.unwrap().remove_inherited_github_cli_tokens);
}

#[tokio::test]
async fn test_set_and_get_active_project() {
    let repo = MemoryAppStateRepository::new();

    let project_id = ProjectId::from_string("proj-123".to_string());
    repo.set_active_project(Some(&project_id)).await.unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(
        settings.active_project_id,
        Some(ProjectId::from_string("proj-123".to_string()))
    );
}

#[tokio::test]
async fn test_clear_active_project() {
    let repo = MemoryAppStateRepository::new();

    let project_id = ProjectId::from_string("proj-123".to_string());
    repo.set_active_project(Some(&project_id)).await.unwrap();

    repo.set_active_project(None).await.unwrap();

    let settings = repo.get().await.unwrap();
    assert!(settings.active_project_id.is_none());
}

#[tokio::test]
async fn test_with_active_project() {
    let project_id = ProjectId::from_string("proj-456".to_string());
    let repo = MemoryAppStateRepository::with_active_project(project_id);

    let settings = repo.get().await.unwrap();
    assert_eq!(
        settings.active_project_id,
        Some(ProjectId::from_string("proj-456".to_string()))
    );
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Running);
}

#[tokio::test]
async fn test_set_execution_halt_mode_paused() {
    let repo = MemoryAppStateRepository::new();

    repo.set_execution_halt_mode(ExecutionHaltMode::Paused)
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Paused);
}

#[tokio::test]
async fn test_set_execution_halt_mode_stopped() {
    let repo = MemoryAppStateRepository::new();

    repo.set_execution_halt_mode(ExecutionHaltMode::Stopped)
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(settings.execution_halt_mode, ExecutionHaltMode::Stopped);
}

#[tokio::test]
async fn test_set_last_seen_release_notes_version() {
    let repo = MemoryAppStateRepository::new();

    repo.set_last_seen_release_notes_version(Some("0.9.0"))
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert_eq!(
        settings.last_seen_release_notes_version,
        Some("0.9.0".to_string())
    );

    repo.set_last_seen_release_notes_version(None)
        .await
        .unwrap();

    let settings = repo.get().await.unwrap();
    assert!(settings.last_seen_release_notes_version.is_none());
}
