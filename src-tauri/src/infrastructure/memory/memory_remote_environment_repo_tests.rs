use ralphx_remote_protocol::{Scope, PROTOCOL_VERSION};

use super::memory_remote_environment_repo::MemoryRemoteEnvironmentRepository;
use crate::domain::entities::remote_environment::{RemoteEnvironmentId, RemoteEnvironmentStatus};
use crate::domain::repositories::{RemoteEnvironmentRepository, UpsertPairedEnvironment};

fn paired(environment_id: &str, url: &str) -> UpsertPairedEnvironment {
    UpsertPairedEnvironment {
        environment_id: environment_id.to_string(),
        name: environment_id.to_string(),
        url: url.to_string(),
        scopes: vec![Scope::UiRead],
        protocol_version: PROTOCOL_VERSION,
    }
}

#[tokio::test]
async fn default_repository_is_empty_and_working() {
    let repo = MemoryRemoteEnvironmentRepository::default();

    assert!(repo.list().await.expect("list").is_empty());
    let inserted = repo
        .upsert_paired(paired("env-default", "https://default.test"))
        .await
        .expect("insert");
    assert_eq!(repo.get(&inserted.id).await.expect("get"), Some(inserted));
}

#[tokio::test]
async fn environment_identity_lookup_covers_found_and_missing_rows() {
    let repo = MemoryRemoteEnvironmentRepository::new();
    let inserted = repo
        .upsert_paired(paired("env-lookup", "https://lookup.test"))
        .await
        .expect("insert");

    assert_eq!(
        repo.get_by_environment_id("env-lookup")
            .await
            .expect("lookup"),
        Some(inserted)
    );
    assert!(repo
        .get_by_environment_id("missing")
        .await
        .expect("missing lookup")
        .is_none());
}

#[tokio::test]
async fn list_orders_distinct_rows_by_created_at_then_id() {
    let repo = MemoryRemoteEnvironmentRepository::new();
    repo.upsert_paired(paired("env-b", "https://b.test"))
        .await
        .expect("insert b");
    repo.upsert_paired(paired("env-a", "https://a.test"))
        .await
        .expect("insert a");

    let rows = repo.list().await.expect("list");
    assert_eq!(rows.len(), 2);
    assert!(rows.windows(2).all(|pair| {
        (pair[0].created_at.as_str(), pair[0].id.as_str())
            <= (pair[1].created_at.as_str(), pair[1].id.as_str())
    }));
}

#[tokio::test]
async fn touch_updates_an_existing_row_and_ignores_a_missing_row() {
    let repo = MemoryRemoteEnvironmentRepository::new();
    let inserted = repo
        .upsert_paired(paired("env-touch", "https://touch.test"))
        .await
        .expect("insert");
    let timestamp = "2026-07-28T10:11:12+00:00";

    repo.touch_last_connected(&inserted.id, timestamp)
        .await
        .expect("touch existing");
    assert_eq!(
        repo.get(&inserted.id)
            .await
            .expect("get")
            .expect("row")
            .last_connected_at
            .as_deref(),
        Some(timestamp)
    );

    let missing = RemoteEnvironmentId::from_string("missing");
    repo.touch_last_connected(&missing, timestamp)
        .await
        .expect("touch missing is idempotent");
    assert!(repo.get(&missing).await.expect("get missing").is_none());
}

#[tokio::test]
async fn re_pairing_the_base_url_does_not_add_a_candidate() {
    let repo = MemoryRemoteEnvironmentRepository::new();
    let first = repo
        .upsert_paired(paired("env-same", "https://same.test"))
        .await
        .expect("first insert");
    let second = repo
        .upsert_paired(paired("env-same", "https://same.test"))
        .await
        .expect("second insert");

    assert_eq!(second.id, first.id);
    assert!(second.candidate_urls.is_empty());
    assert_eq!(second.status, RemoteEnvironmentStatus::PendingAdd);
}

#[tokio::test]
async fn missing_row_operations_keep_their_documented_semantics() {
    let repo = MemoryRemoteEnvironmentRepository::new();
    let missing = RemoteEnvironmentId::from_string("missing");

    assert!(repo.get(&missing).await.expect("get").is_none());
    assert!(repo
        .set_status(&missing, RemoteEnvironmentStatus::Active)
        .await
        .is_err());
    repo.delete(&missing).await.expect("delete is idempotent");
    assert!(repo.list().await.expect("list").is_empty());
}
