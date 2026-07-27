use crate::domain::ideation::model_settings::{ModelBucket, ModelLevel};
use crate::domain::repositories::IdeationModelSettingsRepository;
use crate::infrastructure::memory::MemoryIdeationModelSettingsRepository;

use super::{
    resolve_ideation_model, resolve_ideation_subagent_model_with_source,
    resolve_verifier_subagent_model_with_source, ResolvedModel,
};

// --- model_bucket_for_agent (via model_resolver context) ---

#[tokio::test]
async fn test_resolve_model_non_ideation_agent() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    // Non-ideation agent bypasses DB; result comes from YAML. Verify it doesn't
    // panic, returns a non-empty string, and source is "yaml" or "yaml_default".
    let result = resolve_ideation_model("ralphx-execution-worker", None, &repo).await;
    assert!(
        !result.model.is_empty(),
        "expected non-empty model for ralphx-execution-worker"
    );
    assert!(
        result.source == "yaml" || result.source == "yaml_default" || result.source == "default",
        "expected yaml source, got: {}",
        result.source
    );
}

#[tokio::test]
async fn test_resolve_model_project_override_primary() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-abc", "opus", "sonnet", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-ideation", Some("proj-abc"), &repo).await;
    assert_eq!(
        result,
        ResolvedModel {
            model: "opus".to_string(),
            source: "user".to_string(),
        }
    );
}

#[tokio::test]
async fn test_resolve_model_project_override_verifier() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-abc", "sonnet", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-plan-verifier", Some("proj-abc"), &repo).await;
    assert_eq!(
        result,
        ResolvedModel {
            model: "haiku".to_string(),
            source: "user".to_string(),
        }
    );
}

#[tokio::test]
async fn test_resolve_model_global_override() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("opus", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-ideation", None, &repo).await;
    assert_eq!(
        result,
        ResolvedModel {
            model: "opus".to_string(),
            source: "global".to_string(),
        }
    );
}

#[tokio::test]
async fn test_resolve_model_preserves_native_aliases_across_override_scopes() {
    for alias in ["sonnet", "opus", "haiku", "fable"] {
        let repo = MemoryIdeationModelSettingsRepository::new();
        repo.upsert_global(alias, "inherit", "inherit", "inherit")
            .await
            .unwrap();

        let global = resolve_ideation_model("ralphx-ideation", None, &repo).await;
        assert_eq!(global.model, alias);
        assert_eq!(global.source, "global");

        repo.upsert_for_project(
            "native-alias-preservation-project",
            alias,
            "inherit",
            "inherit",
            "inherit",
        )
        .await
        .unwrap();

        let project = resolve_ideation_model(
            "ralphx-ideation",
            Some("native-alias-preservation-project"),
            &repo,
        )
        .await;
        assert_eq!(project.model, alias);
        assert_eq!(project.source, "user");
    }
}

#[tokio::test]
async fn test_resolve_model_global_exact_opus_id_is_preserved() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("claude-opus-4-7", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-ideation", None, &repo).await;
    assert_eq!(result.model, "claude-opus-4-7");
    assert_eq!(result.source, "global");
}

#[tokio::test]
async fn test_resolve_model_project_exact_opus_id_is_preserved() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project(
        "proj-abc",
        "claude-opus-5",
        "claude-opus-4-8",
        "inherit",
        "inherit",
    )
    .await
    .unwrap();

    let primary = resolve_ideation_model("ralphx-ideation", Some("proj-abc"), &repo).await;
    let verifier = resolve_ideation_model("ralphx-plan-verifier", Some("proj-abc"), &repo).await;
    assert_eq!(primary.model, "claude-opus-5");
    assert_eq!(primary.source, "user");
    assert_eq!(verifier.model, "claude-opus-4-8");
    assert_eq!(verifier.source, "user");
}

#[tokio::test]
async fn test_resolve_model_global_override_verifier() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("sonnet", "opus", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-plan-verifier", None, &repo).await;
    assert_eq!(
        result,
        ResolvedModel {
            model: "opus".to_string(),
            source: "global".to_string(),
        }
    );
}

#[tokio::test]
async fn test_resolve_model_project_inherit_falls_to_global() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    // Project row inherits; global row has opus primary
    repo.upsert_for_project("proj-y", "inherit", "inherit", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_global("opus", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-ideation", Some("proj-y"), &repo).await;
    assert_eq!(result.model, "opus");
    assert_eq!(result.source, "global");
}

#[tokio::test]
async fn test_resolve_model_both_inherit_falls_through_to_yaml() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    // Both rows set to inherit → should fall through to YAML config
    repo.upsert_for_project("proj-x", "inherit", "inherit", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_global("inherit", "inherit", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-ideation", Some("proj-x"), &repo).await;
    assert!(
        !result.model.is_empty(),
        "expected non-empty model from YAML fallback"
    );
    assert_ne!(
        result.model, "inherit",
        "inherit should not be returned as final value"
    );
    assert!(
        result.source == "yaml" || result.source == "yaml_default" || result.source == "default",
        "expected yaml source, got: {}",
        result.source
    );
}

#[tokio::test]
async fn test_resolve_model_no_db_rows_falls_through_to_yaml() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    // No rows at all — should fall through to YAML config (zero-change regression)
    let result = resolve_ideation_model("ralphx-ideation", None, &repo).await;
    assert!(
        !result.model.is_empty(),
        "expected non-empty model with no DB rows"
    );
    assert_ne!(result.model, "inherit");
    assert!(
        result.source == "yaml" || result.source == "yaml_default" || result.source == "default",
        "expected yaml source for no-override case, got: {}",
        result.source
    );
}

#[tokio::test]
async fn test_resolve_model_project_id_none_skips_project_level() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    // Seed a global row but pass project_id = None
    repo.upsert_global("haiku", "sonnet", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-ideation", None, &repo).await;
    assert_eq!(result.model, "haiku");
    assert_eq!(result.source, "global");
}

#[tokio::test]
async fn test_resolve_model_all_primary_agents_use_primary_bucket() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("opus", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    for agent in &["ralphx-ideation", "ralphx-ideation-readonly"] {
        let result = resolve_ideation_model(agent, None, &repo).await;
        assert_eq!(
            result.model, "opus",
            "expected primary (opus) for agent '{}'",
            agent
        );
        assert_eq!(result.source, "global");
    }
}

#[tokio::test]
async fn test_resolve_model_verifier_agent_uses_verifier_bucket() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("sonnet", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx-plan-verifier", None, &repo).await;
    assert_eq!(result.model, "haiku");
    assert_eq!(result.source, "global");
}

#[tokio::test]
async fn test_resolve_model_fully_qualified_verifier_agent_uses_verifier_bucket() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("sonnet", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx:ralphx-plan-verifier", None, &repo).await;
    assert_eq!(result.model, "haiku");
    assert_eq!(result.source, "global");
}

#[tokio::test]
async fn test_resolve_model_fully_qualified_primary_agent_uses_primary_bucket() {
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("opus", "haiku", "inherit", "inherit")
        .await
        .unwrap();

    let result = resolve_ideation_model("ralphx:ralphx-ideation", None, &repo).await;
    assert_eq!(result.model, "opus");
    assert_eq!(result.source, "global");
}

// Ensure the ModelBucket type is accessible (used by other tasks)
#[test]
fn test_model_bucket_variants_exist() {
    let _primary = ModelBucket::Primary;
    let _verifier = ModelBucket::Verifier;
}

// --- resolve_verifier_subagent_model_with_source tests ---

#[tokio::test]
async fn test_verifier_subagent_bucket_resolution_independent() {
    // verifier_subagent_model=haiku at project level; verifier_model=sonnet (different field).
    // Assert the function reads from verifier_subagent_model, NOT verifier_model.
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_for_project("proj-1", "opus", "sonnet", "haiku", "inherit")
        .await
        .unwrap();

    let project_row = repo.get_for_project("proj-1").await.unwrap().unwrap();
    let (model, source) = resolve_verifier_subagent_model_with_source(
        Some(&project_row.verifier_subagent_model),
        None,
    );
    assert_eq!(
        model, "haiku",
        "verifier_subagent_model=haiku should be returned, not verifier_model=sonnet"
    );
    assert_eq!(source, "user");
    // Confirm independence: the function did not return the verifier_model value
    assert_ne!(model, "sonnet");
}

#[tokio::test]
async fn test_verifier_subagent_project_overrides_global() {
    // Project verifier_subagent_model=haiku beats global verifier_subagent_model=sonnet.
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("opus", "opus", "sonnet", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-1", "opus", "opus", "haiku", "inherit")
        .await
        .unwrap();

    let project_row = repo.get_for_project("proj-1").await.unwrap().unwrap();
    let global_row = repo.get_global().await.unwrap().unwrap();
    let (model, source) = resolve_verifier_subagent_model_with_source(
        Some(&project_row.verifier_subagent_model),
        Some(&global_row.verifier_subagent_model),
    );
    assert_eq!(
        model, "haiku",
        "project verifier_subagent_model=haiku should win over global=sonnet"
    );
    assert_eq!(source, "user");
}

#[tokio::test]
async fn test_verifier_subagent_fallback_to_haiku() {
    // Both project and global verifier_subagent_model = inherit → hardcoded "haiku" default.
    let repo = MemoryIdeationModelSettingsRepository::new();
    repo.upsert_global("sonnet", "sonnet", "inherit", "inherit")
        .await
        .unwrap();
    repo.upsert_for_project("proj-1", "sonnet", "sonnet", "inherit", "inherit")
        .await
        .unwrap();

    let project_row = repo.get_for_project("proj-1").await.unwrap().unwrap();
    let global_row = repo.get_global().await.unwrap().unwrap();
    let (model, source) = resolve_verifier_subagent_model_with_source(
        Some(&project_row.verifier_subagent_model),
        Some(&global_row.verifier_subagent_model),
    );
    assert_eq!(
        model, "haiku",
        "inherit on both levels should fall back to hardcoded haiku"
    );
    assert_eq!(source, "default");
}

// --- resolve_ideation_subagent_model_with_source tests (PO#1, PO#2, PO#3) ---

#[test]
fn test_resolve_ideation_subagent_project_wins() {
    // PO#1: project-level value beats global when project is non-inherit.
    let (model, source) = resolve_ideation_subagent_model_with_source(
        Some(&ModelLevel::Opus),
        Some(&ModelLevel::Sonnet),
    );
    assert_eq!(
        model, "opus",
        "project-level opus must win over global sonnet"
    );
    assert_eq!(source, "user");
}

#[test]
fn test_resolve_ideation_subagent_global_fallback() {
    // PO#2: project-level inherit falls through to global when global is non-inherit.
    let (model, source) = resolve_ideation_subagent_model_with_source(
        Some(&ModelLevel::Inherit),
        Some(&ModelLevel::Sonnet),
    );
    assert_eq!(
        model, "sonnet",
        "global sonnet must be used when project=inherit"
    );
    assert_eq!(source, "global");
}

#[test]
fn test_resolve_ideation_subagent_hardcoded_fallback() {
    // PO#3: both None → hardcoded "haiku" default.
    let (model, source) = resolve_ideation_subagent_model_with_source(None, None);
    assert_eq!(
        model, "haiku",
        "hardcoded fallback must be haiku when both are None"
    );
    assert_eq!(source, "default");
}
