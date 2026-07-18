use crate::domain::agents::{AgentHarnessKind, McpOverrideState, McpServerKey};
use crate::domain::repositories::McpPolicyRepository;
use crate::testing::SqliteTestDb;

use super::SqliteMcpPolicyRepository;

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
