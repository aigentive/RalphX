use super::*;
use std::sync::Arc;

// Mock implementation for testing trait object usage
struct MockTaskRepository;

#[async_trait]
impl TaskRepository for MockTaskRepository {
    async fn create(&self, task: Task) -> AppResult<Task> {
        Ok(task)
    }

    async fn get_by_id(&self, _id: &TaskId) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_by_project(&self, _project_id: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn update(&self, _task: &Task) -> AppResult<()> {
        Ok(())
    }

    async fn update_with_expected_status(
        &self,
        _task: &Task,
        _expected_status: InternalStatus,
    ) -> AppResult<bool> {
        Ok(true)
    }

    async fn update_metadata(&self, _id: &TaskId, _metadata: Option<String>) -> AppResult<()> {
        Ok(())
    }

    async fn delete(&self, _id: &TaskId) -> AppResult<()> {
        Ok(())
    }

    async fn get_by_status(
        &self,
        _project_id: &ProjectId,
        _status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn persist_status_change(
        &self,
        _id: &TaskId,
        _from: InternalStatus,
        _to: InternalStatus,
        _trigger: &str,
    ) -> AppResult<String> {
        Ok(uuid::Uuid::new_v4().to_string())
    }

    async fn get_status_history(&self, _id: &TaskId) -> AppResult<Vec<StatusTransition>> {
        Ok(vec![])
    }

    async fn get_status_entered_at(
        &self,
        _task_id: &TaskId,
        _status: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        Ok(None)
    }

    async fn get_status_last_entered_at(
        &self,
        _task_id: &TaskId,
        _status: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        Ok(None)
    }

    async fn get_next_executable(&self, _project_id: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_by_ideation_session(
        &self,
        _session_id: &IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_by_project_filtered(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        let project_id = ProjectId::new();
        let mut task = Task::new(project_id, "Archived task".to_string());
        task.id = task_id.clone();
        Ok(task)
    }

    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        let project_id = ProjectId::new();
        let mut task = Task::new(project_id, "Restored task".to_string());
        task.id = task_id.clone();
        Ok(task)
    }

    async fn get_archived_count(
        &self,
        _project_id: &ProjectId,
        _ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn list_paginated(
        &self,
        _project_id: &ProjectId,
        _statuses: Option<Vec<InternalStatus>>,
        _offset: u32,
        _limit: u32,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
        _categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn count_tasks(
        &self,
        _project_id: &ProjectId,
        _include_archived: bool,
        _ideation_session_id: Option<&str>,
        _execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }

    async fn search(
        &self,
        _project_id: &ProjectId,
        _query: &str,
        _include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        Ok(None)
    }

    async fn get_oldest_ready_tasks(&self, _limit: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }

    async fn update_latest_state_history_metadata(
        &self,
        _task_id: &TaskId,
        _metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn has_task_in_states(
        &self,
        _project_id: &ProjectId,
        _statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn get_status_history_batch(
        &self,
        _task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<StatusTransition>>> {
        Ok(HashMap::new())
    }
}

#[test]
fn test_task_repository_trait_can_be_object_safe() {
    // Verify that TaskRepository can be used as a trait object
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    assert!(Arc::strong_count(&repo) == 1);
}

#[tokio::test]
async fn test_mock_task_repository_create() {
    let repo = MockTaskRepository;
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Test task".to_string());

    let result = repo.create(task.clone()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().id, task.id);
}

#[tokio::test]
async fn test_mock_task_repository_get_by_id_returns_none() {
    let repo = MockTaskRepository;
    let task_id = TaskId::new();

    let result = repo.get_by_id(&task_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_mock_task_repository_get_by_project() {
    let repo = MockTaskRepository;
    let project_id = ProjectId::new();

    let result = repo.get_by_project(&project_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_task_repository_update() {
    let repo = MockTaskRepository;
    let project_id = ProjectId::new();
    let task = Task::new(project_id, "Test task".to_string());

    let result = repo.update(&task).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_task_repository_delete() {
    let repo = MockTaskRepository;
    let task_id = TaskId::new();

    let result = repo.delete(&task_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_task_repository_get_by_status() {
    let repo = MockTaskRepository;
    let project_id = ProjectId::new();

    let result = repo
        .get_by_status(&project_id, InternalStatus::Backlog)
        .await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_task_repository_persist_status_change() {
    let repo = MockTaskRepository;
    let task_id = TaskId::new();

    let result = repo
        .persist_status_change(
            &task_id,
            InternalStatus::Backlog,
            InternalStatus::Ready,
            "user",
        )
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_mock_task_repository_get_status_history() {
    let repo = MockTaskRepository;
    let task_id = TaskId::new();

    let result = repo.get_status_history(&task_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_mock_task_repository_get_next_executable() {
    let repo = MockTaskRepository;
    let project_id = ProjectId::new();

    let result = repo.get_next_executable(&project_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_task_repository_trait_object_in_arc() {
    let repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository);
    let project_id = ProjectId::new();
    let task = Task::new(project_id.clone(), "Test via trait object".to_string());

    // Use through trait object
    let result = repo.create(task).await;
    assert!(result.is_ok());

    let tasks = repo.get_by_project(&project_id).await;
    assert!(tasks.is_ok());
}
