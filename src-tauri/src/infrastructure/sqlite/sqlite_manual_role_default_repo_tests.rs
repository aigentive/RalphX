use crate::domain::agents::{
    AgentHarnessKind, AtlassianMcpAccess, ManualRoleDefault, ManualServiceTier, RoutingRole,
};
use crate::domain::repositories::ManualRoleDefaultRepository;
use crate::testing::SqliteTestDb;

use super::SqliteManualRoleDefaultRepository;

fn setup_repo() -> (SqliteTestDb, SqliteManualRoleDefaultRepository) {
    let db = SqliteTestDb::new("sqlite-manual-role-default-repo");
    let repo = SqliteManualRoleDefaultRepository::from_shared(db.shared_conn());
    (db, repo)
}

fn value(model: &str, tier: ManualServiceTier) -> ManualRoleDefault {
    ManualRoleDefault {
        harness: AgentHarnessKind::Codex,
        model: Some(model.to_string()),
        effort: None,
        service_tier: tier,
        coordination_mode: None,
        persona_id: None,
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        atlassian_access: None,
    }
}

#[tokio::test]
async fn round_trips_whole_values_and_reuses_row_identity() {
    let (_db, repo) = setup_repo();
    let first = repo
        .upsert_global(
            RoutingRole::WorkspaceEdit,
            &value("gpt-first", ManualServiceTier::ProviderDefault),
        )
        .await
        .unwrap();
    let second = repo
        .upsert_global(
            RoutingRole::WorkspaceEdit,
            &value("gpt-second", ManualServiceTier::Standard),
        )
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(second.value.model.as_deref(), Some("gpt-second"));
    assert_eq!(second.value.service_tier, ManualServiceTier::Standard);
}

#[tokio::test]
async fn project_scope_isolated_and_clear_is_specific() {
    let (_db, repo) = setup_repo();
    repo.upsert_for_project(
        "project-a",
        RoutingRole::WorkspaceChat,
        &value("chat-a", ManualServiceTier::Fast),
    )
    .await
    .unwrap();
    repo.upsert_for_project(
        "project-b",
        RoutingRole::WorkspaceChat,
        &value("chat-b", ManualServiceTier::Standard),
    )
    .await
    .unwrap();

    assert!(repo
        .clear_for_project("project-a", RoutingRole::WorkspaceChat)
        .await
        .unwrap());
    assert!(repo
        .get_for_project("project-a", RoutingRole::WorkspaceChat)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .get_for_project("project-b", RoutingRole::WorkspaceChat)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn lists_global_and_project_defaults_in_role_order() {
    let (_db, repo) = setup_repo();
    repo.upsert_global(
        RoutingRole::WorkspaceEdit,
        &value("edit", ManualServiceTier::Standard),
    )
    .await
    .unwrap();
    repo.upsert_global(
        RoutingRole::WorkspaceChat,
        &value("chat", ManualServiceTier::ProviderDefault),
    )
    .await
    .unwrap();
    repo.upsert_for_project(
        "project-a",
        RoutingRole::WorkspacePlan,
        &value("plan", ManualServiceTier::Fast),
    )
    .await
    .unwrap();

    let global = repo.list_global().await.unwrap();
    let project = repo.list_for_project("project-a").await.unwrap();

    assert_eq!(
        global.iter().map(|row| row.role).collect::<Vec<_>>(),
        vec![RoutingRole::WorkspaceChat, RoutingRole::WorkspaceEdit]
    );
    assert_eq!(project.len(), 1);
    assert_eq!(project[0].project_id.as_deref(), Some("project-a"));
    assert_eq!(project[0].role, RoutingRole::WorkspacePlan);
}

#[tokio::test]
async fn clear_global_is_specific_and_reports_absence() {
    let (_db, repo) = setup_repo();
    repo.upsert_global(
        RoutingRole::WorkspaceChat,
        &value("chat", ManualServiceTier::Standard),
    )
    .await
    .unwrap();
    repo.upsert_for_project(
        "project-a",
        RoutingRole::WorkspaceChat,
        &value("project-chat", ManualServiceTier::Fast),
    )
    .await
    .unwrap();

    assert!(repo.clear_global(RoutingRole::WorkspaceChat).await.unwrap());
    assert!(!repo.clear_global(RoutingRole::WorkspaceChat).await.unwrap());
    assert!(repo
        .get_global(RoutingRole::WorkspaceChat)
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .get_for_project("project-a", RoutingRole::WorkspaceChat)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn direct_connection_constructor_uses_the_same_repository_contract() {
    let db = SqliteTestDb::new("sqlite-manual-role-default-repo-direct");
    let repo = SqliteManualRoleDefaultRepository::new(db.new_connection());

    let saved = repo
        .upsert_global(
            RoutingRole::WorkspaceAutomation,
            &value("automation", ManualServiceTier::ProviderDefault),
        )
        .await
        .unwrap();

    assert_eq!(saved.project_id, None);
    assert_eq!(
        repo.get_global(RoutingRole::WorkspaceAutomation)
            .await
            .unwrap()
            .unwrap()
            .value
            .model
            .as_deref(),
        Some("automation")
    );
}

#[tokio::test]
async fn malformed_persisted_value_fails_closed() {
    let (db, repo) = setup_repo();
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO manual_role_defaults (scope_type, scope_id, role, value_json)
             VALUES ('global', '', 'workspace_chat', '{not-json}')",
            [],
        )
        .unwrap();
    });

    assert!(repo.get_global(RoutingRole::WorkspaceChat).await.is_err());
}

#[tokio::test]
async fn atlassian_access_round_trips_through_value_json() {
    let (_db, repo) = setup_repo();
    let mut stored_value = value("gpt-atlassian", ManualServiceTier::ProviderDefault);
    stored_value.atlassian_access = Some(AtlassianMcpAccess::ReadWrite);

    let stored = repo
        .upsert_global(RoutingRole::WorkspaceReviewer, &stored_value)
        .await
        .unwrap();

    assert_eq!(
        stored.value.atlassian_access,
        Some(AtlassianMcpAccess::ReadWrite)
    );

    let fetched = repo
        .get_global(RoutingRole::WorkspaceReviewer)
        .await
        .unwrap()
        .expect("row should exist");
    assert_eq!(
        fetched.value.atlassian_access,
        Some(AtlassianMcpAccess::ReadWrite)
    );
}

#[tokio::test]
async fn omitted_atlassian_access_round_trips_as_none() {
    let (_db, repo) = setup_repo();
    repo.upsert_global(
        RoutingRole::WorkspaceChat,
        &value("gpt-plain", ManualServiceTier::ProviderDefault),
    )
    .await
    .unwrap();

    let fetched = repo
        .get_global(RoutingRole::WorkspaceChat)
        .await
        .unwrap()
        .expect("row should exist");
    assert_eq!(fetched.value.atlassian_access, None);
}

#[test]
fn legacy_value_json_without_atlassian_access_deserializes() {
    let legacy = r#"{
        "harness": "codex",
        "model": "legacy-model",
        "serviceTier": "standard",
        "approvalPolicy": "never",
        "sandboxMode": "danger-full-access"
    }"#;

    let parsed = serde_json::from_str::<ManualRoleDefault>(legacy).unwrap();
    assert_eq!(parsed.model.as_deref(), Some("legacy-model"));
    assert_eq!(parsed.atlassian_access, None);
}

#[test]
fn serialized_value_json_omits_absent_atlassian_access() {
    let json =
        serde_json::to_string(&value("gpt-plain", ManualServiceTier::ProviderDefault)).unwrap();
    assert!(!json.contains("atlassianAccess"), "unexpected key in {json}");

    let mut with_access = value("gpt-plain", ManualServiceTier::ProviderDefault);
    with_access.atlassian_access = Some(AtlassianMcpAccess::Read);
    let json = serde_json::to_string(&with_access).unwrap();
    assert!(json.contains("\"atlassianAccess\":\"read\""), "{json}");
}
