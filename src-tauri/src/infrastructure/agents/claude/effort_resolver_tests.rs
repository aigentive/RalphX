use crate::domain::ideation::EffortBucket;
use crate::domain::repositories::IdeationEffortSettingsRepository;
use crate::infrastructure::memory::MemoryIdeationEffortSettingsRepository;

use super::{effort_bucket_for_agent, resolve_ideation_effort};

// --- effort_bucket_for_agent ---

#[test]
fn test_effort_bucket_mapping_primary_agents() {
    for name in &["ralphx-ideation", "ralphx-ideation-readonly"] {
        assert_eq!(
            effort_bucket_for_agent(name),
            Some(EffortBucket::Primary),
            "expected Primary for agent '{}'",
            name
        );
    }
}

#[test]
fn test_effort_bucket_mapping_primary_agents_fully_qualified() {
    assert_eq!(
        effort_bucket_for_agent("ralphx:ralphx-ideation"),
        Some(EffortBucket::Primary)
    );
}

#[test]
fn test_effort_bucket_mapping_legacy_aliases() {
    for name in &["orchestrator-ideation", "orchestrator-ideation-readonly"] {
        assert_eq!(
            effort_bucket_for_agent(name),
            Some(EffortBucket::Primary),
            "expected Primary for legacy agent '{}'",
            name
        );
    }
}

#[test]
fn test_effort_bucket_mapping_non_ideation() {
    assert_eq!(effort_bucket_for_agent("ralphx-execution-worker"), None);
}

// --- resolve_ideation_effort ---

#[tokio::test]
async fn test_resolve_effort_non_ideation_agent() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    // Non-ideation agent bypasses DB; result comes from YAML. Just verify it
    // doesn't panic and returns a non-empty string.
    let result = resolve_ideation_effort("ralphx-execution-worker", None, &repo).await;
    assert!(
        !result.is_empty(),
        "expected non-empty effort for ralphx-execution-worker"
    );
}

#[tokio::test]
async fn test_resolve_effort_project_override() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    // Seed project row with high primary effort
    repo.upsert(Some("proj-abc"), "high", "low").await.unwrap();

    let result = resolve_ideation_effort("ralphx-ideation", Some("proj-abc"), &repo).await;
    assert_eq!(result, "high");
}

#[tokio::test]
async fn test_resolve_effort_global_override() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    // Seed global row with medium verifier effort
    repo.upsert(None, "low", "medium").await.unwrap();

    let result = resolve_ideation_effort("ralphx-plan-verifier", None, &repo).await;
    assert_eq!(result, "medium");
}

#[tokio::test]
async fn test_resolve_effort_fully_qualified_verifier_uses_verifier_bucket() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    repo.upsert(None, "low", "medium").await.unwrap();

    let result = resolve_ideation_effort("ralphx:ralphx-plan-verifier", None, &repo).await;
    assert_eq!(result, "medium");
}

#[tokio::test]
async fn test_resolve_effort_fully_qualified_primary_uses_primary_bucket() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    repo.upsert(None, "high", "low").await.unwrap();

    let result = resolve_ideation_effort("ralphx:ralphx-ideation", None, &repo).await;
    assert_eq!(result, "high");
}

#[tokio::test]
async fn test_resolve_effort_inherit_falls_through_to_yaml() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    // Both rows set to inherit → should fall through to YAML
    repo.upsert(Some("proj-x"), "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert(None, "inherit", "inherit").await.unwrap();

    let result = resolve_ideation_effort("ralphx-ideation", Some("proj-x"), &repo).await;
    assert!(
        !result.is_empty(),
        "expected non-empty effort from YAML fallback"
    );
    assert_ne!(
        result, "inherit",
        "inherit should not be returned as the final value"
    );
}

#[tokio::test]
async fn test_resolve_effort_project_inherit_falls_to_global() {
    let repo = MemoryIdeationEffortSettingsRepository::new();
    // Project row inherits; global row has "high"
    repo.upsert(Some("proj-y"), "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert(None, "high", "inherit").await.unwrap();

    let result = resolve_ideation_effort("ralphx-ideation", Some("proj-y"), &repo).await;
    assert_eq!(result, "high");
}
