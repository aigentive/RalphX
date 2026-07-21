use crate::domain::entities::{
    BranchUpdateCapacityOwnership, BranchUpdateContinuation, BranchUpdateDirection,
    BranchUpdateOperation, BranchUpdateWorkspaceOwnership, ExecutionPlanId, GitTargetIdentity,
    IdeationSessionId, InternalStatus, ProjectId, Task, TaskCategory, TaskId, TaskStep, TaskStepId,
    TaskStepStatus,
};
use crate::domain::ideation::TasksFeatureAction;
use crate::domain::repositories::{
    BranchUpdateActivation, BranchUpdateRepository, StateHistoryMetadata, TaskRepository,
    TaskStepRepository,
};
use crate::infrastructure::sqlite::{
    SqliteBranchUpdateRepository, SqliteTaskRepository, SqliteTaskStepRepository,
};
use crate::testing::SqliteTestDb;
use chrono::Utc;
use std::path::PathBuf;

fn setup_test_db() -> SqliteTestDb {
    let db = SqliteTestDb::new("sqlite-task-repo");
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('test-project', 'Test Project', '/test/path')",
            [],
        )
        .unwrap();
    });
    db
}

// Note: Tests use Task::new() which initializes source_proposal_id and plan_artifact_id to None
// No test changes needed - field handling is already tested via entity tests

fn create_test_task(title: &str) -> Task {
    Task::new_with_category(
        ProjectId::from_string("test-project".to_string()),
        title.to_string(),
        TaskCategory::Regular,
    )
}

// ==================== CRUD TESTS ====================

#[tokio::test]
async fn test_create_inserts_task_and_returns_it() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    let result = repo.create(task.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, task.id);
    assert_eq!(created.title, "Test Task");
}

#[tokio::test]
async fn test_create_with_tasks_policy_rejects_disabled_without_inserting() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();
    let project_id = ProjectId::from_string("test-project".to_string());

    let error = repo
        .create(create_test_task("Disabled Task"))
        .await
        .expect_err("disabled Tasks must reject standalone creation");
    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    assert_eq!(
        repo.count_tasks(&project_id, true, None, None)
            .await
            .unwrap(),
        0
    );

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 1, tasks_feature_state = 'enabled'
             WHERE id = 1",
            [],
        )
        .unwrap();
    });
    repo.create(create_test_task("Enabled Task"))
        .await
        .expect("re-enabled Tasks must allow creation");
    assert_eq!(
        repo.count_tasks(&project_id, true, None, None)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn test_status_change_with_tasks_policy_rejects_progress_but_allows_pause() {
    let db = setup_test_db();
    let seed_repo = SqliteTaskRepository::new(db.new_connection());
    let task = seed_repo
        .create(create_test_task("Task to pause"))
        .await
        .unwrap();
    let guarded_repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();

    let error = guarded_repo
        .persist_status_change(
            &task.id,
            InternalStatus::Backlog,
            InternalStatus::Ready,
            "stale-worker",
        )
        .await
        .expect_err("disabled Tasks must reject progress persistence");
    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    assert_eq!(
        guarded_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Backlog
    );

    guarded_repo
        .persist_status_change_for_action(
            &task.id,
            InternalStatus::Backlog,
            InternalStatus::Paused,
            "tasks-feature-disabled",
            TasksFeatureAction::Quiesce,
        )
        .await
        .expect("safe pause must remain available while Tasks are off");
    assert_eq!(
        guarded_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn paused_destination_does_not_bypass_history_mutation_policy() {
    let db = setup_test_db();
    let seed_repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = seed_repo
        .create(create_test_task("Paused metadata guard"))
        .await
        .unwrap();
    task.internal_status = InternalStatus::Paused;
    task.metadata = Some(r#"{"unexpected":true}"#.to_string());
    let guarded_repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();

    let error = guarded_repo
        .update(&task)
        .await
        .expect_err("a paused destination must not authorize arbitrary task mutation");

    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    let unchanged = guarded_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(unchanged.internal_status, InternalStatus::Backlog);
    assert!(unchanged.metadata.is_none());
}

#[tokio::test]
async fn test_full_update_with_tasks_policy_rejects_stale_progress_after_pause() {
    let db = setup_test_db();
    let seed_repo = SqliteTaskRepository::new(db.new_connection());
    let task = seed_repo
        .create(create_test_task("Task with stale worker"))
        .await
        .unwrap();
    let mut stale_worker_task = task.clone();
    stale_worker_task.internal_status = InternalStatus::Ready;

    seed_repo
        .persist_status_change(
            &task.id,
            InternalStatus::Backlog,
            InternalStatus::Paused,
            "tasks-feature-disabled",
        )
        .await
        .unwrap();

    let guarded_repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();
    let error = guarded_repo
        .update(&stale_worker_task)
        .await
        .expect_err("stale full-task writes must not revive a paused Task while Tasks are off");
    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    assert_eq!(
        guarded_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Paused
    );
}

#[tokio::test]
async fn test_expected_status_update_with_tasks_policy_rejects_progress() {
    let db = setup_test_db();
    let seed_repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = seed_repo
        .create(create_test_task("Task with guarded compare-and-set"))
        .await
        .unwrap();
    task.internal_status = InternalStatus::Ready;
    let guarded_repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();

    let error = guarded_repo
        .update_with_expected_status(&task, InternalStatus::Backlog)
        .await
        .expect_err("disabled Tasks must reject guarded progress writes");
    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    assert_eq!(
        guarded_repo
            .get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Backlog
    );
}

#[tokio::test]
async fn guarded_task_lifecycle_writes_preserve_status_history_and_authority() {
    let db = setup_test_db();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 1, tasks_feature_state = 'enabled'
             WHERE id = 1",
            [],
        )
        .unwrap();
    });
    let repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();
    let task = repo
        .create(create_test_task("Guarded lifecycle"))
        .await
        .expect("enabled Tasks should allow task creation");

    let mut ready = task.clone();
    ready.internal_status = InternalStatus::Ready;
    ready.touch();
    let history_id = repo
        .update_with_expected_status_and_history_for_action(
            &ready,
            InternalStatus::Backlog,
            "guarded-ready",
            TasksFeatureAction::Progress,
        )
        .await
        .expect("guarded transition should commit atomically")
        .expect("current Backlog authority should apply");

    let mut stale = ready.clone();
    stale.internal_status = InternalStatus::Executing;
    stale.touch();
    assert!(repo
        .update_with_expected_status_and_history_for_action(
            &stale,
            InternalStatus::Backlog,
            "stale-worker",
            TasksFeatureAction::Progress,
        )
        .await
        .expect("stale authority should be a non-error")
        .is_none());
    assert_eq!(
        repo.get_by_id(&task.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Ready
    );
    assert_eq!(repo.get_status_history(&task.id).await.unwrap().len(), 1);

    repo.update_metadata(&task.id, Some(r#"{"guarded":true}"#.to_string()))
        .await
        .expect("enabled Tasks should allow metadata writes");
    assert!(repo.archive(&task.id).await.unwrap().archived_at.is_some());
    assert!(repo.restore(&task.id).await.unwrap().archived_at.is_none());

    repo.update_latest_state_history_metadata(
        &task.id,
        &StateHistoryMetadata {
            conversation_id: "conversation-guarded".to_string(),
            agent_run_id: "run-guarded".to_string(),
        },
    )
    .await
    .expect("enabled Tasks should allow audit metadata writes");
    let history = repo.get_status_history(&task.id).await.unwrap();
    assert_eq!(
        history[0].conversation_id.as_deref(),
        Some("conversation-guarded")
    );
    assert_eq!(history[0].agent_run_id.as_deref(), Some("run-guarded"));
    assert!(!history_id.is_empty());

    repo.delete(&task.id)
        .await
        .expect("enabled Tasks should allow task deletion");
    assert!(repo.get_by_id(&task.id).await.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_retrieves_task_correctly() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    repo.create(task.clone()).await.unwrap();
    let result = repo.get_by_id(&task.id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let found_task = found.unwrap();
    assert_eq!(found_task.id, task.id);
    assert_eq!(found_task.title, "Test Task");
    // Default category is Regular (legacy "feature" value maps to Regular via FromStr fallback)
    assert_eq!(found_task.category, TaskCategory::Regular);
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let id = TaskId::new();

    let result = repo.get_by_id(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_project_returns_sorted_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    // Create tasks with different priorities
    let mut task1 = create_test_task("Low Priority");
    task1.priority = 1;

    let mut task2 = create_test_task("High Priority");
    task2.priority = 10;

    let mut task3 = create_test_task("Medium Priority");
    task3.priority = 5;

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.create(task3.clone()).await.unwrap();

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    let tasks = result.unwrap();
    assert_eq!(tasks.len(), 3);
    // Should be sorted by priority DESC
    assert_eq!(tasks[0].title, "High Priority");
    assert_eq!(tasks[1].title, "Medium Priority");
    assert_eq!(tasks[2].title, "Low Priority");
}

#[tokio::test]
async fn test_update_modifies_task_fields() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("Original Title");

    repo.create(task.clone()).await.unwrap();

    task.title = "Updated Title".to_string();
    task.priority = 99;
    task.description = Some("New description".to_string());

    let update_result = repo.update(&task).await;
    assert!(update_result.is_ok());

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(found.title, "Updated Title");
    assert_eq!(found.priority, 99);
    assert_eq!(found.description, Some("New description".to_string()));
}

#[tokio::test]
async fn test_delete_removes_task_from_database() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("To Delete");

    repo.create(task.clone()).await.unwrap();

    let delete_result = repo.delete(&task.id).await;
    assert!(delete_result.is_ok());

    let found = repo.get_by_id(&task.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_create_and_retrieve_preserves_all_fields() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task = create_test_task("Full Task");
    task.description = Some("A description".to_string());
    task.priority = 42;
    task.internal_status = InternalStatus::Ready;
    task.task_branch = Some("ralphx/test/task-1".to_string());
    task.task_branch_base_ref = Some("ralphx/test/agent-plan".to_string());
    task.task_branch_base_sha = Some("abc123base".to_string());

    repo.create(task.clone()).await.unwrap();
    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();

    assert_eq!(found.id, task.id);
    assert_eq!(found.project_id, task.project_id);
    assert_eq!(found.category, task.category);
    assert_eq!(found.task_branch, task.task_branch);
    assert_eq!(found.task_branch_base_ref, task.task_branch_base_ref);
    assert_eq!(found.task_branch_base_sha, task.task_branch_base_sha);
    assert_eq!(found.title, task.title);
    assert_eq!(found.description, task.description);
    assert_eq!(found.priority, task.priority);
    assert_eq!(found.internal_status, task.internal_status);
}

#[tokio::test]
async fn test_get_by_project_returns_empty_for_no_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_project_only_returns_matching_project() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    // Add another project
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('other-project', 'Other', '/other')",
            [],
        )
        .unwrap();
    });

    let task1 = create_test_task("Task 1");
    let task2 = Task::new_with_category(
        ProjectId::from_string("other-project".to_string()),
        "Task 2".to_string(),
        TaskCategory::Regular,
    );

    repo.create(task1).await.unwrap();
    repo.create(task2).await.unwrap();

    let project_id = ProjectId::from_string("test-project".to_string());
    let result = repo.get_by_project(&project_id).await;

    assert!(result.is_ok());
    let tasks = result.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title, "Task 1");
}

#[tokio::test]
async fn test_get_by_ids_returns_only_requested_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task1 = create_test_task("Task 1");
    let task2 = create_test_task("Task 2");
    let task3 = create_test_task("Task 3");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.create(task3).await.unwrap();

    let tasks = repo
        .get_by_ids(&[task2.id.clone(), task1.id.clone()])
        .await
        .unwrap();
    let ids = tasks
        .into_iter()
        .map(|task| task.id)
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&task1.id));
    assert!(ids.contains(&task2.id));
}

#[tokio::test]
async fn test_get_by_status_with_metadata_bool_filters_in_sql() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut pending = create_test_task("Pending cleanup");
    pending.internal_status = InternalStatus::Merged;
    pending.metadata = Some(r#"{"pending_cleanup":true}"#.to_string());
    let mut not_pending = create_test_task("Not pending cleanup");
    not_pending.internal_status = InternalStatus::Merged;
    not_pending.metadata = Some(r#"{"pending_cleanup":false}"#.to_string());
    let mut malformed = create_test_task("Malformed metadata");
    malformed.internal_status = InternalStatus::Merged;
    malformed.metadata = Some("{pending_cleanup:true".to_string());

    repo.create(pending.clone()).await.unwrap();
    repo.create(not_pending).await.unwrap();
    repo.create(malformed).await.unwrap();

    let tasks = repo
        .get_by_status_with_metadata_bool(&project_id, InternalStatus::Merged, "pending_cleanup")
        .await
        .unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, pending.id);
}

#[tokio::test]
async fn test_find_merged_regular_plan_keys_filters_candidates() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());
    let session_id = IdeationSessionId::from_string("session-1".to_string());
    let execution_plan_id = ExecutionPlanId::from_string("plan-1".to_string());
    let missing_plan_id = ExecutionPlanId::from_string("plan-missing".to_string());

    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO ideation_sessions (id, project_id, status, created_at, updated_at)
             VALUES (?1, ?2, 'accepted', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![session_id.as_str(), project_id.as_str()],
        )
        .unwrap();
        for plan_id in [&execution_plan_id, &missing_plan_id] {
            conn.execute(
                "INSERT INTO execution_plans (id, session_id, status, created_at)
                 VALUES (?1, ?2, 'active', strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
                rusqlite::params![plan_id.as_str(), session_id.as_str()],
            )
            .unwrap();
        }
    });

    let mut merged_regular = create_test_task("Merged regular");
    merged_regular.internal_status = InternalStatus::Merged;
    merged_regular.ideation_session_id = Some(session_id.clone());
    merged_regular.execution_plan_id = Some(execution_plan_id.clone());

    let mut active_regular = create_test_task("Active regular");
    active_regular.internal_status = InternalStatus::Executing;
    active_regular.ideation_session_id = Some(session_id.clone());
    active_regular.execution_plan_id = Some(missing_plan_id.clone());

    let mut merged_plan_task = create_test_task("Merged plan task");
    merged_plan_task.internal_status = InternalStatus::Merged;
    merged_plan_task.category = TaskCategory::PlanMerge;
    merged_plan_task.ideation_session_id = Some(session_id.clone());
    merged_plan_task.execution_plan_id = Some(missing_plan_id.clone());

    repo.create(merged_regular).await.unwrap();
    repo.create(active_regular).await.unwrap();
    repo.create(merged_plan_task).await.unwrap();

    let keys = repo
        .find_merged_regular_plan_keys(
            &project_id,
            &[
                (session_id.clone(), execution_plan_id.clone()),
                (session_id.clone(), missing_plan_id.clone()),
            ],
        )
        .await
        .unwrap();

    assert_eq!(keys.len(), 1);
    assert!(keys.contains(&(session_id, execution_plan_id)));
}

// ==================== STATUS OPERATION TESTS ====================

#[tokio::test]
async fn test_persist_status_change_updates_task_status() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    repo.create(task.clone()).await.unwrap();

    let result = repo
        .persist_status_change(
            &task.id,
            InternalStatus::Backlog,
            InternalStatus::Ready,
            "user",
        )
        .await;

    assert!(result.is_ok());

    // Verify task status was updated
    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(found.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_persist_status_change_creates_history_record() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    repo.create(task.clone()).await.unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "system",
    )
    .await
    .unwrap();

    let history = repo.get_status_history(&task.id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from, InternalStatus::Backlog);
    assert_eq!(history[0].to, InternalStatus::Ready);
    assert_eq!(history[0].trigger, "system");
}

#[tokio::test]
async fn test_status_change_and_history_are_atomic() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    repo.create(task.clone()).await.unwrap();

    // Make multiple status changes
    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "user",
    )
    .await
    .unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Ready,
        InternalStatus::Executing,
        "agent",
    )
    .await
    .unwrap();

    // Verify both status and history are consistent
    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(found.internal_status, InternalStatus::Executing);

    let history = repo.get_status_history(&task.id).await.unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].from, InternalStatus::Ready);
    assert_eq!(history[1].to, InternalStatus::Executing);
}

#[tokio::test]
async fn terminal_restart_commits_cleanup_steps_status_and_history_atomically() {
    let db = setup_test_db();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 1, tasks_feature_state = 'enabled'
             WHERE id = 1",
            [],
        )
        .unwrap();
    });
    let repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();
    let step_repo = SqliteTaskStepRepository::new(db.new_connection());
    let mut task = create_test_task("Atomic terminal restart");
    task.internal_status = InternalStatus::Failed;
    task.task_branch = Some("ralphx/stale".to_string());
    let task = repo.create(task).await.unwrap();
    let mut failed_step = TaskStep::new(
        task.id.clone(),
        "Retry failed step".to_string(),
        0,
        "test".to_string(),
    );
    failed_step.status = TaskStepStatus::Failed;
    let failed_step = step_repo.create(failed_step).await.unwrap();

    let mut restarted = task.clone();
    restarted.internal_status = InternalStatus::Ready;
    restarted.task_branch = None;
    restarted.metadata = Some(r#"{"trigger_origin":"retry"}"#.to_string());
    restarted.touch();
    let result = repo
        .restart_terminal_task_to_ready_with_history_for_action(
            &restarted,
            InternalStatus::Failed,
            std::slice::from_ref(&failed_step.id),
            "user_restart",
            TasksFeatureAction::Progress,
        )
        .await
        .unwrap()
        .expect("restart authority should apply");

    assert_eq!(result.1, 1);
    let persisted = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(persisted.internal_status, InternalStatus::Ready);
    assert!(persisted.task_branch.is_none());
    assert_eq!(
        step_repo
            .get_by_id(&failed_step.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        TaskStepStatus::Pending
    );
    let history = repo.get_status_history(&task.id).await.unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].from, InternalStatus::Failed);
    assert_eq!(history[0].to, InternalStatus::Ready);
}

#[tokio::test]
async fn guarded_terminal_restart_rolls_back_stale_steps_and_rejects_tasks_off() {
    let db = setup_test_db();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 1, tasks_feature_state = 'enabled'
             WHERE id = 1",
            [],
        )
        .unwrap();
    });
    let repo = SqliteTaskRepository::new(db.new_connection()).with_tasks_feature_policy();
    let mut failed = create_test_task("Guarded restart rollback");
    failed.internal_status = InternalStatus::Failed;
    failed.task_branch = Some("ralphx/preserved".to_string());
    let failed = repo.create(failed).await.unwrap();
    let mut restarted = failed.clone();
    restarted.internal_status = InternalStatus::Ready;
    restarted.task_branch = None;
    restarted.touch();

    assert!(repo
        .restart_terminal_task_to_ready_with_history_for_action(
            &restarted,
            InternalStatus::Cancelled,
            &[],
            "stale-restart",
            TasksFeatureAction::Progress,
        )
        .await
        .expect("wrong-from restart should be a non-error")
        .is_none());

    let error = repo
        .restart_terminal_task_to_ready_with_history_for_action(
            &restarted,
            InternalStatus::Failed,
            &[TaskStepId::from_string("missing-failed-step")],
            "changed-step-restart",
            TasksFeatureAction::Progress,
        )
        .await
        .expect_err("a changed failed step must roll back the entire restart");
    assert!(error
        .to_string()
        .contains("changed during terminal restart"));
    let preserved = repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(preserved.internal_status, InternalStatus::Failed);
    assert_eq!(preserved.task_branch.as_deref(), Some("ralphx/preserved"));
    assert!(repo
        .get_status_history(&failed.id)
        .await
        .unwrap()
        .is_empty());

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 0, tasks_feature_state = 'disabled'
             WHERE id = 1",
            [],
        )
        .unwrap();
    });
    let error = repo
        .restart_terminal_task_to_ready_with_history_for_action(
            &restarted,
            InternalStatus::Failed,
            &[],
            "tasks-off-restart",
            TasksFeatureAction::Progress,
        )
        .await
        .expect_err("Tasks off must reject terminal restart before mutation");
    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    let preserved = repo.get_by_id(&failed.id).await.unwrap().unwrap();
    assert_eq!(preserved.internal_status, InternalStatus::Failed);
    assert_eq!(preserved.task_branch.as_deref(), Some("ralphx/preserved"));
    assert!(repo
        .get_status_history(&failed.id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_get_status_history_returns_transitions_in_order() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    repo.create(task.clone()).await.unwrap();

    // Create a sequence of transitions
    repo.persist_status_change(
        &task.id,
        InternalStatus::Backlog,
        InternalStatus::Ready,
        "step1",
    )
    .await
    .unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Ready,
        InternalStatus::Executing,
        "step2",
    )
    .await
    .unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::QaRefining,
        "step3",
    )
    .await
    .unwrap();

    let history = repo.get_status_history(&task.id).await.unwrap();

    assert_eq!(history.len(), 3);
    // Should be in chronological order (oldest first)
    assert_eq!(history[0].trigger, "step1");
    assert_eq!(history[1].trigger, "step2");
    assert_eq!(history[2].trigger, "step3");
}

#[tokio::test]
async fn test_get_status_history_returns_empty_for_no_transitions() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    repo.create(task.clone()).await.unwrap();

    let history = repo.get_status_history(&task.id).await.unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn test_get_status_last_entered_at_returns_most_recent_entry() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Re-enter status");

    repo.create(task.clone()).await.unwrap();

    repo.persist_status_change(
        &task.id,
        InternalStatus::Ready,
        InternalStatus::Executing,
        "first",
    )
    .await
    .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    repo.persist_status_change(
        &task.id,
        InternalStatus::Executing,
        InternalStatus::Failed,
        "leave",
    )
    .await
    .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(2)).await;
    repo.persist_status_change(
        &task.id,
        InternalStatus::Ready,
        InternalStatus::Executing,
        "second",
    )
    .await
    .unwrap();

    let latest = repo
        .get_status_last_entered_at(&task.id, InternalStatus::Executing)
        .await
        .unwrap()
        .expect("latest entry should exist");
    let earliest = repo
        .get_status_entered_at(&task.id, InternalStatus::Executing)
        .await
        .unwrap()
        .expect("earliest entry should exist");

    assert!(
        latest > earliest,
        "latest execution entry should be newer than earliest entry"
    );
}

#[tokio::test]
async fn test_get_by_status_filters_correctly() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task1 = create_test_task("Backlog Task");
    task1.internal_status = InternalStatus::Backlog;

    let mut task2 = create_test_task("Ready Task 1");
    task2.internal_status = InternalStatus::Ready;

    let mut task3 = create_test_task("Ready Task 2");
    task3.internal_status = InternalStatus::Ready;

    let mut task4 = create_test_task("Executing Task");
    task4.internal_status = InternalStatus::Executing;

    repo.create(task1).await.unwrap();
    repo.create(task2).await.unwrap();
    repo.create(task3).await.unwrap();
    repo.create(task4).await.unwrap();

    let ready_tasks = repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();

    assert_eq!(ready_tasks.len(), 2);
    assert!(ready_tasks
        .iter()
        .all(|t| t.internal_status == InternalStatus::Ready));
}

#[tokio::test]
async fn test_get_by_status_returns_empty_for_no_matches() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = create_test_task("Backlog Task");
    repo.create(task).await.unwrap();

    let ready_tasks = repo
        .get_by_status(&project_id, InternalStatus::Ready)
        .await
        .unwrap();

    assert!(ready_tasks.is_empty());
}

// Note: blocker operation tests removed — blockers are now managed via TaskDependencyRepository.
// See sqlite_task_dependency_repo_tests.rs for dependency tests.

#[tokio::test]
async fn test_get_next_executable_returns_highest_priority_ready() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut low = create_test_task("Low");
    low.internal_status = InternalStatus::Ready;
    low.priority = 1;

    let mut high = create_test_task("High");
    high.internal_status = InternalStatus::Ready;
    high.priority = 10;

    repo.create(low).await.unwrap();
    repo.create(high.clone()).await.unwrap();

    let next = repo.get_next_executable(&project_id).await.unwrap();
    assert!(next.is_some());
    assert_eq!(next.unwrap().id, high.id);
}

#[tokio::test]
async fn test_get_next_executable_returns_none_when_no_ready_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = create_test_task("Backlog Task"); // Default status is Backlog
    repo.create(task).await.unwrap();

    let next = repo.get_next_executable(&project_id).await.unwrap();
    assert!(next.is_none());
}

// ==================== ARCHIVE OPERATION TESTS ====================

#[tokio::test]
async fn test_archive_sets_archived_at() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Task to Archive");

    repo.create(task.clone()).await.unwrap();

    let archived = repo.archive(&task.id).await.unwrap();
    assert!(archived.archived_at.is_some());

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(found.archived_at.is_some());
}

#[tokio::test]
async fn test_restore_clears_archived_at() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Task to Archive and Restore");

    repo.create(task.clone()).await.unwrap();
    repo.archive(&task.id).await.unwrap();

    let restored = repo.restore(&task.id).await.unwrap();
    assert!(restored.archived_at.is_none());

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(found.archived_at.is_none());
}

#[tokio::test]
async fn test_get_archived_count_returns_correct_count() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task1 = create_test_task("Task 1");
    let task2 = create_test_task("Task 2");
    let task3 = create_test_task("Task 3");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.create(task3.clone()).await.unwrap();

    // Archive two tasks
    repo.archive(&task1.id).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    let count = repo.get_archived_count(&project_id, None).await.unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_get_by_project_filtered_excludes_archived_by_default() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task1 = create_test_task("Active Task");
    let task2 = create_test_task("Archived Task");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    let active_tasks = repo
        .get_by_project_filtered(&project_id, false)
        .await
        .unwrap();

    assert_eq!(active_tasks.len(), 1);
    assert_eq!(active_tasks[0].title, "Active Task");
}

#[tokio::test]
async fn test_get_by_project_filtered_includes_archived_when_requested() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task1 = create_test_task("Active Task");
    let task2 = create_test_task("Archived Task");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    let all_tasks = repo
        .get_by_project_filtered(&project_id, true)
        .await
        .unwrap();

    assert_eq!(all_tasks.len(), 2);
}

#[tokio::test]
async fn test_archive_and_restore_updates_updated_at() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Task");

    repo.create(task.clone()).await.unwrap();
    let original = repo.get_by_id(&task.id).await.unwrap().unwrap();

    // Archive
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    repo.archive(&task.id).await.unwrap();
    let archived = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(archived.updated_at > original.updated_at);

    // Restore
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    repo.restore(&task.id).await.unwrap();
    let restored = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(restored.updated_at > archived.updated_at);
}

// ==================== SEARCH OPERATION TESTS ====================

#[tokio::test]
async fn test_search_by_title() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task1 = create_test_task("Implement authentication");
    let task2 = create_test_task("Add user login");
    let task3 = create_test_task("Fix database bug");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.create(task3.clone()).await.unwrap();

    // Search for "auth" - should match "authentication"
    let results = repo.search(&project_id, "auth", false).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task1.id);
}

#[tokio::test]
async fn test_search_by_description() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task1 = create_test_task("Task One");
    task1.description = Some("This task implements authentication".to_string());

    let mut task2 = create_test_task("Task Two");
    task2.description = Some("This task adds logging".to_string());

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    // Search for "authentication" - should match description
    let results = repo
        .search(&project_id, "authentication", false)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task1.id);
}

#[tokio::test]
async fn test_search_case_insensitive() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = create_test_task("Add USER Authentication");
    repo.create(task.clone()).await.unwrap();

    // Search with lowercase - should match
    let results = repo.search(&project_id, "user", false).await.unwrap();
    assert_eq!(results.len(), 1);

    // Search with uppercase - should also match
    let results = repo.search(&project_id, "USER", false).await.unwrap();
    assert_eq!(results.len(), 1);

    // Search with mixed case - should also match
    let results = repo.search(&project_id, "UsEr", false).await.unwrap();
    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_search_returns_no_results_for_no_match() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = create_test_task("Add user login");
    repo.create(task.clone()).await.unwrap();

    // Search for something that doesn't exist
    let results = repo
        .search(&project_id, "nonexistent", false)
        .await
        .unwrap();
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_search_excludes_archived_by_default() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task1 = create_test_task("Active authentication task");
    let task2 = create_test_task("Archived authentication task");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    // Search without including archived - should only find active task
    let results = repo
        .search(&project_id, "authentication", false)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task1.id);
}

#[tokio::test]
async fn test_search_includes_archived_when_requested() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task1 = create_test_task("Active authentication task");
    let task2 = create_test_task("Archived authentication task");

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();
    repo.archive(&task2.id).await.unwrap();

    // Search with including archived - should find both tasks
    let results = repo
        .search(&project_id, "authentication", true)
        .await
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_search_matches_partial_strings() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = create_test_task("Implement user authentication system");
    repo.create(task.clone()).await.unwrap();

    // Search for partial match
    let results = repo.search(&project_id, "authen", false).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, task.id);
}

// ==================== BLOCKED REASON TESTS ====================

#[tokio::test]
async fn test_create_preserves_blocked_reason() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task = create_test_task("Blocked Task");
    task.internal_status = InternalStatus::Blocked;
    task.blocked_reason = Some("Waiting for API design".to_string());

    repo.create(task.clone()).await.unwrap();
    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();

    assert_eq!(
        found.blocked_reason,
        Some("Waiting for API design".to_string())
    );
    assert_eq!(found.internal_status, InternalStatus::Blocked);
}

#[tokio::test]
async fn test_update_preserves_blocked_reason() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task = create_test_task("Task");
    repo.create(task.clone()).await.unwrap();

    // Update to blocked with reason
    task.internal_status = InternalStatus::Blocked;
    task.blocked_reason = Some("Waiting for dependency".to_string());
    repo.update(&task).await.unwrap();

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(
        found.blocked_reason,
        Some("Waiting for dependency".to_string())
    );
    assert_eq!(found.internal_status, InternalStatus::Blocked);
}

#[tokio::test]
async fn test_update_clears_blocked_reason() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task = create_test_task("Task");
    task.internal_status = InternalStatus::Blocked;
    task.blocked_reason = Some("Waiting for something".to_string());
    repo.create(task.clone()).await.unwrap();

    // Unblock - clear the reason
    task.internal_status = InternalStatus::Ready;
    task.blocked_reason = None;
    repo.update(&task).await.unwrap();

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(found.blocked_reason.is_none());
    assert_eq!(found.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_blocked_reason_defaults_to_none() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let task = create_test_task("Normal Task");
    repo.create(task.clone()).await.unwrap();

    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(found.blocked_reason.is_none());
}

// ==================== IDEATION SESSION QUERY TESTS ====================

#[tokio::test]
async fn test_get_by_ideation_session_returns_matching_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let session_id = IdeationSessionId::from_string("test-session-1");

    let mut task1 = create_test_task("Session Task 1");
    task1.ideation_session_id = Some(session_id.clone());

    let mut task2 = create_test_task("Session Task 2");
    task2.ideation_session_id = Some(session_id.clone());

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    let result = repo.get_by_ideation_session(&session_id).await;

    assert!(result.is_ok());
    let tasks = result.unwrap();
    assert_eq!(tasks.len(), 2);
}

#[tokio::test]
async fn test_get_by_ideation_session_excludes_other_sessions() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let session_a = IdeationSessionId::from_string("session-a");
    let session_b = IdeationSessionId::from_string("session-b");

    let mut task1 = create_test_task("Task A");
    task1.ideation_session_id = Some(session_a.clone());

    let mut task2 = create_test_task("Task B");
    task2.ideation_session_id = Some(session_b.clone());

    let task3 = create_test_task("Task No Session");
    // task3 has no ideation_session_id (None)

    repo.create(task1).await.unwrap();
    repo.create(task2).await.unwrap();
    repo.create(task3).await.unwrap();

    let result_a = repo.get_by_ideation_session(&session_a).await.unwrap();
    assert_eq!(result_a.len(), 1);
    assert_eq!(result_a[0].title, "Task A");

    let result_b = repo.get_by_ideation_session(&session_b).await.unwrap();
    assert_eq!(result_b.len(), 1);
    assert_eq!(result_b[0].title, "Task B");
}

#[tokio::test]
async fn test_get_by_ideation_session_returns_empty_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let session_id = IdeationSessionId::from_string("nonexistent-session");

    let result = repo.get_by_ideation_session(&session_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_ideation_session_sorted_by_created_at() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let session_id = IdeationSessionId::from_string("test-session-sort");

    // Create tasks — they get created_at = Utc::now() sequentially
    let mut task1 = create_test_task("First Task");
    task1.ideation_session_id = Some(session_id.clone());

    let mut task2 = create_test_task("Second Task");
    task2.ideation_session_id = Some(session_id.clone());

    repo.create(task1).await.unwrap();
    repo.create(task2).await.unwrap();

    let tasks = repo.get_by_ideation_session(&session_id).await.unwrap();
    assert_eq!(tasks.len(), 2);
    // ORDER BY created_at ASC — first created should come first
    assert_eq!(tasks[0].title, "First Task");
    assert_eq!(tasks[1].title, "Second Task");
}

#[tokio::test]
async fn test_get_by_status_excludes_archived() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task1 = create_test_task("Active PendingMerge");
    task1.internal_status = InternalStatus::PendingMerge;

    let mut task2 = create_test_task("Archived PendingMerge");
    task2.internal_status = InternalStatus::PendingMerge;

    repo.create(task1).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    // Archive the second task
    repo.archive(&task2.id).await.unwrap();

    let results = repo
        .get_by_status(&project_id, InternalStatus::PendingMerge)
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Active PendingMerge");
}

// ==================== UPDATE METADATA TESTS ====================

#[tokio::test]
async fn test_update_metadata_sets_metadata_on_task_with_no_prior_metadata() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let task = create_test_task("Test Task");

    // Create task with no metadata
    repo.create(task.clone()).await.unwrap();

    // Update metadata
    let metadata = r#"{"failure_error":"Task execution failed"}"#;
    let result = repo
        .update_metadata(&task.id, Some(metadata.to_string()))
        .await;

    assert!(result.is_ok());

    // Verify metadata was set
    let updated = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(updated.metadata.is_some());
    assert_eq!(updated.metadata.unwrap(), metadata);
}

#[tokio::test]
async fn test_update_metadata_replaces_existing_metadata() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("Test Task");

    // Create task with initial metadata
    task.metadata = Some(r#"{"old_key":"old_value"}"#.to_string());
    repo.create(task.clone()).await.unwrap();

    // Replace with new metadata
    let new_metadata = r#"{"failure_error":"Task execution failed"}"#;
    let result = repo
        .update_metadata(&task.id, Some(new_metadata.to_string()))
        .await;

    assert!(result.is_ok());

    // Verify metadata was replaced
    let updated = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(updated.metadata.is_some());
    assert_eq!(updated.metadata.unwrap(), new_metadata);
}

#[tokio::test]
async fn test_update_metadata_sets_none_to_clear_metadata() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("Test Task");

    // Create task with metadata
    task.metadata = Some(r#"{"key":"value"}"#.to_string());
    repo.create(task.clone()).await.unwrap();

    // Clear metadata
    let result = repo.update_metadata(&task.id, None).await;

    assert!(result.is_ok());

    // Verify metadata was cleared
    let updated = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert!(updated.metadata.is_none());
}

#[tokio::test]
async fn test_update_metadata_does_not_change_internal_status() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("Test Task");

    // Set initial status
    task.internal_status = InternalStatus::Executing;
    repo.create(task.clone()).await.unwrap();

    // Update metadata
    let metadata = r#"{"key":"value"}"#;
    let result = repo
        .update_metadata(&task.id, Some(metadata.to_string()))
        .await;

    assert!(result.is_ok());

    // Verify status was not changed
    let updated = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Executing);
    assert_eq!(updated.metadata.unwrap(), metadata);
}

#[tokio::test]
async fn test_update_metadata_does_not_change_other_columns() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("Test Task");

    // Set up task with various fields
    task.description = Some("Original description".to_string());
    task.priority = 42;
    task.internal_status = InternalStatus::Ready;
    task.task_branch = Some("feature/test".to_string());
    task.worktree_path = Some("/path/to/worktree".to_string());
    task.blocked_reason = Some("Blocked by dependency".to_string());

    repo.create(task.clone()).await.unwrap();

    // Update metadata
    let metadata = r#"{"key":"value"}"#;
    let result = repo
        .update_metadata(&task.id, Some(metadata.to_string()))
        .await;

    assert!(result.is_ok());

    // Verify other columns were not changed
    let updated = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(
        updated.description,
        Some("Original description".to_string())
    );
    assert_eq!(updated.priority, 42);
    assert_eq!(updated.internal_status, InternalStatus::Ready);
    assert_eq!(updated.task_branch, Some("feature/test".to_string()));
    assert_eq!(updated.worktree_path, Some("/path/to/worktree".to_string()));
    assert_eq!(
        updated.blocked_reason,
        Some("Blocked by dependency".to_string())
    );
    assert_eq!(updated.metadata.unwrap(), metadata);
}

#[tokio::test]
async fn test_update_metadata_returns_ok_for_nonexistent_task() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let id = TaskId::new();

    // Try to update metadata on non-existent task
    let metadata = r#"{"key":"value"}"#;
    let result = repo.update_metadata(&id, Some(metadata.to_string())).await;

    // Should succeed (UPDATE affects 0 rows but doesn't error)
    assert!(result.is_ok());
}

// ==================== UPDATE_WITH_EXPECTED_STATUS TESTS ====================

#[tokio::test]
async fn test_update_with_expected_status_succeeds_when_status_matches() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("CAS Task");
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();

    task.title = "Updated Title".to_string();
    let result = repo
        .update_with_expected_status(&task, InternalStatus::Ready)
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap()); // returns true when update succeeds
    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(found.title, "Updated Title");
}

#[tokio::test]
async fn test_update_with_expected_status_returns_false_on_status_mismatch() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let mut task = create_test_task("CAS Task");
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();

    task.title = "Should Not Update".to_string();
    // Expect Executing but actual status is Ready — CAS fails
    let result = repo
        .update_with_expected_status(&task, InternalStatus::Executing)
        .await;

    assert!(result.is_ok());
    assert!(!result.unwrap()); // returns false when status mismatch
    let found = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(found.title, "CAS Task"); // unchanged
}

#[tokio::test]
async fn active_branch_update_fences_generic_status_writers_but_allows_same_status_metadata() {
    let db = setup_test_db();
    let shared = db.shared_conn();
    let repo = SqliteTaskRepository::from_shared(shared.clone());
    let branch_repo = SqliteBranchUpdateRepository::from_shared(shared);
    let task = repo.create(create_test_task("Fenced Task")).await.unwrap();
    let operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::PlanBranch,
        BranchUpdateContinuation::ResumeExecution,
        "fenced-history",
        "main",
        "ralphx/project/plan",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        GitTargetIdentity::new(
            PathBuf::from("/repo/.git"),
            "refs/heads/ralphx/project/plan",
        )
        .unwrap(),
        Utc::now(),
    );
    branch_repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "freshness".into(),
        })
        .await
        .unwrap();

    let mut stale = repo.get_by_id(&task.id).await.unwrap().unwrap();
    stale.internal_status = InternalStatus::Executing;
    assert!(!repo
        .update_with_expected_status(&stale, InternalStatus::UpdatingPlanBranch)
        .await
        .unwrap());

    let mut metadata_only = repo.get_by_id(&task.id).await.unwrap().unwrap();
    metadata_only.title = "Metadata still allowed".into();
    assert!(repo
        .update_with_expected_status(&metadata_only, InternalStatus::UpdatingPlanBranch)
        .await
        .unwrap());
    let stored = repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(stored.internal_status, InternalStatus::UpdatingPlanBranch);
    assert_eq!(stored.title, "Metadata still allowed");
}

// ==================== LIST_PAGINATED TESTS ====================

#[tokio::test]
async fn test_list_paginated_respects_limit() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    for i in 0..5 {
        repo.create(create_test_task(&format!("Task {}", i)))
            .await
            .unwrap();
    }

    let tasks = repo
        .list_paginated(&project_id, None, 0, 3, false, None, None, None)
        .await
        .unwrap();
    assert_eq!(tasks.len(), 3);
}

#[tokio::test]
async fn test_list_paginated_offset_skips_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    for i in 0..4 {
        repo.create(create_test_task(&format!("Task {}", i)))
            .await
            .unwrap();
    }

    let page1 = repo
        .list_paginated(&project_id, None, 0, 2, false, None, None, None)
        .await
        .unwrap();
    let page2 = repo
        .list_paginated(&project_id, None, 2, 2, false, None, None, None)
        .await
        .unwrap();

    assert_eq!(page1.len(), 2);
    assert_eq!(page2.len(), 2);
    assert_ne!(page1[0].id, page2[0].id);
}

#[tokio::test]
async fn test_list_paginated_filters_by_status() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut backlog = create_test_task("Backlog Task");
    backlog.internal_status = InternalStatus::Backlog;
    let mut ready = create_test_task("Ready Task");
    ready.internal_status = InternalStatus::Ready;
    repo.create(backlog).await.unwrap();
    repo.create(ready).await.unwrap();

    let tasks = repo
        .list_paginated(
            &project_id,
            Some(vec![InternalStatus::Ready]),
            0,
            10,
            false,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_list_paginated_include_archived_flag() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let active = create_test_task("Active");
    let to_archive = create_test_task("Archived");
    repo.create(active.clone()).await.unwrap();
    repo.create(to_archive.clone()).await.unwrap();
    repo.archive(&to_archive.id).await.unwrap();

    let active_only = repo
        .list_paginated(&project_id, None, 0, 10, false, None, None, None)
        .await
        .unwrap();
    assert_eq!(active_only.len(), 1);

    let with_archived = repo
        .list_paginated(&project_id, None, 0, 10, true, None, None, None)
        .await
        .unwrap();
    assert_eq!(with_archived.len(), 2);
}

#[tokio::test]
async fn test_list_paginated_filters_by_category() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let regular = Task::new_with_category(
        ProjectId::from_string("test-project".to_string()),
        "Regular Task".to_string(),
        TaskCategory::Regular,
    );
    let merge = Task::new_with_category(
        ProjectId::from_string("test-project".to_string()),
        "Merge Task".to_string(),
        TaskCategory::PlanMerge,
    );
    repo.create(regular).await.unwrap();
    repo.create(merge).await.unwrap();

    // Filter for plan_merge only
    let plan_merge_cats = vec!["plan_merge".to_string()];
    let merge_tasks = repo
        .list_paginated(
            &project_id,
            None,
            0,
            10,
            false,
            None,
            None,
            Some(&plan_merge_cats),
        )
        .await
        .unwrap();
    assert_eq!(merge_tasks.len(), 1);
    assert_eq!(merge_tasks[0].title, "Merge Task");

    // Filter for regular only
    let regular_cats = vec!["regular".to_string()];
    let regular_tasks = repo
        .list_paginated(
            &project_id,
            None,
            0,
            10,
            false,
            None,
            None,
            Some(&regular_cats),
        )
        .await
        .unwrap();
    assert_eq!(regular_tasks.len(), 1);
    assert_eq!(regular_tasks[0].title, "Regular Task");

    // No category filter returns all
    let all_tasks = repo
        .list_paginated(&project_id, None, 0, 10, false, None, None, None)
        .await
        .unwrap();
    assert_eq!(all_tasks.len(), 2);
}

// ==================== GET_OLDEST_READY_TASK(S) TESTS ====================

#[tokio::test]
async fn test_get_oldest_ready_task_returns_oldest() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task1 = create_test_task("Older Ready");
    task1.internal_status = InternalStatus::Ready;
    let mut task2 = create_test_task("Newer Ready");
    task2.internal_status = InternalStatus::Ready;

    repo.create(task1.clone()).await.unwrap();
    repo.create(task2.clone()).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, task1.id);
}

#[tokio::test]
async fn test_get_oldest_ready_task_returns_none_when_no_ready_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let task = create_test_task("Backlog Task");
    repo.create(task).await.unwrap();

    let result = repo.get_oldest_ready_task().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_oldest_ready_tasks_respects_limit() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    for i in 0..5 {
        let mut task = create_test_task(&format!("Ready {}", i));
        task.internal_status = InternalStatus::Ready;
        repo.create(task).await.unwrap();
    }

    let tasks = repo.get_oldest_ready_tasks(3).await.unwrap();
    assert_eq!(tasks.len(), 3);
}

#[tokio::test]
async fn test_get_oldest_ready_tasks_returns_empty_when_no_ready_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let tasks = repo.get_oldest_ready_tasks(10).await.unwrap();
    assert!(tasks.is_empty());
}

// ==================== GET_STALE_READY_TASKS TESTS ====================

#[tokio::test]
async fn test_get_stale_ready_tasks_includes_tasks_at_zero_threshold() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task = create_test_task("Ready Task");
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();

    // threshold_secs = 0: cutoff is now, so existing task created just before qualifies
    let stale = repo.get_stale_ready_tasks(0).await.unwrap();
    assert!(stale.iter().any(|t| t.id == task.id));
}

#[tokio::test]
async fn test_get_stale_ready_tasks_excludes_recent_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());

    let mut task = create_test_task("Recent Ready Task");
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();

    // threshold = 24h: a just-created task should not be considered stale
    let stale = repo.get_stale_ready_tasks(86400).await.unwrap();
    assert!(!stale.iter().any(|t| t.id == task.id));
}

// ==================== HAS_TASK_IN_STATES TESTS ====================

#[tokio::test]
async fn test_has_task_in_states_returns_true_when_match_exists() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task = create_test_task("Executing Task");
    task.internal_status = InternalStatus::Executing;
    repo.create(task).await.unwrap();

    let result = repo
        .has_task_in_states(&project_id, &[InternalStatus::Executing])
        .await
        .unwrap();
    assert!(result);
}

#[tokio::test]
async fn test_has_task_in_states_returns_false_when_no_match() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let task = create_test_task("Backlog Task");
    repo.create(task).await.unwrap();

    let result = repo
        .has_task_in_states(&project_id, &[InternalStatus::Executing])
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_has_task_in_states_returns_false_for_empty_statuses() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    repo.create(create_test_task("Any Task")).await.unwrap();

    let result = repo.has_task_in_states(&project_id, &[]).await.unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_has_task_in_states_excludes_archived_tasks() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task = create_test_task("Archived Ready");
    task.internal_status = InternalStatus::Ready;
    repo.create(task.clone()).await.unwrap();
    repo.archive(&task.id).await.unwrap();

    let result = repo
        .has_task_in_states(&project_id, &[InternalStatus::Ready])
        .await
        .unwrap();
    assert!(!result);
}

#[tokio::test]
async fn test_has_task_in_states_checks_multiple_statuses() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let mut task = create_test_task("QA Task");
    task.internal_status = InternalStatus::QaRefining;
    repo.create(task).await.unwrap();

    let result = repo
        .has_task_in_states(
            &project_id,
            &[InternalStatus::Executing, InternalStatus::QaRefining],
        )
        .await
        .unwrap();
    assert!(result);
}

// ==================== COUNT_TASKS TESTS ====================

#[tokio::test]
async fn test_count_tasks_returns_correct_count() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    repo.create(create_test_task("T1")).await.unwrap();
    repo.create(create_test_task("T2")).await.unwrap();
    repo.create(create_test_task("T3")).await.unwrap();

    let count = repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn test_count_tasks_excludes_archived_by_default() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let active = create_test_task("Active");
    let to_archive = create_test_task("Archived");
    repo.create(active).await.unwrap();
    repo.create(to_archive.clone()).await.unwrap();
    repo.archive(&to_archive.id).await.unwrap();

    let active_count = repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(active_count, 1);

    let all_count = repo
        .count_tasks(&project_id, true, None, None)
        .await
        .unwrap();
    assert_eq!(all_count, 2);
}

#[tokio::test]
async fn test_count_tasks_returns_zero_for_empty_project() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());

    let count = repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_count_tasks_filters_by_ideation_session() {
    let db = setup_test_db();
    let repo = SqliteTaskRepository::new(db.new_connection());
    let project_id = ProjectId::from_string("test-project".to_string());
    let session_id = IdeationSessionId::from_string("my-session");

    let mut session_task = create_test_task("Session Task");
    session_task.ideation_session_id = Some(session_id.clone());
    repo.create(session_task).await.unwrap();
    repo.create(create_test_task("Other Task")).await.unwrap();

    let session_count = repo
        .count_tasks(&project_id, false, Some("my-session"), None)
        .await
        .unwrap();
    assert_eq!(session_count, 1);

    let total_count = repo
        .count_tasks(&project_id, false, None, None)
        .await
        .unwrap();
    assert_eq!(total_count, 2);
}
