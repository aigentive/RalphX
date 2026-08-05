//! Tests for the spawn-free remote workspace projections.
//!
//! The provider-projection tests are the security core of Part A: they prove the KEEP
//! allowlist (identity + stored selection) is carried faithfully AND that every NEVER-CROSS
//! field (host paths, env-file path, probe/process-config surface) is absent by construction,
//! including a sentinel-path assertion that no filesystem path can leak through serialization.

use std::sync::Arc;

use super::{
    get_remote_project_for_app_state, list_remote_agent_providers_for_app_state,
    list_remote_projects_for_app_state, RemoteAgentProviderView,
};
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, AgentProviderCliManagementMode, AgentProviderSettings, LogicalEffort,
};
use crate::domain::entities::{Project, ProjectRepositoryCapability};
use crate::infrastructure::memory::{
    MemoryAgentProviderSettingsRepository, MemoryProjectRepository,
};

const SENTINEL_BINARY_PATH: &str = "/host/secret/bin/claude-SENTINEL";
const SENTINEL_ENV_FILE_PATH: &str = "/host/secret/env/claude-SENTINEL.env";

fn state_with_empty_provider_repo() -> AppState {
    let mut state = AppState::new_test();
    state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    state
}

/// A fully-populated Claude row whose NEVER-CROSS fields carry sentinel values, so a leak
/// through any field is detectable in the serialized output.
fn claude_row_with_sentinels() -> AgentProviderSettings {
    AgentProviderSettings {
        provider: AgentHarnessKind::Claude,
        enabled: true,
        is_default: true,
        model: Some("claude-opus-4-8".to_string()),
        effort: Some(LogicalEffort::High),
        approval_policy: Some("never-SENTINEL".to_string()),
        sandbox_mode: Some("read-only-SENTINEL".to_string()),
        service_tier: Some("priority-SENTINEL".to_string()),
        claude_permission_mode: Some("bypassPermissions-SENTINEL".to_string()),
        claude_dangerously_skip_permissions: true,
        claude_allow_dangerously_skip_permissions: true,
        cli_management_mode: AgentProviderCliManagementMode::UserManaged,
        auto_update_enabled: true,
        custom_binary_enabled: true,
        custom_binary_path: Some(SENTINEL_BINARY_PATH.to_string()),
        custom_env_file_enabled: true,
        custom_env_file_path: Some(SENTINEL_ENV_FILE_PATH.to_string()),
        updated_at: chrono::Utc::now(),
    }
}

fn codex_row_disabled() -> AgentProviderSettings {
    AgentProviderSettings {
        provider: AgentHarnessKind::Codex,
        enabled: false,
        is_default: false,
        model: Some("gpt-5.6-sol".to_string()),
        effort: Some(LogicalEffort::Medium),
        ..AgentProviderSettings::disabled_defaults(AgentHarnessKind::Codex)
    }
}

async fn seed(state: &AppState, rows: &[AgentProviderSettings]) {
    for row in rows {
        state
            .agent_provider_settings_repo
            .upsert(row)
            .await
            .expect("seed provider row");
    }
}

#[tokio::test]
async fn provider_projection_keeps_every_allowlisted_field() {
    let state = state_with_empty_provider_repo();
    seed(&state, &[claude_row_with_sentinels(), codex_row_disabled()]).await;

    let views = list_remote_agent_providers_for_app_state(&state)
        .await
        .expect("list providers");

    let claude = views
        .iter()
        .find(|view| view.provider == "claude")
        .expect("claude view");
    assert_eq!(
        claude,
        &RemoteAgentProviderView {
            provider: "claude".to_string(),
            enabled: true,
            is_default: true,
            model: Some("claude-opus-4-8".to_string()),
            effort: Some("high".to_string()),
        }
    );

    let codex = views
        .iter()
        .find(|view| view.provider == "codex")
        .expect("codex view");
    assert_eq!(
        codex,
        &RemoteAgentProviderView {
            provider: "codex".to_string(),
            enabled: false,
            is_default: false,
            model: Some("gpt-5.6-sol".to_string()),
            effort: Some("medium".to_string()),
        }
    );
}

#[tokio::test]
async fn provider_projection_never_crosses_paths_or_process_config() {
    let state = state_with_empty_provider_repo();
    seed(&state, &[claude_row_with_sentinels()]).await;

    let views = list_remote_agent_providers_for_app_state(&state)
        .await
        .expect("list providers");
    let serialized = serde_json::to_value(&views).expect("serialize views");

    // The serialized text must not contain ANY sentinel path or process-config value.
    let text = serde_json::to_string(&serialized).expect("stringify");
    for sentinel in [
        SENTINEL_BINARY_PATH,
        SENTINEL_ENV_FILE_PATH,
        "never-SENTINEL",
        "read-only-SENTINEL",
        "priority-SENTINEL",
        "bypassPermissions-SENTINEL",
    ] {
        assert!(
            !text.contains(sentinel),
            "sentinel `{sentinel}` leaked into the remote provider projection: {text}"
        );
    }

    // The object shape is EXACTLY the allowlist — no extra key can appear.
    let object = serialized
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row.as_object())
        .expect("first provider object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["effort", "enabled", "isDefault", "model", "provider"],
        "remote provider projection must carry only the KEEP allowlist"
    );
}

#[tokio::test]
async fn provider_projection_is_empty_when_no_providers_configured() {
    let state = state_with_empty_provider_repo();

    let views = list_remote_agent_providers_for_app_state(&state)
        .await
        .expect("list providers");
    assert!(views.is_empty());
}

// ---------------------------------------------------------------------------------------
// The project projections (WP4 (d) — the single-project twin).
// ---------------------------------------------------------------------------------------

fn state_with_projects(projects: Vec<Project>) -> AppState {
    let mut state = AppState::new_test();
    state.project_repo = Arc::new(MemoryProjectRepository::with_projects(projects));
    state
}

fn sentinel_project() -> Project {
    let mut project = Project::new("RalphX".to_string(), "/host/code/ralphx".to_string());
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project
}

#[tokio::test]
async fn single_project_projection_matches_the_list_projection_field_for_field() {
    let project = sentinel_project();
    let id = project.id.to_string();
    let state = state_with_projects(vec![project]);

    let listed = list_remote_projects_for_app_state(&state)
        .await
        .expect("list projects");
    let fetched = get_remote_project_for_app_state(&state, &id)
        .await
        .expect("get project")
        .expect("project exists");

    assert_eq!(listed, vec![fetched.clone()]);
    // Never inspected is a real absence, not a preference-derived fabrication.
    let value = serde_json::to_value(&fetched).expect("serializes");
    let object = value.as_object().expect("object");
    assert!(!object.contains_key("repository_capability"));
    assert!(!object.contains_key("repositoryCapability"));
    assert_eq!(
        object["repository_capability_snapshot"],
        serde_json::Value::Null
    );
    assert!(!object.contains_key("fetch_url"));
    assert!(!object.contains_key("push_url"));
    assert!(!object.keys().any(|key| key.contains("worktree")));
    assert_eq!(object["working_directory"], "/host/code/ralphx");
}

#[tokio::test]
async fn project_snapshot_is_real_stale_safe_and_preference_independent() {
    let mut project = sentinel_project();
    let id = project.id.clone();
    project.github_pr_enabled = false;
    let state = state_with_projects(vec![project.clone()]);
    state
        .project_repository_capability_repo
        .upsert(&ProjectRepositoryCapability {
            project_id: id.clone(),
            kind: "github".to_string(),
            fetch_url: Some("secret-fetch".to_string()),
            push_url: Some("secret-push".to_string()),
            message: None,
            inspected_at: chrono::Utc::now(),
            working_directory: project.working_directory.clone(),
        })
        .await
        .expect("seed snapshot");

    let false_view = get_remote_project_for_app_state(&state, id.as_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        false_view
            .repository_capability_snapshot
            .as_ref()
            .unwrap()
            .kind,
        "github"
    );
    assert!(
        false_view
            .repository_capability_snapshot
            .as_ref()
            .unwrap()
            .has_remote
    );
    let serialized = serde_json::to_string(&false_view).unwrap();
    assert!(!serialized.contains("secret-fetch"));
    assert!(!serialized.contains("secret-push"));

    project.github_pr_enabled = true;
    state
        .project_repo
        .update(&project)
        .await
        .expect("toggle preference");
    let true_view = get_remote_project_for_app_state(&state, id.as_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        true_view.repository_capability_snapshot,
        false_view.repository_capability_snapshot
    );

    project.working_directory = "/moved/repo".to_string();
    state
        .project_repo
        .update(&project)
        .await
        .expect("move project");
    let stale_view = get_remote_project_for_app_state(&state, id.as_str())
        .await
        .unwrap()
        .unwrap();
    assert!(stale_view.repository_capability_snapshot.is_none());
}

#[tokio::test]
async fn single_project_projection_is_none_for_an_unknown_id() {
    let state = state_with_projects(vec![]);

    let fetched = get_remote_project_for_app_state(&state, "project-does-not-exist")
        .await
        .expect("missing project is Ok(None), never an error");
    assert!(fetched.is_none());
}
