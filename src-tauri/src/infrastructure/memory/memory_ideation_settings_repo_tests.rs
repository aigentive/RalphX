use super::*;
use crate::domain::ideation::IdeationPlanMode;

#[tokio::test]
async fn test_get_default_settings() {
    let repo = MemoryIdeationSettingsRepository::new();

    let settings = repo.get_settings().await.unwrap();
    assert!(!settings.tasks_enabled);
    assert_eq!(settings.plan_mode, IdeationPlanMode::Optional);
    assert!(!settings.require_plan_approval);
    assert!(settings.suggest_plans_for_complex);
    assert!(settings.auto_link_proposals);
}

#[tokio::test]
async fn test_update_settings() {
    let repo = MemoryIdeationSettingsRepository::new();

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
    assert!(!updated.tasks_enabled);

    // Verify persistence
    let retrieved = repo.get_settings().await.unwrap();
    assert_eq!(retrieved.plan_mode, IdeationPlanMode::Required);
    assert!(retrieved.require_plan_approval);
}

#[tokio::test]
async fn test_with_settings() {
    let initial_settings = IdeationSettings {
        tasks_enabled: true,
        tasks_feature_state: TasksFeatureState::Enabled,
        plan_mode: IdeationPlanMode::Parallel,
        require_plan_approval: true,
        suggest_plans_for_complex: false,
        auto_link_proposals: false,
        require_verification_for_accept: false,
        require_verification_for_proposals: false,
        require_accept_for_finalize: false,
        ..Default::default()
    };

    let repo = MemoryIdeationSettingsRepository::with_settings(initial_settings);

    let settings = repo.get_settings().await.unwrap();
    assert_eq!(settings.plan_mode, IdeationPlanMode::Parallel);
    assert!(settings.require_plan_approval);
}
