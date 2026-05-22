use std::sync::Arc;

use crate::application::ideation_model_bootstrap::seed_ideation_model_settings;
use crate::domain::ideation::ModelLevel;
use crate::domain::repositories::IdeationModelSettingsRepository;
use crate::infrastructure::memory::MemoryIdeationModelSettingsRepository;

#[tokio::test]
async fn test_seeds_when_empty() {
    let repo = Arc::new(MemoryIdeationModelSettingsRepository::new());
    let result = seed_ideation_model_settings(Arc::clone(&repo) as _)
        .await
        .unwrap();
    assert!(result.seeded_global);

    let row = repo.get_global().await.unwrap().unwrap();
    assert_eq!(row.primary_model, ModelLevel::Inherit);
    assert_eq!(row.verifier_model, ModelLevel::Inherit);
}

#[tokio::test]
async fn test_idempotent_when_already_seeded() {
    let repo = Arc::new(MemoryIdeationModelSettingsRepository::new());
    // Pre-seed with user values
    repo.upsert_global("opus", "sonnet", "inherit", "inherit").await.unwrap();

    let result = seed_ideation_model_settings(Arc::clone(&repo) as _)
        .await
        .unwrap();
    assert!(!result.seeded_global);

    // User values preserved
    let row = repo.get_global().await.unwrap().unwrap();
    assert_eq!(row.primary_model, ModelLevel::Opus);
    assert_eq!(row.verifier_model, ModelLevel::Sonnet);
}

#[tokio::test]
async fn test_idempotent_on_repeated_calls() {
    let repo = Arc::new(MemoryIdeationModelSettingsRepository::new());

    let r1 = seed_ideation_model_settings(Arc::clone(&repo) as _)
        .await
        .unwrap();
    let r2 = seed_ideation_model_settings(Arc::clone(&repo) as _)
        .await
        .unwrap();

    assert!(r1.seeded_global);
    assert!(!r2.seeded_global);
}

#[tokio::test]
async fn test_preserves_user_configured_inherit_values() {
    let repo = Arc::new(MemoryIdeationModelSettingsRepository::new());
    // Pre-seed explicitly with inherit values (user chose inherit, not just default)
    repo.upsert_global("inherit", "inherit", "inherit", "inherit").await.unwrap();

    let result = seed_ideation_model_settings(Arc::clone(&repo) as _)
        .await
        .unwrap();
    // Row existed, so no re-seed
    assert!(!result.seeded_global);

    let row = repo.get_global().await.unwrap().unwrap();
    assert_eq!(row.primary_model, ModelLevel::Inherit);
    assert_eq!(row.verifier_model, ModelLevel::Inherit);
}
