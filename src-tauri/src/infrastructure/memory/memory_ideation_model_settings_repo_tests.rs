use crate::domain::ideation::model_settings::ModelLevel;
use crate::domain::repositories::IdeationModelSettingsRepository;
use crate::infrastructure::memory::MemoryIdeationModelSettingsRepository;

#[tokio::test]
async fn test_upsert_and_get_global() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    assert!(repo.get_global().await.unwrap().is_none());

    let result = repo
        .upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(result.primary_model, ModelLevel::Sonnet);
    assert_eq!(result.verifier_model, ModelLevel::Opus);
    assert!(result.project_id.is_none());

    let fetched = repo.get_global().await.unwrap().unwrap();
    assert_eq!(fetched.primary_model, ModelLevel::Sonnet);
    assert_eq!(fetched.verifier_model, ModelLevel::Opus);
}

#[tokio::test]
async fn test_upsert_and_get_project() {
    let repo = MemoryIdeationModelSettingsRepository::new();

    let result = repo
        .upsert_for_project("proj-123", "opus", "haiku", "inherit", "inherit")
        .await
        .unwrap();
    assert_eq!(result.primary_model, ModelLevel::Opus);
    assert!(result.project_id.is_some());

    let fetched = repo.get_for_project("proj-123").await.unwrap().unwrap();
    assert_eq!(fetched.primary_model, ModelLevel::Opus);
    assert_eq!(fetched.verifier_model, ModelLevel::Haiku);

    // Global is still empty
    assert!(repo.get_global().await.unwrap().is_none());
}

#[tokio::test]
async fn test_exact_opus_ids_round_trip_for_global_and_project_rows() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("claude-opus-4-7", "claude-opus-4-8", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-123", "claude-opus-5", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let global = repo.get_global().await.unwrap().unwrap();
    let project = repo.get_for_project("proj-123").await.unwrap().unwrap();
    assert_eq!(global.primary_model.to_string(), "claude-opus-4-7");
    assert_eq!(global.verifier_model.to_string(), "claude-opus-4-8");
    assert_eq!(project.primary_model.to_string(), "claude-opus-5");
}

#[tokio::test]
async fn test_upsert_overwrites_existing() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_global("haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let fetched = repo.get_global().await.unwrap().unwrap();
    assert_eq!(fetched.primary_model, ModelLevel::Haiku);
    assert_eq!(fetched.verifier_model, ModelLevel::Inherit);
}

#[tokio::test]
async fn test_upsert_project_overwrites_existing() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-abc", "sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-abc", "haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let fetched = repo.get_for_project("proj-abc").await.unwrap().unwrap();
    assert_eq!(fetched.primary_model, ModelLevel::Haiku);
    assert_eq!(fetched.verifier_model, ModelLevel::Inherit);
}

#[tokio::test]
async fn test_get_missing_returns_none() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    assert!(repo.get_for_project("nonexistent").await.unwrap().is_none());
}

#[tokio::test]
async fn test_project_and_global_are_independent() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-1", "haiku", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let global = repo.get_global().await.unwrap().unwrap();
    let project = repo.get_for_project("proj-1").await.unwrap().unwrap();

    assert_eq!(global.primary_model, ModelLevel::Sonnet);
    assert_eq!(project.primary_model, ModelLevel::Haiku);
    assert!(global.project_id.is_none());
    assert!(project.project_id.is_some());
}

#[tokio::test]
async fn test_invalid_model_level_returns_error() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    let result = repo
        .upsert_global("turbo", "opus", "inherit", "inherit")
        .await;
    assert!(result.is_err());
}
