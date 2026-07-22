use std::sync::Arc;

use async_trait::async_trait;

use crate::application::memory_capture_service::{
    MemoryCaptureInput, MemoryCaptureService, MemoryCaptureUpsertCommand, MemoryCaptureUpsertPort,
};
use crate::domain::entities::{
    MemoryBucket, MemoryEntry, MemoryEntryId, MemoryEvent, MemoryStatus, ProjectId,
};
use crate::domain::repositories::{MemoryEntryRepository, MemoryEventRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::memory::{InMemoryMemoryEntryRepository, InMemoryMemoryEventRepository};

fn input(title: &str) -> MemoryCaptureInput {
    MemoryCaptureInput {
        bucket: "implementation_discoveries".to_string(),
        title: title.to_string(),
        summary: "A reusable implementation discovery.".to_string(),
        details_markdown: "The production run exposed a durable implementation detail.".to_string(),
        scope_paths: vec!["src-tauri/src/application/**".to_string()],
        source_context_type: Some("task_execution".to_string()),
        source_context_id: Some("task-a1".to_string()),
        source_conversation_id: Some("conversation-a1".to_string()),
        quality_score: Some(0.93),
    }
}

fn command(
    project_id: &ProjectId,
    memories: Vec<MemoryCaptureInput>,
) -> MemoryCaptureUpsertCommand {
    MemoryCaptureUpsertCommand {
        project_id: project_id.clone(),
        memories,
    }
}

#[tokio::test]
async fn service_preserves_source_fields_and_deduplicates_active_entries() {
    let project_id = ProjectId::from_string("project-service-dedupe".to_string());
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let service = MemoryCaptureService::new(entry_repo.clone(), event_repo.clone());

    let first = service
        .upsert_memories(command(&project_id, vec![input("Preserve capture fields")]))
        .await
        .unwrap();
    let duplicate = service
        .upsert_memories(command(&project_id, vec![input("Preserve capture fields")]))
        .await
        .unwrap();

    assert_eq!((first.inserted, first.skipped, first.failed), (1, 0, 0));
    assert_eq!(
        (duplicate.inserted, duplicate.skipped, duplicate.failed),
        (0, 1, 0)
    );
    let entries = entry_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].source_context_type.as_deref(),
        Some("task_execution")
    );
    assert_eq!(entries[0].source_context_id.as_deref(), Some("task-a1"));
    assert_eq!(
        entries[0].source_conversation_id.as_deref(),
        Some("conversation-a1")
    );
    assert_eq!(entries[0].quality_score, Some(0.93));

    let events = event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert!(events.iter().any(|event| event.details["inserted"] == 1));
    assert!(events.iter().any(|event| event.details["skipped"] == 1));
}

struct ControlledEntryRepository {
    inner: InMemoryMemoryEntryRepository,
    fail_lookup: bool,
    fail_create: bool,
}

impl ControlledEntryRepository {
    fn new(fail_lookup: bool, fail_create: bool) -> Self {
        Self {
            inner: InMemoryMemoryEntryRepository::new(),
            fail_lookup,
            fail_create,
        }
    }
}

#[async_trait]
impl MemoryEntryRepository for ControlledEntryRepository {
    async fn create(&self, entry: MemoryEntry) -> AppResult<MemoryEntry> {
        if self.fail_create {
            return Err(AppError::Database("forced create failure".to_string()));
        }
        self.inner.create(entry).await
    }

    async fn get_by_id(&self, id: &MemoryEntryId) -> AppResult<Option<MemoryEntry>> {
        self.inner.get_by_id(id).await
    }

    async fn find_by_content_hash(
        &self,
        project_id: &ProjectId,
        bucket: &MemoryBucket,
        content_hash: &str,
    ) -> AppResult<Option<MemoryEntry>> {
        if self.fail_lookup {
            return Err(AppError::Database("forced lookup failure".to_string()));
        }
        self.inner
            .find_by_content_hash(project_id, bucket, content_hash)
            .await
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<MemoryEntry>> {
        self.inner.get_by_project(project_id).await
    }

    async fn get_by_project_and_status(
        &self,
        project_id: &ProjectId,
        status: MemoryStatus,
    ) -> AppResult<Vec<MemoryEntry>> {
        self.inner
            .get_by_project_and_status(project_id, status)
            .await
    }

    async fn get_by_project_and_bucket(
        &self,
        project_id: &ProjectId,
        bucket: MemoryBucket,
    ) -> AppResult<Vec<MemoryEntry>> {
        self.inner
            .get_by_project_and_bucket(project_id, bucket)
            .await
    }

    async fn get_by_rule_file(
        &self,
        project_id: &ProjectId,
        rule_file: &str,
    ) -> AppResult<Vec<MemoryEntry>> {
        self.inner.get_by_rule_file(project_id, rule_file).await
    }

    async fn get_by_content_hash(&self, content_hash: &str) -> AppResult<Vec<MemoryEntry>> {
        self.inner.get_by_content_hash(content_hash).await
    }

    async fn update_status(&self, id: &MemoryEntryId, status: MemoryStatus) -> AppResult<()> {
        self.inner.update_status(id, status).await
    }

    async fn update(&self, entry: &MemoryEntry) -> AppResult<()> {
        self.inner.update(entry).await
    }

    async fn delete(&self, id: &MemoryEntryId) -> AppResult<()> {
        self.inner.delete(id).await
    }

    async fn get_by_paths(
        &self,
        project_id: &ProjectId,
        paths: &[String],
    ) -> AppResult<Vec<MemoryEntry>> {
        self.inner.get_by_paths(project_id, paths).await
    }
}

#[tokio::test]
async fn service_counts_invalid_buckets_and_create_failures_per_item() {
    let project_id = ProjectId::from_string("project-service-failures".to_string());
    let entry_repo = Arc::new(ControlledEntryRepository::new(false, true));
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let service = MemoryCaptureService::new(entry_repo, event_repo.clone());
    let mut invalid = input("Invalid bucket");
    invalid.bucket = "not-a-memory-bucket".to_string();

    let result = service
        .upsert_memories(command(&project_id, vec![invalid, input("Create failure")]))
        .await
        .unwrap();

    assert_eq!((result.inserted, result.skipped, result.failed), (0, 0, 2));
    let events = event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].details["failed"], 2);
    assert_eq!(events[0].details["total"], 2);
}

#[tokio::test]
async fn service_propagates_content_hash_lookup_failure_without_audit_event() {
    let project_id = ProjectId::from_string("project-service-lookup".to_string());
    let entry_repo = Arc::new(ControlledEntryRepository::new(true, false));
    let event_repo = Arc::new(InMemoryMemoryEventRepository::new());
    let service = MemoryCaptureService::new(entry_repo, event_repo.clone());

    let error = service
        .upsert_memories(command(&project_id, vec![input("Lookup failure")]))
        .await
        .unwrap_err();

    assert!(matches!(error, AppError::Database(message) if message == "forced lookup failure"));
    assert!(event_repo
        .get_by_type("memory_capture_decision")
        .await
        .unwrap()
        .is_empty());
}

struct FailingEventRepository;

#[async_trait]
impl MemoryEventRepository for FailingEventRepository {
    async fn create(&self, _event: MemoryEvent) -> AppResult<MemoryEvent> {
        Err(AppError::Database("forced audit failure".to_string()))
    }

    async fn get_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<MemoryEvent>> {
        Ok(Vec::new())
    }

    async fn get_by_type(&self, _event_type: &str) -> AppResult<Vec<MemoryEvent>> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn service_keeps_insert_when_capture_decision_audit_fails() {
    let project_id = ProjectId::from_string("project-service-audit".to_string());
    let entry_repo = Arc::new(InMemoryMemoryEntryRepository::new());
    let service = MemoryCaptureService::new(entry_repo.clone(), Arc::new(FailingEventRepository));

    let result = service
        .upsert_memories(command(&project_id, vec![input("Audit failure")]))
        .await
        .unwrap();

    assert_eq!((result.inserted, result.skipped, result.failed), (1, 0, 0));
    assert_eq!(
        entry_repo.get_by_project(&project_id).await.unwrap().len(),
        1
    );
}
