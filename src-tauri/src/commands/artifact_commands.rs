// Tauri commands for Artifact CRUD operations
// Thin layer that delegates to ArtifactRepository and ArtifactBucketRepository

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};

use crate::application::AppState;
use crate::domain::entities::{
    Artifact, ArtifactBucket, ArtifactBucketId, ArtifactContent, ArtifactId, ArtifactRelation,
    ArtifactRelationType, ArtifactType, ProcessId, TaskId,
};

/// Summary of a team artifact (lighter than full ArtifactResponse)
#[derive(Debug, Serialize)]
pub struct TeamArtifactSummaryResponse {
    pub id: String,
    pub name: String,
    pub artifact_type: String,
    pub version: u32,
    pub content_preview: String,
    pub created_at: String,
    pub author_teammate: Option<String>,
}

/// Response for get_team_artifacts_by_session
#[derive(Debug, Serialize)]
pub struct GetTeamArtifactsResponse {
    pub artifacts: Vec<TeamArtifactSummaryResponse>,
    pub count: usize,
}

/// Input for creating a new artifact
#[derive(Debug, Deserialize)]
pub struct CreateArtifactInput {
    pub name: String,
    pub artifact_type: String,
    pub content_type: String, // "inline" or "file"
    pub content: String,      // text for inline, path for file
    pub created_by: String,
    pub bucket_id: Option<String>,
    pub task_id: Option<String>,
    pub process_id: Option<String>,
    pub derived_from: Option<Vec<String>>,
}

/// Input for updating an artifact
#[derive(Debug, Deserialize)]
pub struct UpdateArtifactInput {
    pub name: Option<String>,
    pub content_type: Option<String>,
    pub content: Option<String>,
    pub bucket_id: Option<String>,
}

/// Input for creating a bucket
#[derive(Debug, Deserialize)]
pub struct CreateBucketInput {
    pub name: String,
    pub accepted_types: Option<Vec<String>>,
    pub writers: Option<Vec<String>>,
    pub readers: Option<Vec<String>>,
}

/// Input for adding an artifact relation
#[derive(Debug, Deserialize)]
pub struct AddRelationInput {
    pub from_artifact_id: String,
    pub to_artifact_id: String,
    pub relation_type: String, // "derived_from" or "related_to"
}

/// Response wrapper for artifact operations
#[derive(Debug, Serialize)]
pub struct ArtifactResponse {
    pub id: String,
    pub name: String,
    pub artifact_type: String,
    pub content_type: String,
    pub content: String,
    pub created_at: String,
    pub created_by: String,
    pub version: u32,
    pub bucket_id: Option<String>,
    pub task_id: Option<String>,
    pub process_id: Option<String>,
    pub derived_from: Vec<String>,
}

impl From<Artifact> for ArtifactResponse {
    fn from(artifact: Artifact) -> Self {
        let (content_type, content) = match &artifact.content {
            ArtifactContent::Inline { text } => ("inline".to_string(), text.clone()),
            ArtifactContent::File { path } => ("file".to_string(), path.clone()),
        };

        Self {
            id: artifact.id.as_str().to_string(),
            name: artifact.name,
            artifact_type: artifact.artifact_type.to_string(),
            content_type,
            content,
            created_at: artifact.metadata.created_at.to_rfc3339(),
            created_by: artifact.metadata.created_by,
            version: artifact.metadata.version,
            bucket_id: artifact.bucket_id.map(|id| id.as_str().to_string()),
            task_id: artifact.metadata.task_id.map(|id| id.as_str().to_string()),
            process_id: artifact
                .metadata
                .process_id
                .map(|id| id.as_str().to_string()),
            derived_from: artifact
                .derived_from
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
        }
    }
}

/// Response wrapper for bucket operations
#[derive(Debug, Serialize)]
pub struct BucketResponse {
    pub id: String,
    pub name: String,
    pub accepted_types: Vec<String>,
    pub writers: Vec<String>,
    pub readers: Vec<String>,
    pub is_system: bool,
}

impl From<ArtifactBucket> for BucketResponse {
    fn from(bucket: ArtifactBucket) -> Self {
        Self {
            id: bucket.id.as_str().to_string(),
            name: bucket.name,
            accepted_types: bucket
                .accepted_types
                .iter()
                .map(|t| t.to_string())
                .collect(),
            writers: bucket.writers,
            readers: bucket.readers,
            is_system: bucket.is_system,
        }
    }
}

/// Summary of an artifact version for history display
#[derive(Debug, Serialize)]
pub struct ArtifactVersionSummaryResponse {
    pub id: String,
    pub version: u32,
    pub name: String,
    pub created_at: String,
    pub created_by: String,
    pub metadata: Option<serde_json::Value>,
}

impl From<crate::domain::repositories::ArtifactVersionSummary> for ArtifactVersionSummaryResponse {
    fn from(summary: crate::domain::repositories::ArtifactVersionSummary) -> Self {
        Self {
            id: summary.id.as_str().to_string(),
            version: summary.version,
            name: summary.name,
            created_at: summary.created_at.to_rfc3339(),
            created_by: summary.created_by,
            metadata: summary.metadata,
        }
    }
}

/// Response wrapper for artifact relation operations
#[derive(Debug, Serialize)]
pub struct ArtifactRelationResponse {
    pub id: String,
    pub from_artifact_id: String,
    pub to_artifact_id: String,
    pub relation_type: String,
}

impl From<ArtifactRelation> for ArtifactRelationResponse {
    fn from(relation: ArtifactRelation) -> Self {
        Self {
            id: relation.id.as_str().to_string(),
            from_artifact_id: relation.from_artifact_id.as_str().to_string(),
            to_artifact_id: relation.to_artifact_id.as_str().to_string(),
            relation_type: relation.relation_type.to_string(),
        }
    }
}

// ===== Artifact Commands =====

/// Get all artifacts (optionally filtered by type)
#[tauri::command]
pub async fn get_artifacts(
    artifact_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<ArtifactResponse>, String> {
    match artifact_type {
        Some(type_str) => {
            let parsed_type: ArtifactType = type_str
                .parse()
                .map_err(|_| format!("Invalid artifact type: {}", type_str))?;
            state
                .artifact_repo
                .get_by_type(parsed_type)
                .await
                .map(|artifacts| artifacts.into_iter().map(ArtifactResponse::from).collect())
                .map_err(|e| e.to_string())
        }
        None => {
            // No filtering - we'll need to iterate through types
            // For now, return empty since there's no get_all method
            // This is a limitation - in practice you'd want a proper get_all
            Ok(vec![])
        }
    }
}

/// Get a single artifact by ID
#[tauri::command]
pub async fn get_artifact(
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<ArtifactResponse>, String> {
    let artifact_id = ArtifactId::from_string(id);
    state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .map(|opt| opt.map(ArtifactResponse::from))
        .map_err(|e| e.to_string())
}

/// Get an artifact at a specific version
#[tauri::command]
pub async fn get_artifact_at_version(
    id: String,
    version: u32,
    state: State<'_, AppState>,
) -> Result<Option<ArtifactResponse>, String> {
    let artifact_id = ArtifactId::from_string(id);
    state
        .artifact_repo
        .get_by_id_at_version(&artifact_id, version)
        .await
        .map(|opt| opt.map(ArtifactResponse::from))
        .map_err(|e| e.to_string())
}

/// Create a new artifact
#[tauri::command]
pub async fn create_artifact(
    input: CreateArtifactInput,
    state: State<'_, AppState>,
) -> Result<ArtifactResponse, String> {
    // Parse artifact type
    let artifact_type: ArtifactType = input
        .artifact_type
        .parse()
        .map_err(|_| format!("Invalid artifact type: {}", input.artifact_type))?;

    // Create artifact based on content type
    let mut artifact = match input.content_type.as_str() {
        "inline" => Artifact::new_inline(
            &input.name,
            artifact_type,
            &input.content,
            &input.created_by,
        ),
        "file" => Artifact::new_file(
            &input.name,
            artifact_type,
            &input.content,
            &input.created_by,
        ),
        _ => return Err(format!("Invalid content type: {}", input.content_type)),
    };

    // Add optional fields
    if let Some(bucket_id_str) = input.bucket_id {
        artifact = artifact.with_bucket(ArtifactBucketId::from_string(bucket_id_str));
    }
    if let Some(task_id_str) = input.task_id {
        artifact = artifact.with_task(TaskId::from_string(task_id_str));
    }
    if let Some(process_id_str) = input.process_id {
        artifact = artifact.with_process(ProcessId::from_string(process_id_str));
    }
    if let Some(derived_from) = input.derived_from {
        for parent_id in derived_from {
            artifact = artifact.derived_from_artifact(ArtifactId::from_string(parent_id));
        }
    }

    state
        .artifact_repo
        .create(artifact)
        .await
        .map(ArtifactResponse::from)
        .map_err(|e| e.to_string())
}

/// Update an existing artifact
#[tauri::command]
pub async fn update_artifact(
    id: String,
    input: UpdateArtifactInput,
    state: State<'_, AppState>,
) -> Result<ArtifactResponse, String> {
    let artifact_id = ArtifactId::from_string(id);

    // Get existing artifact
    let mut artifact = state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Artifact not found: {}", artifact_id.as_str()))?;

    // Apply updates
    if let Some(name) = input.name {
        artifact.name = name;
    }
    if let (Some(content_type), Some(content)) = (input.content_type, input.content) {
        artifact.content = match content_type.as_str() {
            "inline" => ArtifactContent::inline(content),
            "file" => ArtifactContent::file(content),
            _ => return Err(format!("Invalid content type: {}", content_type)),
        };
    }
    if let Some(bucket_id_str) = input.bucket_id {
        artifact.bucket_id = Some(ArtifactBucketId::from_string(bucket_id_str));
    }

    state
        .artifact_repo
        .update(&artifact)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ArtifactResponse::from(artifact))
}

/// Archive an artifact
///
/// Sets the archived_at timestamp, hiding the artifact from normal views.
///
/// # Arguments
/// * `artifact_id` - The artifact ID to archive
/// * `app` - Tauri app handle for event emission
///
/// # Returns
/// * `ArtifactResponse` - The archived artifact
///
/// # Events
/// * Emits 'artifact:archived' with the artifact ID
#[tauri::command]
pub async fn archive_artifact(
    artifact_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<ArtifactResponse, String> {
    let id = ArtifactId::from_string(artifact_id);
    let archived = state
        .artifact_repo
        .archive(&id)
        .await
        .map_err(|e| e.to_string())?;
    app.emit("artifact:archived", archived.id.as_str()).ok();
    Ok(ArtifactResponse::from(archived))
}

/// Delete an artifact
#[tauri::command]
pub async fn delete_artifact(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let artifact_id = ArtifactId::from_string(id);
    state
        .artifact_repo
        .delete(&artifact_id)
        .await
        .map_err(|e| e.to_string())
}

/// Get artifacts by bucket
#[tauri::command]
pub async fn get_artifacts_by_bucket(
    bucket_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ArtifactResponse>, String> {
    let bucket_id = ArtifactBucketId::from_string(bucket_id);
    state
        .artifact_repo
        .get_by_bucket(&bucket_id)
        .await
        .map(|artifacts| artifacts.into_iter().map(ArtifactResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Get artifacts by task
#[tauri::command]
pub async fn get_artifacts_by_task(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ArtifactResponse>, String> {
    let task_id = TaskId::from_string(task_id);
    state
        .artifact_repo
        .get_by_task(&task_id)
        .await
        .map(|artifacts| artifacts.into_iter().map(ArtifactResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Get team artifacts filtered by session ID.
///
/// Fetches artifacts from the "team-findings" bucket and filters by session_id
/// in team_metadata. Returns summary responses with content previews.
#[tauri::command]
pub async fn get_team_artifacts_by_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<GetTeamArtifactsResponse, String> {
    let bucket_id = ArtifactBucketId::from_string("team-findings");
    let artifacts = state
        .artifact_repo
        .get_by_bucket(&bucket_id)
        .await
        .map_err(|e| e.to_string())?;

    let filtered: Vec<TeamArtifactSummaryResponse> = artifacts
        .into_iter()
        .filter(|a| {
            a.metadata
                .team_metadata
                .as_ref()
                .and_then(|tm| tm.session_id.as_deref())
                == Some(session_id.as_str())
        })
        .map(|a| {
            let content_preview = match &a.content {
                ArtifactContent::Inline { text } => {
                    if text.chars().count() <= 200 {
                        text.clone()
                    } else {
                        let truncated: String = text.chars().take(200).collect();
                        format!("{truncated}...")
                    }
                }
                ArtifactContent::File { path } => format!("[File: {}]", path),
            };
            let author_teammate = a
                .metadata
                .team_metadata
                .as_ref()
                .map(|tm| tm.author_teammate.clone());
            TeamArtifactSummaryResponse {
                id: a.id.as_str().to_string(),
                name: a.name.clone(),
                artifact_type: format!("{:?}", a.artifact_type),
                version: a.metadata.version,
                content_preview,
                created_at: a.metadata.created_at.to_rfc3339(),
                author_teammate,
            }
        })
        .collect();

    let count = filtered.len();
    Ok(GetTeamArtifactsResponse {
        artifacts: filtered,
        count,
    })
}

/// Get version history for an artifact
#[tauri::command]
pub async fn get_artifact_version_history(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ArtifactVersionSummaryResponse>, String> {
    let artifact_id = ArtifactId::from_string(id);
    state
        .artifact_repo
        .get_version_history(&artifact_id)
        .await
        .map(|history| {
            history
                .into_iter()
                .map(ArtifactVersionSummaryResponse::from)
                .collect()
        })
        .map_err(|e| e.to_string())
}

// ===== Bucket Commands =====

/// Get all buckets
#[tauri::command]
pub async fn get_buckets(state: State<'_, AppState>) -> Result<Vec<BucketResponse>, String> {
    state
        .artifact_bucket_repo
        .get_all()
        .await
        .map(|buckets| buckets.into_iter().map(BucketResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Create a new bucket
#[tauri::command]
pub async fn create_bucket(
    input: CreateBucketInput,
    state: State<'_, AppState>,
) -> Result<BucketResponse, String> {
    let mut bucket = ArtifactBucket::new(&input.name);

    // Add accepted types
    if let Some(types) = input.accepted_types {
        for type_str in types {
            let parsed_type: ArtifactType = type_str
                .parse()
                .map_err(|_| format!("Invalid artifact type: {}", type_str))?;
            bucket = bucket.accepts(parsed_type);
        }
    }

    // Add writers
    if let Some(writers) = input.writers {
        for writer in writers {
            bucket = bucket.with_writer(writer);
        }
    }

    // Add readers
    if let Some(readers) = input.readers {
        for reader in readers {
            bucket = bucket.with_reader(reader);
        }
    }

    state
        .artifact_bucket_repo
        .create(bucket)
        .await
        .map(BucketResponse::from)
        .map_err(|e| e.to_string())
}

/// Get system buckets
#[tauri::command]
pub async fn get_system_buckets() -> Result<Vec<BucketResponse>, String> {
    Ok(ArtifactBucket::system_buckets()
        .into_iter()
        .map(BucketResponse::from)
        .collect())
}

// ===== Relation Commands =====

/// Add a relation between two artifacts
#[tauri::command]
pub async fn add_artifact_relation(
    input: AddRelationInput,
    state: State<'_, AppState>,
) -> Result<ArtifactRelationResponse, String> {
    let from_id = ArtifactId::from_string(input.from_artifact_id);
    let to_id = ArtifactId::from_string(input.to_artifact_id);

    let relation_type: ArtifactRelationType = input
        .relation_type
        .parse()
        .map_err(|_| format!("Invalid relation type: {}", input.relation_type))?;

    let relation = ArtifactRelation::new(from_id, to_id, relation_type);

    state
        .artifact_repo
        .add_relation(relation)
        .await
        .map(ArtifactRelationResponse::from)
        .map_err(|e| e.to_string())
}

/// Get all relations for an artifact
#[tauri::command]
pub async fn get_artifact_relations(
    artifact_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ArtifactRelationResponse>, String> {
    let artifact_id = ArtifactId::from_string(artifact_id);
    state
        .artifact_repo
        .get_relations(&artifact_id)
        .await
        .map(|relations| {
            relations
                .into_iter()
                .map(ArtifactRelationResponse::from)
                .collect()
        })
        .map_err(|e| e.to_string())
}
