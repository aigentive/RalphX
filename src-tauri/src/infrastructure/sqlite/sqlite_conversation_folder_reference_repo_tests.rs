use crate::domain::entities::{ChatConversationId, ConversationFolderReference};
use crate::domain::repositories::ConversationFolderReferenceRepository;
use crate::error::AppError;
use crate::infrastructure::sqlite::run_migrations;

use super::sqlite_conversation_folder_reference_repo::SqliteConversationFolderReferenceRepository;

fn repository() -> SqliteConversationFolderReferenceRepository {
    let connection = rusqlite::Connection::open_in_memory().expect("open SQLite");
    run_migrations(&connection).expect("run migrations");
    connection
        .execute("PRAGMA foreign_keys = OFF", [])
        .expect("disable unrelated FK setup");
    SqliteConversationFolderReferenceRepository::new(connection)
}

#[tokio::test]
async fn sqlite_conversation_folder_reference_repo_enforces_live_cap_and_soft_remove() {
    let repository = repository();
    let conversation_id = ChatConversationId::new();
    let first = repository
        .create_if_below_live_cap(
            ConversationFolderReference::new(conversation_id, "/folder/one", "One"),
            1,
        )
        .await
        .expect("first insert succeeds");
    let capped = repository
        .create_if_below_live_cap(
            ConversationFolderReference::new(conversation_id, "/folder/two", "Two"),
            1,
        )
        .await;
    assert!(matches!(
        capped,
        Err(AppError::ConversationFolderReferenceLimit { limit: 1, .. })
    ));
    assert!(repository
        .soft_remove(&first.id, &conversation_id)
        .await
        .expect("soft remove succeeds"));
    repository
        .create_if_below_live_cap(
            ConversationFolderReference::new(conversation_id, "/folder/two", "Two"),
            1,
        )
        .await
        .expect("replacement succeeds");
    assert_eq!(
        repository
            .list_live(&conversation_id)
            .await
            .expect("list live")
            .len(),
        1
    );
}

#[tokio::test]
async fn sqlite_conversation_folder_reference_repo_maps_duplicate_and_allows_readd() {
    let repository = repository();
    let conversation_id = ChatConversationId::new();
    let first = repository
        .create_if_below_live_cap(
            ConversationFolderReference::new(conversation_id, "/folder/one", "One"),
            1,
        )
        .await
        .expect("first insert succeeds");
    let duplicate = repository
        .create_if_below_live_cap(
            ConversationFolderReference::new(conversation_id, "/folder/one", "Duplicate"),
            1,
        )
        .await;
    assert!(matches!(
        duplicate,
        Err(AppError::ConversationFolderReferenceDuplicate { .. })
    ));

    repository
        .soft_remove(&first.id, &conversation_id)
        .await
        .expect("soft remove succeeds");
    repository
        .create_if_below_live_cap(
            ConversationFolderReference::new(conversation_id, "/folder/one", "Re-added"),
            1,
        )
        .await
        .expect("soft-removed path can be re-added");
}
