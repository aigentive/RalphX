use crate::domain::agents::{AgentHarnessKind, McpOverrideState, McpServerKey};
use crate::domain::repositories::McpPolicyRepository;
use crate::testing::SqliteTestDb;

use super::SqliteMcpPolicyRepository;

#[tokio::test]
async fn lists_global_and_project_policies_without_cross_scope_leakage() {
    let db = SqliteTestDb::new("mcp-policy-list-scopes");
    let repo = SqliteMcpPolicyRepository::new(db.new_connection());
    let github = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    let linear = McpServerKey::new(AgentHarnessKind::Claude, "linear").unwrap();

    repo.set_server_state(None, &github, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_server_state(Some("project-1"), &linear, McpOverrideState::Enabled)
        .await
        .unwrap();

    let global = repo.list_global().await.unwrap();
    let project = repo.list_for_project("project-1").await.unwrap();
    assert_eq!(global.len(), 1);
    assert_eq!(global[0].key.server_id, "github");
    assert_eq!(project.len(), 1);
    assert_eq!(project[0].key.server_id, "linear");
}

#[tokio::test]
async fn invalid_tool_names_fail_before_mutating_rows() {
    let db = SqliteTestDb::new("mcp-policy-invalid-tool");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap();

    assert!(repo
        .set_tool_state(None, &key, "../unsafe", McpOverrideState::Disabled)
        .await
        .is_err());
    assert!(repo.get_global(&key).await.unwrap().is_none());

    repo.set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    assert!(repo.clear_tool(None, &key, "../unsafe").await.is_err());
    assert_eq!(
        repo.get_global(&key).await.unwrap().unwrap().server_state,
        McpOverrideState::Disabled
    );
}

#[tokio::test]
async fn clearing_missing_tool_from_existing_policy_is_a_noop() {
    let db = SqliteTestDb::new("mcp-policy-clear-missing-tool");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();

    repo.set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .unwrap();

    assert!(!repo.clear_tool(None, &key, "missing_tool").await.unwrap());
    let policy = repo.get_global(&key).await.unwrap().unwrap();
    assert_eq!(policy.server_state, McpOverrideState::Disabled);
    assert!(policy.tool_states.is_empty());
}

#[tokio::test]
async fn policy_round_trips_server_and_tool_fields_by_scope() {
    let db = SqliteTestDb::new("mcp-policy-round-trip");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap();

    repo.set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_tool_state(
        Some("project-1"),
        &key,
        "create_issue",
        McpOverrideState::Disabled,
    )
    .await
    .unwrap();

    let global = repo.get_global(&key).await.unwrap().unwrap();
    let project = repo
        .get_for_project("project-1", &key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(global.server_state, McpOverrideState::Disabled);
    assert_eq!(project.server_state, McpOverrideState::Follow);
    assert_eq!(
        project.tool_states.get("create_issue"),
        Some(&McpOverrideState::Disabled)
    );
}

#[tokio::test]
async fn invalid_required_disable_leaves_repository_unchanged() {
    let db = SqliteTestDb::new("mcp-policy-required-guard");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Claude, "ralphx").unwrap();

    assert!(repo
        .set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .is_err());
    assert!(repo.get_global(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn clear_tool_removes_empty_follow_policy_but_preserves_server_override() {
    let db = SqliteTestDb::new("mcp-policy-clear-tool");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();

    assert!(!repo
        .clear_tool(Some("project-1"), &key, "delete_issue")
        .await
        .unwrap());
    repo.set_tool_state(
        Some("project-1"),
        &key,
        "delete_issue",
        McpOverrideState::Disabled,
    )
    .await
    .unwrap();
    assert!(repo
        .clear_tool(Some("project-1"), &key, "delete_issue")
        .await
        .unwrap());
    assert!(repo
        .get_for_project("project-1", &key)
        .await
        .unwrap()
        .is_none());

    repo.set_server_state(Some("project-1"), &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_tool_state(
        Some("project-1"),
        &key,
        "delete_issue",
        McpOverrideState::Disabled,
    )
    .await
    .unwrap();
    assert!(repo
        .clear_tool(Some("project-1"), &key, "delete_issue")
        .await
        .unwrap());
    let policy = repo
        .get_for_project("project-1", &key)
        .await
        .unwrap()
        .expect("server override should remain after clearing only tool");
    assert_eq!(policy.server_state, McpOverrideState::Disabled);
    assert!(policy.tool_states.is_empty());
}

#[tokio::test]
async fn clear_server_is_scope_specific() {
    let db = SqliteTestDb::new("mcp-policy-clear-server-scope");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap();

    repo.set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_server_state(Some("project-1"), &key, McpOverrideState::Enabled)
        .await
        .unwrap();
    assert!(repo.clear_server(Some("project-1"), &key).await.unwrap());
    assert!(repo
        .get_for_project("project-1", &key)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        repo.get_global(&key).await.unwrap().unwrap().server_state,
        McpOverrideState::Disabled
    );
}

#[tokio::test]
async fn clearing_server_state_preserves_independent_tool_overrides() {
    let db = SqliteTestDb::new("mcp-policy-clear-server-preserves-tools");
    let repo = SqliteMcpPolicyRepository::from_shared(db.shared_conn());
    let key = McpServerKey::new(AgentHarnessKind::Codex, "github").unwrap();

    repo.set_server_state(Some("project-1"), &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_tool_state(
        Some("project-1"),
        &key,
        "delete_issue",
        McpOverrideState::Disabled,
    )
    .await
    .unwrap();

    assert!(repo.clear_server(Some("project-1"), &key).await.unwrap());
    let policy = repo
        .get_for_project("project-1", &key)
        .await
        .unwrap()
        .expect("tool-only policy remains");
    assert_eq!(policy.server_state, McpOverrideState::Follow);
    assert_eq!(
        policy.tool_states.get("delete_issue"),
        Some(&McpOverrideState::Disabled)
    );
    assert!(!repo.clear_server(Some("project-1"), &key).await.unwrap());
}
