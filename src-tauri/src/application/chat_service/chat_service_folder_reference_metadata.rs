use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::application::conversation_folder_reference_service::ConversationFolderReferenceService;
use crate::domain::entities::ChatConversationId;
use crate::domain::repositories::ConversationFolderReferenceRepository;

const FOLDER_REFERENCE_METADATA_KEY: &str = "composer_folder_references";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FolderReferenceSnapshot {
    id: String,
    folder_path: String,
    display_name: String,
}

pub(super) async fn snapshot_live_folder_references_in_metadata(
    metadata: Option<String>,
    conversation_id: &ChatConversationId,
    repository: Option<Arc<dyn ConversationFolderReferenceRepository>>,
    app_data_dir: Option<&Path>,
) -> Option<String> {
    let (Some(repository), Some(app_data_dir)) = (repository, app_data_dir) else {
        return metadata;
    };
    if metadata_has_folder_snapshot(metadata.as_deref()) {
        return metadata;
    }

    let service = ConversationFolderReferenceService::new(
        repository,
        app_data_dir.to_path_buf(),
        crate::infrastructure::agents::limits_config().max_live_folder_references,
    );
    let references = match service.list_live_validated(conversation_id).await {
        Ok(result) => result.references,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                %error,
                "failed to snapshot live folder references for message history"
            );
            return metadata;
        }
    };
    if references.is_empty() {
        return metadata;
    }

    let snapshots = references
        .into_iter()
        .map(|reference| FolderReferenceSnapshot {
            id: reference.id.as_str(),
            folder_path: reference.folder_path,
            display_name: reference.display_name,
        })
        .collect::<Vec<_>>();
    let mut value = match metadata {
        Some(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .unwrap_or_else(|_| serde_json::json!({ "raw_metadata": raw })),
        None => serde_json::Value::Object(serde_json::Map::new()),
    };
    if !value.is_object() {
        value = serde_json::json!({ "metadata": value });
    }
    let object = value.as_object_mut()?;
    object.insert(
        FOLDER_REFERENCE_METADATA_KEY.to_string(),
        serde_json::to_value(snapshots).ok()?,
    );
    Some(value.to_string())
}

fn metadata_has_folder_snapshot(metadata: Option<&str>) -> bool {
    let Some(raw) = metadata else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    value
        .as_object()
        .is_some_and(|object| object.contains_key(FOLDER_REFERENCE_METADATA_KEY))
}
