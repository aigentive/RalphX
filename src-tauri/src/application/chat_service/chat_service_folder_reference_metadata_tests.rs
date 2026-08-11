use std::fs;
use std::sync::Arc;

use tempfile::tempdir;

use super::chat_service_folder_reference_metadata::snapshot_live_folder_references_in_metadata;
use crate::application::conversation_folder_reference_service::ConversationFolderReferenceService;
use crate::domain::entities::ChatConversationId;
use crate::infrastructure::memory::MemoryConversationFolderReferenceRepository;

#[tokio::test]
async fn snapshots_only_currently_valid_live_folders_into_message_metadata() {
    let root = tempdir().expect("temp root");
    let app_data_dir = root.path().join("app-data");
    let referenced_folder = root.path().join("brand-kit");
    fs::create_dir_all(&app_data_dir).expect("app data dir");
    fs::create_dir_all(&referenced_folder).expect("referenced folder");

    let conversation_id = ChatConversationId::from_string("conversation-1");
    let repository = Arc::new(MemoryConversationFolderReferenceRepository::new());
    let service =
        ConversationFolderReferenceService::new(repository.clone(), app_data_dir.clone(), 6);
    let reference = service
        .add(conversation_id, &referenced_folder, "brand-kit".to_string())
        .await
        .expect("register folder");

    let metadata = snapshot_live_folder_references_in_metadata(
        Some(r#"{"source":"composer"}"#.to_string()),
        &conversation_id,
        Some(repository.clone()),
        Some(&app_data_dir),
    )
    .await;
    let value: serde_json::Value =
        serde_json::from_str(metadata.as_deref().expect("folder snapshot metadata"))
            .expect("valid metadata");

    assert_eq!(value["source"], "composer");
    assert_eq!(
        value["composer_folder_references"][0]["id"],
        reference.id.as_str()
    );
    assert_eq!(
        value["composer_folder_references"][0]["folderPath"],
        referenced_folder
            .canonicalize()
            .expect("canonical folder")
            .to_string_lossy()
            .as_ref()
    );
    assert_eq!(
        value["composer_folder_references"][0]["displayName"],
        "brand-kit"
    );

    service
        .remove(&reference.id, &conversation_id)
        .await
        .expect("remove folder");
    let after_removal = snapshot_live_folder_references_in_metadata(
        Some(r#"{"source":"composer"}"#.to_string()),
        &conversation_id,
        Some(repository),
        Some(&app_data_dir),
    )
    .await;
    let after_removal: serde_json::Value =
        serde_json::from_str(after_removal.as_deref().expect("original metadata remains"))
            .expect("valid metadata");
    assert_eq!(after_removal["source"], "composer");
    assert!(
        after_removal.get("composer_folder_references").is_none(),
        "removed folders must not be attributed to later messages"
    );
}

#[tokio::test]
async fn excludes_unavailable_folder_without_losing_existing_metadata() {
    let root = tempdir().expect("temp root");
    let app_data_dir = root.path().join("app-data");
    let referenced_folder = root.path().join("temporary-context");
    fs::create_dir_all(&app_data_dir).expect("app data dir");
    fs::create_dir_all(&referenced_folder).expect("referenced folder");

    let conversation_id = ChatConversationId::from_string("conversation-2");
    let repository = Arc::new(MemoryConversationFolderReferenceRepository::new());
    ConversationFolderReferenceService::new(repository.clone(), app_data_dir.clone(), 6)
        .add(
            conversation_id,
            &referenced_folder,
            "temporary-context".to_string(),
        )
        .await
        .expect("register folder");
    fs::remove_dir_all(&referenced_folder).expect("remove referenced folder");

    let metadata = snapshot_live_folder_references_in_metadata(
        Some(r#"{"source":"composer"}"#.to_string()),
        &conversation_id,
        Some(repository),
        Some(&app_data_dir),
    )
    .await;
    let value: serde_json::Value =
        serde_json::from_str(metadata.as_deref().expect("original metadata remains"))
            .expect("valid metadata");

    assert_eq!(value["source"], "composer");
    assert!(
        value.get("composer_folder_references").is_none(),
        "unavailable folders must not be represented as readable history context"
    );
}

#[tokio::test]
async fn preserves_an_existing_folder_snapshot_when_live_references_change() {
    let root = tempdir().expect("temp root");
    let app_data_dir = root.path().join("app-data");
    fs::create_dir_all(&app_data_dir).expect("app data dir");

    let conversation_id = ChatConversationId::from_string("conversation-3");
    let repository = Arc::new(MemoryConversationFolderReferenceRepository::new());
    let existing = r#"{"composer_folder_references":[{"id":"folder-original","folderPath":"/work/original","displayName":"original"}],"source":"queued"}"#;

    let metadata = snapshot_live_folder_references_in_metadata(
        Some(existing.to_string()),
        &conversation_id,
        Some(repository),
        Some(&app_data_dir),
    )
    .await;

    assert_eq!(
        metadata.as_deref(),
        Some(existing),
        "queue replay must retain the folder context captured when the message was accepted"
    );
}
