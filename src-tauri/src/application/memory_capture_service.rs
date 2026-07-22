use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tracing::{error, info, warn};

use crate::domain::entities::{MemoryActorType, MemoryBucket, MemoryEntry, MemoryEvent, ProjectId};
use crate::domain::repositories::{MemoryEntryRepository, MemoryEventRepository};
use crate::error::AppResult;

#[derive(Clone, Debug)]
pub struct MemoryCaptureInput {
    pub bucket: String,
    pub title: String,
    pub summary: String,
    pub details_markdown: String,
    pub scope_paths: Vec<String>,
    pub source_context_type: Option<String>,
    pub source_context_id: Option<String>,
    pub source_conversation_id: Option<String>,
    pub quality_score: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct MemoryCaptureUpsertCommand {
    pub project_id: ProjectId,
    pub memories: Vec<MemoryCaptureInput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryCaptureUpsertResult {
    pub inserted: usize,
    pub skipped: usize,
    pub failed: usize,
    pub total: usize,
}

impl MemoryCaptureUpsertResult {
    pub fn message(&self) -> String {
        format!(
            "Processed {} memories: {} inserted, {} skipped (duplicates), {} failed",
            self.total, self.inserted, self.skipped, self.failed
        )
    }
}

#[async_trait]
pub trait MemoryCaptureUpsertPort: Send + Sync {
    async fn upsert_memories(
        &self,
        command: MemoryCaptureUpsertCommand,
    ) -> AppResult<MemoryCaptureUpsertResult>;
}

pub struct MemoryCaptureService {
    memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    memory_event_repo: Arc<dyn MemoryEventRepository>,
}

impl MemoryCaptureService {
    pub fn new(
        memory_entry_repo: Arc<dyn MemoryEntryRepository>,
        memory_event_repo: Arc<dyn MemoryEventRepository>,
    ) -> Self {
        Self {
            memory_entry_repo,
            memory_event_repo,
        }
    }
}

#[async_trait]
impl MemoryCaptureUpsertPort for MemoryCaptureService {
    async fn upsert_memories(
        &self,
        command: MemoryCaptureUpsertCommand,
    ) -> AppResult<MemoryCaptureUpsertResult> {
        let project_id = command.project_id;
        let total = command.memories.len();
        let mut inserted = 0;
        let mut skipped = 0;
        let mut failed = 0;

        for input in &command.memories {
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
            let existing = self
                .memory_entry_repo
                .find_by_content_hash(&project_id, &bucket, &content_hash)
                .await?;

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

            match self.memory_entry_repo.create(entry).await {
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
        if let Err(event_error) = self
            .memory_event_repo
            .create(MemoryEvent::new(
                project_id.clone(),
                "memory_capture_decision",
                MemoryActorType::MemoryCapture,
                json!({
                    "inserted": inserted,
                    "skipped": skipped,
                    "failed": failed,
                    "total": total,
                }),
            ))
            .await
        {
            warn!(
                error = %event_error,
                project_id = project_id.as_str(),
                "Failed to record memory capture decision"
            );
        }

        Ok(MemoryCaptureUpsertResult {
            inserted,
            skipped,
            failed,
            total,
        })
    }
}
