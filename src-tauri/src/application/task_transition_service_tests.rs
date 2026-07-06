// Tests for TauriEventEmitter enrichment — build_enriched_payload helper.
// Acceptance criteria 4 and 5 for the foundation step:
//   - build_enriched_payload returns Some(...) with all presentation fields when lookups succeed
//   - build_enriched_payload returns None when task lookup fails

use std::sync::Arc;

use crate::application::task_transition_service::TauriEventEmitter;
use crate::domain::entities::{IdeationSessionBuilder, IdeationSessionId, Task, TaskId};
use crate::domain::repositories::{
    ExternalEventsRepository, IdeationSessionRepository, ProjectRepository, TaskRepository,
};
use crate::infrastructure::memory::{
    MemoryExternalEventsRepository, MemoryIdeationSessionRepository, MemoryProjectRepository,
    MemoryTaskRepository,
};

mod enrichment_tests {
    use super::*;
    use crate::domain::entities::Project;

    /// Build a TauriEventEmitter backed by the given in-memory repos.
    fn make_emitter(
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        session_repo: Arc<dyn IdeationSessionRepository>,
    ) -> TauriEventEmitter<tauri::Wry> {
        TauriEventEmitter::new(None).with_external_events(
            Arc::new(MemoryExternalEventsRepository::new()) as Arc<dyn ExternalEventsRepository>,
            task_repo,
            project_repo,
            session_repo,
        )
    }

    fn make_emitter_without_external_events(
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        session_repo: Arc<dyn IdeationSessionRepository>,
    ) -> TauriEventEmitter<tauri::Wry> {
        TauriEventEmitter::new(None)
            .with_enrichment_repos(task_repo, project_repo, session_repo)
    }

    #[tokio::test]
    async fn build_enriched_payload_with_project_and_session() {
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
        let session_repo: Arc<dyn IdeationSessionRepository> =
            Arc::new(MemoryIdeationSessionRepository::new());

        // Seed project (ID auto-generated)
        let project = Project::new("My Project".to_string(), "/tmp".to_string());
        let project_id = project.id.clone();
        project_repo.create(project).await.unwrap();

        // Seed session with title
        let session_id = IdeationSessionId::new();
        let session = IdeationSessionBuilder::new()
            .id(session_id.clone())
            .project_id(project_id.clone())
            .title("My Session".to_string())
            .build();
        session_repo.create(session).await.unwrap();

        // Seed task referencing project and session
        let mut task = Task::new(project_id.clone(), "My Task".to_string());
        task.ideation_session_id = Some(session_id.clone());
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        let emitter = make_emitter(task_repo, project_repo, session_repo);
        let result = emitter
            .build_enriched_payload(&task_id.to_string(), "ready", "executing")
            .await;

        let (proj_id, payload) = result.expect("expected Some payload");
        assert_eq!(proj_id, project_id.to_string());

        // Base fields
        assert_eq!(payload["task_id"], task_id.to_string());
        assert_eq!(payload["project_id"], project_id.to_string());
        assert_eq!(payload["old_status"], "ready");
        assert_eq!(payload["new_status"], "executing");
        assert!(payload["timestamp"].is_string());

        // Enriched fields
        assert_eq!(payload["project_name"], "My Project");
        assert_eq!(payload["session_title"], "My Session");
        assert_eq!(payload["task_title"], "My Task");
        assert_eq!(payload["presentation_kind"], "task_status_changed");
    }

    #[tokio::test]
    async fn build_enriched_payload_project_only_no_session() {
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
        let session_repo: Arc<dyn IdeationSessionRepository> =
            Arc::new(MemoryIdeationSessionRepository::new());

        let project = Project::new("Solo Project".to_string(), "/tmp".to_string());
        let project_id = project.id.clone();
        project_repo.create(project).await.unwrap();

        // Task has no ideation_session_id
        let task = Task::new(project_id.clone(), "Solo Task".to_string());
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        let emitter = make_emitter(task_repo, project_repo, session_repo);
        let result = emitter
            .build_enriched_payload(&task_id.to_string(), "backlog", "ready")
            .await;

        let (_proj_id, payload) = result.expect("expected Some payload");

        // project_name and task_title present
        assert_eq!(payload["project_name"], "Solo Project");
        assert_eq!(payload["task_title"], "Solo Task");

        // session_title key must be absent (inject_into skips None fields)
        assert!(
            payload.get("session_title").is_none(),
            "session_title should be absent when task has no session"
        );

        // Base fields intact (backward-compat coverage)
        assert_eq!(payload["task_id"], task_id.to_string());
        assert_eq!(payload["project_id"], project_id.to_string());
        assert_eq!(payload["old_status"], "backlog");
        assert_eq!(payload["new_status"], "ready");
    }

    #[tokio::test]
    async fn build_enriched_payload_works_without_external_events_repo() {
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
        let session_repo: Arc<dyn IdeationSessionRepository> =
            Arc::new(MemoryIdeationSessionRepository::new());

        let project = Project::new("UI Event Project".to_string(), "/tmp".to_string());
        let project_id = project.id.clone();
        project_repo.create(project).await.unwrap();

        let task = Task::new(project_id.clone(), "UI Event Task".to_string());
        let task_id = task.id.clone();
        task_repo.create(task).await.unwrap();

        let emitter = make_emitter_without_external_events(task_repo, project_repo, session_repo);
        let result = emitter
            .build_enriched_payload(&task_id.to_string(), "pending_merge", "merge_incomplete")
            .await;

        let (_proj_id, payload) = result.expect("expected Some payload");
        assert_eq!(payload["task_id"], task_id.to_string());
        assert_eq!(payload["project_name"], "UI Event Project");
        assert_eq!(payload["task_title"], "UI Event Task");
    }

    #[tokio::test]
    async fn build_enriched_payload_returns_none_when_task_not_found() {
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
        let session_repo: Arc<dyn IdeationSessionRepository> =
            Arc::new(MemoryIdeationSessionRepository::new());

        // No task seeded — lookup will return None
        let missing_task_id = TaskId::new();

        let emitter = make_emitter(task_repo, project_repo, session_repo);
        let result = emitter
            .build_enriched_payload(&missing_task_id.to_string(), "ready", "executing")
            .await;

        assert!(result.is_none(), "expected None when task is not found");
    }
}
