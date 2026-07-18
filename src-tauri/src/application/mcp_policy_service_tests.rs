use std::collections::BTreeMap;

use chrono::Utc;

use crate::domain::agents::{
    AgentHarnessKind, McpOverrideState, McpPolicyOverride, McpPolicySource, McpServerKey,
    NativeMcpServerSnapshot, NativeMcpState,
};

use super::mcp_policy_service::{
    resolve_layers_for_test, resolve_provider_native_config_root_for_test,
};

#[test]
fn codex_config_root_matches_effective_child_environment() {
    use std::collections::HashMap;

    let default_home = std::path::Path::new("/Users/example");
    let shell_env = HashMap::from([(
        "CODEX_HOME".to_string(),
        "/Users/example/.codex-shell".to_string(),
    )]);
    let provider_env = HashMap::from([(
        "CODEX_HOME".to_string(),
        "/Users/example/.codex-custom".to_string(),
    )]);

    assert_eq!(
        resolve_provider_native_config_root_for_test(
            AgentHarnessKind::Codex,
            default_home,
            &shell_env,
            &provider_env,
        )
        .unwrap(),
        std::path::PathBuf::from("/Users/example/.codex-custom")
    );
    assert_eq!(
        resolve_provider_native_config_root_for_test(
            AgentHarnessKind::Codex,
            default_home,
            &shell_env,
            &HashMap::new(),
        )
        .unwrap(),
        std::path::PathBuf::from("/Users/example/.codex-shell")
    );
    assert_eq!(
        resolve_provider_native_config_root_for_test(
            AgentHarnessKind::Codex,
            default_home,
            &HashMap::new(),
            &HashMap::new(),
        )
        .unwrap(),
        default_home.join(".codex")
    );
}

fn native(state: NativeMcpState) -> NativeMcpServerSnapshot {
    NativeMcpServerSnapshot {
        key: McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap(),
        native_scope: Some("user".to_string()),
        native_state: state,
        known_tools: vec!["create_issue".to_string(), "list_issues".to_string()],
        diagnostic: None,
    }
}

fn policy(
    server_state: McpOverrideState,
    tool: Option<(&str, McpOverrideState)>,
) -> McpPolicyOverride {
    let mut tool_states = BTreeMap::new();
    if let Some((name, state)) = tool {
        tool_states.insert(name.to_string(), state);
    }
    McpPolicyOverride {
        project_id: None,
        key: native(NativeMcpState::Enabled).key,
        server_state,
        tool_states,
        updated_at: Utc::now(),
    }
}

#[test]
fn fields_resolve_independently_across_all_precedence_layers() {
    let global_yaml = policy(McpOverrideState::Disabled, None);
    let global_ui = policy(
        McpOverrideState::Follow,
        Some(("create_issue", McpOverrideState::Disabled)),
    );
    let project_yaml = policy(McpOverrideState::Enabled, None);
    let project_ui = policy(
        McpOverrideState::Follow,
        Some(("list_issues", McpOverrideState::Disabled)),
    );

    let effective = resolve_layers_for_test(
        native(NativeMcpState::Enabled),
        Some(&global_yaml),
        Some(&global_ui),
        Some(&project_yaml),
        Some(&project_ui),
    );
    assert!(effective.enabled);
    assert_eq!(effective.server_source, McpPolicySource::ProjectYaml);
    assert_eq!(
        effective.disabled_tools,
        vec!["create_issue".to_string(), "list_issues".to_string()]
    );
    assert_eq!(
        effective.tool_sources.get("create_issue"),
        Some(&McpPolicySource::GlobalUi)
    );
}

#[test]
fn ralphx_enabled_never_exceeds_native_trust_or_auth_state() {
    let project_ui = policy(McpOverrideState::Enabled, None);
    for state in [
        NativeMcpState::Disabled,
        NativeMcpState::PendingApproval,
        NativeMcpState::AuthRequired,
        NativeMcpState::Untrusted,
        NativeMcpState::Unavailable,
    ] {
        let effective = resolve_layers_for_test(native(state), None, None, None, Some(&project_ui));
        assert!(
            !effective.enabled,
            "native state {state:?} must remain an upper bound"
        );
    }
}

#[tokio::test]
async fn resolve_applies_yaml_and_ui_overrides_to_provider_server() {
    use std::fs;
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".ralphx")).unwrap();
    fs::write(
        home.path().join("mcp.yaml"),
        "mcp:\n  providers:\n    claude:\n      servers:\n        github:\n          tools:\n            archive_issue: disabled\n",
    )
    .unwrap();
    fs::write(
        project.path().join(".ralphx").join("mcp.yaml"),
        "mcp:\n  providers:\n    claude:\n      servers:\n        github:\n          state: disabled\n",
    )
    .unwrap();
    let repo = Arc::new(MemoryMcpPolicyRepository::new());
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    repo.set_tool_state(
        Some("project-1"),
        &key,
        "delete_issue",
        McpOverrideState::Disabled,
    )
    .await
    .unwrap();
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, home.path().join("mcp.yaml"));

    let effective = service
        .resolve(
            native(NativeMcpState::Enabled),
            Some("project-1"),
            Some(project.path()),
        )
        .await
        .unwrap();

    assert!(!effective.enabled);
    assert_eq!(effective.server_state, McpOverrideState::Disabled);
    assert_eq!(effective.server_source, McpPolicySource::ProjectYaml);
    assert_eq!(
        effective.disabled_tools,
        vec!["archive_issue".to_string(), "delete_issue".to_string()]
    );
    assert_eq!(
        effective.tool_sources.get("delete_issue"),
        Some(&McpPolicySource::ProjectUi)
    );
}

#[tokio::test]
async fn service_mutators_delegate_to_repository_by_scope() {
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let root = tempfile::tempdir().unwrap();
    let repo: Arc<dyn McpPolicyRepository> = Arc::new(MemoryMcpPolicyRepository::new());
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, root.path().join("mcp.yaml"));
    let key = McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap();

    let server = service
        .set_server_state(Some("project-1"), &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    let tool = service
        .set_tool_state(
            Some("project-1"),
            &key,
            "create_issue",
            McpOverrideState::Disabled,
        )
        .await
        .unwrap();

    assert_eq!(server.server_state, McpOverrideState::Disabled);
    assert_eq!(
        tool.tool_states.get("create_issue"),
        Some(&McpOverrideState::Disabled)
    );
    assert!(service
        .clear_tool(Some("project-1"), &key, "create_issue")
        .await
        .unwrap());
    assert!(service.clear_server(Some("project-1"), &key).await.unwrap());
    assert!(!service.clear_server(Some("project-1"), &key).await.unwrap());
}

#[tokio::test]
async fn update_follow_is_rejected_without_persisting_a_row() {
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let root = tempfile::tempdir().unwrap();
    let repo = Arc::new(MemoryMcpPolicyRepository::new());
    let service = super::mcp_policy_service::McpPolicyService::new(
        repo.clone(),
        root.path().join("mcp.yaml"),
    );
    let key = McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap();

    let error = service
        .set_server_state(None, &key, McpOverrideState::Follow)
        .await
        .expect_err("Follow uses the clear endpoint");

    assert!(error.to_string().contains("clear"));
    assert!(repo.get_global(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn launch_policy_rejects_yaml_with_invalid_policy_identifiers() {
    use std::fs;
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("mcp.yaml"),
        "mcp:\n  providers:\n    codex:\n      servers:\n        'bad server':\n          state: disabled\n",
    )
    .unwrap();
    let repo: Arc<dyn McpPolicyRepository> = Arc::new(MemoryMcpPolicyRepository::new());
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, root.path().join("mcp.yaml"));

    let error = service
        .resolve_launch_policy(AgentHarnessKind::Codex, None, None)
        .await
        .expect_err("invalid YAML diagnostics must fail launch closed");

    assert!(error.to_string().to_lowercase().contains("invalid"));
}

#[tokio::test]
async fn required_internal_server_is_locked_and_cannot_accumulate_denies() {
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let mut internal = native(NativeMcpState::Unavailable);
    internal.key = McpServerKey::new(AgentHarnessKind::Codex, "ralphx_internal").unwrap();
    internal.native_scope = Some("ralphx".to_string());
    let root = tempfile::tempdir().unwrap();
    let repo: Arc<dyn McpPolicyRepository> = Arc::new(MemoryMcpPolicyRepository::new());
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, root.path().join("mcp.yaml"));
    let effective = service.resolve(internal, None, None).await.unwrap();
    assert!(effective.enabled);
    assert!(effective.locked);
    assert!(effective.disabled_tools.is_empty());
}

#[tokio::test]
async fn provider_native_reserved_id_collision_fails_closed() {
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let mut collision = native(NativeMcpState::Enabled);
    collision.key = McpServerKey::new(AgentHarnessKind::Claude, "ralphx").unwrap();
    collision.native_scope = Some("user".to_string());
    let root = tempfile::tempdir().unwrap();
    let repo: Arc<dyn McpPolicyRepository> = Arc::new(MemoryMcpPolicyRepository::new());
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, root.path().join("mcp.yaml"));

    let effective = service.resolve(collision, None, None).await.unwrap();
    assert!(!effective.enabled);
    assert!(effective.locked);
    assert!(effective.native.diagnostic.is_some());
}

#[tokio::test]
async fn launch_policy_merges_global_and_project_denies_for_one_provider() {
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let root = tempfile::tempdir().unwrap();
    let repo = Arc::new(MemoryMcpPolicyRepository::new());
    let github = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    let linear = McpServerKey::new(AgentHarnessKind::Claude, "linear").unwrap();
    repo.set_server_state(None, &github, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_tool_state(
        Some("project-1"),
        &linear,
        "delete_issue",
        McpOverrideState::Disabled,
    )
    .await
    .unwrap();
    let repo: Arc<dyn McpPolicyRepository> = repo;
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, root.path().join("mcp.yaml"));

    let policy = service
        .resolve_launch_policy(
            AgentHarnessKind::Claude,
            Some("project-1"),
            Some(root.path()),
        )
        .await
        .unwrap();

    assert_eq!(policy.disabled_servers, vec!["github"]);
    assert_eq!(
        policy.disabled_tools.get("linear"),
        Some(&vec!["delete_issue".to_string()])
    );
}

#[tokio::test]
async fn launch_policy_merges_yaml_and_ui_with_project_precedence() {
    use std::fs;
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join(".ralphx")).unwrap();
    fs::write(
        home.path().join("mcp.yaml"),
        "mcp:\n  providers:\n    claude:\n      servers:\n        github:\n          state: disabled\n        slack:\n          tools:\n            post_message: disabled\n",
    )
    .unwrap();
    fs::write(
        project.path().join(".ralphx").join("mcp.yaml"),
        "mcp:\n  providers:\n    claude:\n      servers:\n        github:\n          state: enabled\n          tools:\n            delete_issue: disabled\n",
    )
    .unwrap();
    let repo = Arc::new(MemoryMcpPolicyRepository::new());
    let linear = McpServerKey::new(AgentHarnessKind::Claude, "linear").unwrap();
    repo.set_server_state(Some("project-1"), &linear, McpOverrideState::Disabled)
        .await
        .unwrap();
    let repo: Arc<dyn McpPolicyRepository> = repo;
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, home.path().join("mcp.yaml"));

    let policy = service
        .resolve_launch_policy(
            AgentHarnessKind::Claude,
            Some("project-1"),
            Some(project.path()),
        )
        .await
        .unwrap();

    assert_eq!(policy.disabled_servers, vec!["linear"]);
    assert_eq!(
        policy.disabled_tools.get("github"),
        Some(&vec!["delete_issue".to_string()])
    );
    assert_eq!(
        policy.disabled_tools.get("slack"),
        Some(&vec!["post_message".to_string()])
    );
}

#[tokio::test]
async fn resolve_keeps_existing_required_internal_diagnostic_when_not_colliding() {
    use std::sync::Arc;

    use crate::domain::repositories::McpPolicyRepository;
    use crate::infrastructure::memory::MemoryMcpPolicyRepository;

    let mut internal = native(NativeMcpState::Enabled);
    internal.key = McpServerKey::new(AgentHarnessKind::Claude, "ralphx").unwrap();
    internal.native_scope = Some("ralphx".to_string());
    internal.diagnostic = Some("managed by RalphX".to_string());
    let root = tempfile::tempdir().unwrap();
    let repo: Arc<dyn McpPolicyRepository> = Arc::new(MemoryMcpPolicyRepository::new());
    let service =
        super::mcp_policy_service::McpPolicyService::new(repo, root.path().join("mcp.yaml"));

    let effective = service.resolve(internal, None, None).await.unwrap();
    assert!(effective.enabled);
    assert_eq!(
        effective.native.diagnostic.as_deref(),
        Some("managed by RalphX")
    );
    assert_eq!(effective.server_source, McpPolicySource::RequiredInternal);
}
