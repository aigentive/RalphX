// Memory MCP tool handlers
// These handlers are restricted to memory agents only (ralphx-memory-maintainer, ralphx-memory-capture)
// Access control is enforced via three-layer allowlist model

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, Json};
use serde_json::json;
use tracing::{error, info};

use super::*;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ArchiveJobPayload, ArchiveJobType, MemoryActorType, MemoryArchiveJob, MemoryBucket,
    MemoryEntry, MemoryEntryId, MemoryEvent, MemoryStatus,
};
use crate::domain::services::{IndexRewriter, RuleIngestionService};

// ============================================================================
// Handler: search_memories
// ============================================================================

pub async fn search_memories(
    State(state): State<HttpServerState>,
    Json(req): Json<SearchMemoriesRequest>,
) -> Result<Json<SearchMemoriesResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id);
    let mut entries = if let Some(bucket_str) = req.bucket.as_deref() {
        let bucket = bucket_str.parse::<MemoryBucket>().map_err(|_| {
            error!("Invalid memory bucket filter: {}", bucket_str);
            StatusCode::BAD_REQUEST
        })?;
        state
            .app_state
            .memory_entry_repo
            .get_by_project_and_bucket(&project_id, bucket)
            .await
            .map_err(|e| {
                error!("Failed to fetch memories by bucket: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    } else {
        state
            .app_state
            .memory_entry_repo
            .get_by_project_and_status(&project_id, MemoryStatus::Active)
            .await
            .map_err(|e| {
                error!("Failed to fetch active memories: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?
    };

    if let Some(query) = req
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        let q = query.to_lowercase();
        entries.retain(|entry| {
            entry.title.to_lowercase().contains(&q)
                || entry.summary.to_lowercase().contains(&q)
                || entry.details_markdown.to_lowercase().contains(&q)
        });
    }

    if let Some(limit) = req.limit {
        entries.truncate(limit);
    }

    let memories: Vec<MemoryEntryResponse> = entries.into_iter().map(Into::into).collect();
    let count = memories.len();

    Ok(Json(SearchMemoriesResponse { memories, count }))
}

// ============================================================================
// Handler: get_memory
// ============================================================================

pub async fn get_memory(
    State(state): State<HttpServerState>,
    Json(req): Json<GetMemoryRequest>,
) -> Result<Json<GetMemoryResponse>, StatusCode> {
    let memory_id = MemoryEntryId::from_string(req.memory_id);
    let memory = state
        .app_state
        .memory_entry_repo
        .get_by_id(&memory_id)
        .await
        .map_err(|e| {
            error!("Failed to fetch memory by id: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .map(Into::into);

    Ok(Json(GetMemoryResponse { memory }))
}

// ============================================================================
// Handler: get_memories_for_paths
// ============================================================================

pub async fn get_memories_for_paths(
    State(state): State<HttpServerState>,
    Json(req): Json<GetMemoriesForPathsRequest>,
) -> Result<Json<GetMemoriesForPathsResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id);
    let mut entries = state
        .app_state
        .memory_entry_repo
        .get_by_paths(&project_id, &req.paths)
        .await
        .map_err(|e| {
            error!("Failed to fetch memories for paths: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if let Some(limit) = req.limit {
        entries.truncate(limit);
    }

    let memories: Vec<MemoryEntryResponse> = entries.into_iter().map(Into::into).collect();
    let count = memories.len();

    Ok(Json(GetMemoriesForPathsResponse { memories, count }))
}

// ============================================================================
// Handler: upsert_memories
// ============================================================================

pub async fn upsert_memories(
    State(state): State<HttpServerState>,
    Json(req): Json<UpsertMemoriesRequest>,
) -> Result<Json<UpsertMemoriesResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id.clone());
    let mut inserted = 0;
    let mut skipped = 0;
    let mut failed = 0;

    for input in &req.memories {
        // Parse bucket
        let bucket = match input.bucket.parse::<MemoryBucket>() {
            Ok(b) => b,
            Err(_) => {
                error!("Invalid bucket: {}", input.bucket);
                failed += 1;
                continue;
            }
        };

        // Compute content hash for deduplication
        let content_hash = MemoryEntry::compute_content_hash(
            &input.title,
            &input.summary,
            &input.details_markdown,
        );

        // Check for duplicate
        let existing = state
            .app_state
            .memory_entry_repo
            .find_by_content_hash(&project_id, &bucket, &content_hash)
            .await
            .map_err(|e| {
                error!("Failed to check content hash: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        if existing.is_some() {
            skipped += 1;
            continue;
        }

        // Create new memory entry
        let mut entry = MemoryEntry::new(
            project_id.clone(),
            bucket,
            input.title.clone(),
            input.summary.clone(),
            input.details_markdown.clone(),
            input.scope_paths.clone(),
            content_hash,
        );
        entry.source_context_type = input.source_context_type.clone();
        entry.source_context_id = input.source_context_id.clone();
        entry.source_conversation_id = input.source_conversation_id.clone();
        entry.quality_score = input.quality_score;

        match state.app_state.memory_entry_repo.create(entry).await {
            Ok(_) => inserted += 1,
            Err(e) => {
                error!("Failed to create memory entry: {}", e);
                failed += 1;
            }
        }
    }

    info!(
        "upsert_memories: inserted={}, skipped={}, failed={}",
        inserted, skipped, failed
    );
    let _ = state
        .app_state
        .memory_event_repo
        .create(MemoryEvent::new(
            project_id,
            "memory_capture_decision",
            MemoryActorType::MemoryCapture,
            json!({
                "inserted": inserted,
                "skipped": skipped,
                "failed": failed,
                "total": req.memories.len(),
            }),
        ))
        .await;

    Ok(Json(UpsertMemoriesResponse {
        inserted,
        skipped,
        failed,
        message: format!(
            "Processed {} memories: {} inserted, {} skipped (duplicates), {} failed",
            req.memories.len(),
            inserted,
            skipped,
            failed
        ),
    }))
}

// ============================================================================
// Handler: mark_memory_obsolete
// ============================================================================

pub async fn mark_memory_obsolete(
    State(state): State<HttpServerState>,
    Json(req): Json<MarkMemoryObsoleteRequest>,
) -> Result<Json<MarkMemoryObsoleteResponse>, StatusCode> {
    let memory_id = MemoryEntryId::from_string(&req.memory_id);

    // Verify entry exists
    let entry = state
        .app_state
        .memory_entry_repo
        .get_by_id(&memory_id)
        .await
        .map_err(|e| {
            error!("Failed to get memory entry: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Update status to obsolete
    state
        .app_state
        .memory_entry_repo
        .update_status(&memory_id, MemoryStatus::Obsolete)
        .await
        .map_err(|e| {
            error!("Failed to update memory status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Record audit event
    let event = MemoryEvent::new(
        ProjectId::from_string(entry.project_id.0.clone()),
        "memory_obsoleted",
        MemoryActorType::System,
        json!({
            "memory_id": req.memory_id,
            "title": entry.title,
        }),
    );
    let _ = state.app_state.memory_event_repo.create(event).await;

    Ok(Json(MarkMemoryObsoleteResponse {
        success: true,
        message: format!("Memory {} marked as obsolete", req.memory_id),
    }))
}

// ============================================================================
// Handler: refresh_memory_rule_index
// ============================================================================

pub async fn refresh_memory_rule_index(
    State(state): State<HttpServerState>,
    Json(req): Json<RefreshMemoryRuleIndexRequest>,
) -> Result<Json<RefreshMemoryRuleIndexResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id);

    // Get all active memories for the project
    let all_memories = state
        .app_state
        .memory_entry_repo
        .get_by_project_and_status(&project_id, MemoryStatus::Active)
        .await
        .map_err(|e| {
            error!("Failed to fetch active memories: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Group memories by source_rule_file
    let mut memories_by_rule_file: std::collections::HashMap<String, Vec<MemoryEntry>> =
        std::collections::HashMap::new();

    for memory in all_memories {
        if let Some(rule_file) = memory.source_rule_file.clone() {
            memories_by_rule_file
                .entry(rule_file)
                .or_insert_with(Vec::new)
                .push(memory);
        }
    }

    // Filter by scope_key if provided
    let memories_by_rule_file: std::collections::HashMap<String, Vec<MemoryEntry>> =
        if let Some(scope_key) = req.scope_key.as_deref() {
            memories_by_rule_file
                .into_iter()
                .filter(|(rule_file, _)| rule_file.contains(scope_key))
                .collect()
        } else {
            memories_by_rule_file
        };

    let index_rewriter = IndexRewriter::new();
    let mut files_refreshed = 0;

    // For each rule file, regenerate its index and write to filesystem
    for (rule_file, memories) in memories_by_rule_file {
        // Get the paths from the first memory (they should all have the same source paths)
        let paths = memories
            .first()
            .map(|m| m.scope_paths.clone())
            .unwrap_or_default();

        // Rewrite the rule file
        match index_rewriter.rewrite_rule_file(&rule_file, paths, &memories) {
            Ok(_) => {
                files_refreshed += 1;
                info!("Refreshed rule index for: {}", rule_file);
            }
            Err(e) => {
                error!("Failed to refresh rule index for '{}': {}", rule_file, e);
                // Continue with next file instead of failing completely
            }
        }
    }

    info!(
        "refresh_memory_rule_index: refreshed {} rule index files",
        files_refreshed
    );

    Ok(Json(RefreshMemoryRuleIndexResponse {
        files_refreshed,
        message: format!("Refreshed {} rule index files", files_refreshed),
    }))
}

// ============================================================================
// Handler: ingest_rule_file
// ============================================================================

pub async fn ingest_rule_file(
    State(state): State<HttpServerState>,
    Json(req): Json<IngestRuleFileRequest>,
) -> Result<Json<IngestRuleFileResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id.clone());

    let service = RuleIngestionService::new(
        Arc::clone(&state.app_state.memory_entry_repo),
        Arc::clone(&state.app_state.memory_event_repo),
        Arc::clone(&state.app_state.memory_archive_repo),
    );

    let result = service
        .ingest_rule_file(project_id, &req.rule_file_path)
        .await
        .map_err(|e| {
            error!("Failed to ingest rule file '{}': {}", req.rule_file_path, e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!(
        "ingest_rule_file '{}': created={}, updated={}, rewritten={}",
        req.rule_file_path, result.memories_created, result.memories_updated, result.file_rewritten
    );

    Ok(Json(IngestRuleFileResponse {
        memories_created: result.memories_created,
        memories_updated: result.memories_updated,
        file_rewritten: result.file_rewritten,
        message: format!(
            "Ingested '{}': {} memories created, {} updated",
            req.rule_file_path, result.memories_created, result.memories_updated
        ),
    }))
}

// ============================================================================
// Handler: rebuild_archive_snapshots
// ============================================================================

pub async fn rebuild_archive_snapshots(
    State(state): State<HttpServerState>,
    Json(req): Json<RebuildArchiveSnapshotsRequest>,
) -> Result<Json<RebuildArchiveSnapshotsResponse>, StatusCode> {
    let project_id = ProjectId::from_string(req.project_id.clone());

    let payload = ArchiveJobPayload::full_rebuild(false);
    let job = MemoryArchiveJob::new(project_id, ArchiveJobType::FullRebuild, payload);
    let job_id = job.id.to_string();

    state
        .app_state
        .memory_archive_repo
        .create(job)
        .await
        .map_err(|e| {
            error!("Failed to create archive rebuild job: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    info!("rebuild_archive_snapshots: enqueued job {}", job_id);

    Ok(Json(RebuildArchiveSnapshotsResponse {
        job_id,
        message: "Full archive rebuild job enqueued".to_string(),
    }))
}

// ============================================================================
// Handler: get_conversation_transcript
// ============================================================================

pub async fn get_conversation_transcript(
    State(state): State<HttpServerState>,
    Json(req): Json<GetConversationTranscriptRequest>,
) -> Result<Json<GetConversationTranscriptResponse>, StatusCode> {
    use crate::domain::entities::ChatConversationId;

    let conversation_id = ChatConversationId::from_string(req.conversation_id.clone());
    let messages = state
        .app_state
        .chat_message_repo
        .get_by_conversation(&conversation_id)
        .await
        .map_err(|e| {
            error!("Failed to fetch conversation transcript: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let transcript_messages: Vec<TranscriptMessage> = messages
        .into_iter()
        .map(|msg| TranscriptMessage {
            role: msg.role.to_string(),
            content: msg.content,
            created_at: msg.created_at.to_rfc3339(),
        })
        .collect();

    let message_count = transcript_messages.len();

    Ok(Json(GetConversationTranscriptResponse {
        conversation_id: req.conversation_id,
        messages: transcript_messages,
        message_count,
    }))
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod memory_tests;
