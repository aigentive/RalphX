use chrono::Utc;

use super::SqliteTicketingStatusCatalogRepository;
use crate::domain::integrations::{
    TicketingStatusCatalogRepository, TicketingStatusCatalogUpsert,
    TicketingStatusPresentationPatch,
};
use crate::infrastructure::sqlite::{open_memory_connection, run_migrations};

fn repo() -> SqliteTicketingStatusCatalogRepository {
    let conn = open_memory_connection().expect("memory db opens");
    run_migrations(&conn).expect("migrations run");
    SqliteTicketingStatusCatalogRepository::new(conn)
}

fn upsert(
    provider_status_id: &str,
    name: &str,
    display_order: i64,
) -> TicketingStatusCatalogUpsert {
    TicketingStatusCatalogUpsert {
        provider: "linear".to_string(),
        scope_kind: "linear_team".to_string(),
        scope_id: "team-1".to_string(),
        provider_status_id: provider_status_id.to_string(),
        provider_status_name: name.to_string(),
        provider_category: "todo".to_string(),
        provider_color: Some("#123456".to_string()),
        provider_order: Some(display_order),
        display_order,
        is_terminal: false,
        last_seen_at: Utc::now(),
        metadata_json: None,
    }
}

#[tokio::test]
async fn upsert_preserves_presentation_fields_when_refreshed_with_same_display_order() {
    let repo = repo();
    repo.upsert_status_catalog_entry(upsert("state-1", "Backlog", 0))
        .await
        .unwrap();
    repo.update_status_presentation(
        "linear",
        "linear_team",
        "team-1",
        vec![TicketingStatusPresentationPatch {
            provider_status_id: "state-1".to_string(),
            display_order: Some(7),
            color_override: Some(Some("#abcdef".to_string())),
            is_visible: Some(false),
        }],
    )
    .await
    .unwrap();

    repo.upsert_status_catalog_entry(TicketingStatusCatalogUpsert {
        provider_status_name: "Ready".to_string(),
        provider_color: Some("#654321".to_string()),
        display_order: 7,
        ..upsert("state-1", "Backlog", 0)
    })
    .await
    .unwrap();

    let entries = repo
        .list_status_catalog("linear", "linear_team", "team-1")
        .await
        .unwrap();
    let entry = entries.first().expect("entry exists");
    assert_eq!(entry.provider_status_name, "Ready");
    assert_eq!(entry.provider_color.as_deref(), Some("#654321"));
    assert_eq!(entry.display_order, 7);
    assert_eq!(entry.color_override.as_deref(), Some("#abcdef"));
    assert!(!entry.is_visible);
    assert!(entry.stale_since.is_none());
}

#[tokio::test]
async fn missing_statuses_are_marked_stale_without_deleting_them() {
    let repo = repo();
    repo.upsert_status_catalog_entry(upsert("state-1", "Backlog", 0))
        .await
        .unwrap();
    repo.upsert_status_catalog_entry(upsert("state-2", "Done", 1))
        .await
        .unwrap();

    let entries = repo
        .mark_missing_statuses_stale(
            "linear",
            "linear_team",
            "team-1",
            &["state-1".to_string()],
            Utc::now(),
        )
        .await
        .unwrap();

    assert_eq!(entries.len(), 2);
    assert!(entries
        .iter()
        .find(|entry| entry.provider_status_id == "state-1")
        .unwrap()
        .stale_since
        .is_none());
    assert!(entries
        .iter()
        .find(|entry| entry.provider_status_id == "state-2")
        .unwrap()
        .stale_since
        .is_some());
}
