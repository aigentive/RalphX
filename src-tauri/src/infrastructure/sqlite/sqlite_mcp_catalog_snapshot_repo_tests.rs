use crate::domain::repositories::{McpCatalogSnapshot, McpCatalogSnapshotRepository};
use crate::testing::SqliteTestDb;

use super::SqliteMcpCatalogSnapshotRepository;

fn full_response_json(server_id: &str) -> String {
    format!(
        r#"{{"eligible_providers":["codex"],"eligible_default_provider":"codex","probed_at":"2026-08-05T18:00:00+00:00","probe_stale":false,"provider_diagnostics":{{"codex":"fallback"}},"policy_diagnostics":["policy warning"],"servers":[{{"provider":"codex","server_id":"{server_id}","native_scope":"user","native_state":"enabled","effective_enabled":true,"configured_state":"follow","effective_state":"enabled","effective_source":"provider_native","known_tools":[{{"tool_name":"search","configured_state":"follow","effective_state":"enabled","effective_source":"provider_native"}}],"disabled_tools":[],"locked":false,"locked_reason":null,"diagnostic":null,"conflict_kind":null,"repair_status":null}}]}}"#
    )
}

#[tokio::test]
async fn snapshot_round_trips_full_serialized_response_verbatim_and_scopes_are_distinct() {
    let db = SqliteTestDb::new("mcp-catalog-snapshot-round-trip");
    let repo = SqliteMcpCatalogSnapshotRepository::from_shared(db.shared_conn());
    let global = McpCatalogSnapshot {
        scope_project_id: None,
        provider: "codex".to_string(),
        response_json: full_response_json("global-server"),
        captured_at: "2026-08-05T18:01:00+00:00".to_string(),
    };
    let project = McpCatalogSnapshot {
        scope_project_id: Some("project-1".to_string()),
        provider: "codex".to_string(),
        response_json: full_response_json("project-server"),
        captured_at: "2026-08-05T18:02:00+00:00".to_string(),
    };

    assert_eq!(repo.upsert(global.clone()).await.unwrap(), global);
    assert_eq!(repo.upsert(project.clone()).await.unwrap(), project);
    assert_eq!(repo.get(None, "codex").await.unwrap(), Some(global));
    assert_eq!(
        repo.get(Some("project-1"), "codex").await.unwrap(),
        Some(project)
    );
}

#[tokio::test]
async fn snapshot_upsert_replaces_the_same_scope_and_provider() {
    let db = SqliteTestDb::new("mcp-catalog-snapshot-upsert");
    let repo = SqliteMcpCatalogSnapshotRepository::from_shared(db.shared_conn());
    let mut snapshot = McpCatalogSnapshot {
        scope_project_id: None,
        provider: "claude".to_string(),
        response_json: full_response_json("first"),
        captured_at: "2026-08-05T18:01:00+00:00".to_string(),
    };
    repo.upsert(snapshot.clone()).await.unwrap();
    snapshot.response_json = full_response_json("second");
    snapshot.captured_at = "2026-08-05T18:03:00+00:00".to_string();
    repo.upsert(snapshot.clone()).await.unwrap();

    assert_eq!(repo.get(None, "claude").await.unwrap(), Some(snapshot));
}
