use crate::domain::agents::{AgentHarnessKind, McpOverrideState, McpServerKey};
use crate::domain::repositories::McpPolicyRepository;

use super::MemoryMcpPolicyRepository;

#[tokio::test]
async fn global_and_project_rows_stay_isolated_and_follow_clear_reveals_lower_scope() {
    let repo = MemoryMcpPolicyRepository::new();
    let key = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();

    repo.set_server_state(None, &key, McpOverrideState::Disabled)
        .await
        .unwrap();
    repo.set_server_state(Some("project-1"), &key, McpOverrideState::Enabled)
        .await
        .unwrap();

    assert_eq!(
        repo.get_global(&key).await.unwrap().unwrap().server_state,
        McpOverrideState::Disabled
    );
    assert_eq!(
        repo.get_for_project("project-1", &key)
            .await
            .unwrap()
            .unwrap()
            .server_state,
        McpOverrideState::Enabled
    );

    assert!(repo.clear_server(Some("project-1"), &key).await.unwrap());
    assert!(repo
        .get_for_project("project-1", &key)
        .await
        .unwrap()
        .is_none());
    assert!(repo.get_global(&key).await.unwrap().is_some());
}

#[tokio::test]
async fn required_server_disables_fail_without_writing() {
    let repo = MemoryMcpPolicyRepository::new();
    let key = McpServerKey::new(AgentHarnessKind::Codex, "ralphx_internal").unwrap();

    assert!(repo
        .set_tool_state(None, &key, "list_agent_tasks", McpOverrideState::Disabled)
        .await
        .is_err());
    assert!(repo.get_global(&key).await.unwrap().is_none());
}
