use crate::domain::agents::{AgentHarnessKind, ManualRoleDefault, ManualServiceTier, RoutingRole};
use crate::domain::repositories::ManualRoleDefaultRepository;

use super::MemoryManualRoleDefaultRepository;

fn value(model: &str) -> ManualRoleDefault {
    ManualRoleDefault {
        harness: AgentHarnessKind::Codex,
        model: Some(model.to_string()),
        effort: None,
        service_tier: ManualServiceTier::ProviderDefault,
        coordination_mode: None,
        persona_id: None,
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        atlassian_access: None,
    }
}

#[tokio::test]
async fn global_and_project_rows_are_isolated_whole_values() {
    let repo = MemoryManualRoleDefaultRepository::new();
    repo.upsert_global(RoutingRole::WorkspaceEdit, &value("gpt-global"))
        .await
        .unwrap();
    repo.upsert_for_project(
        "project-a",
        RoutingRole::WorkspaceEdit,
        &value("gpt-project"),
    )
    .await
    .unwrap();

    let global = repo
        .get_global(RoutingRole::WorkspaceEdit)
        .await
        .unwrap()
        .unwrap();
    let project = repo
        .get_for_project("project-a", RoutingRole::WorkspaceEdit)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(global.value.model.as_deref(), Some("gpt-global"));
    assert_eq!(project.value.model.as_deref(), Some("gpt-project"));
    assert!(repo
        .get_for_project("project-b", RoutingRole::WorkspaceEdit)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn clear_removes_only_the_requested_scope_and_role() {
    let repo = MemoryManualRoleDefaultRepository::new();
    repo.upsert_global(RoutingRole::WorkspaceChat, &value("chat"))
        .await
        .unwrap();
    repo.upsert_global(RoutingRole::WorkspaceEdit, &value("edit"))
        .await
        .unwrap();

    assert!(repo.clear_global(RoutingRole::WorkspaceChat).await.unwrap());
    assert!(!repo.clear_global(RoutingRole::WorkspaceChat).await.unwrap());
    assert!(repo
        .get_global(RoutingRole::WorkspaceEdit)
        .await
        .unwrap()
        .is_some());
}
