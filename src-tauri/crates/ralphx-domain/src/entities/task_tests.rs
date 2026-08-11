use super::*;
use chrono::Datelike;
use chrono::Timelike;

// ===== Task Creation Tests =====

#[test]
fn task_new_creates_with_defaults() {
    let project_id = ProjectId::new();
    let task = Task::new(project_id.clone(), "Test Task".to_string());

    assert_eq!(task.title, "Test Task");
    assert_eq!(task.project_id, project_id);
    assert_eq!(task.category, TaskCategory::Regular);
    assert!(task.description.is_none());
    assert_eq!(task.priority, 0);
    assert_eq!(task.internal_status, InternalStatus::Backlog);
    assert!(!task.needs_review_point);
    assert!(task.started_at.is_none());
    assert!(task.completed_at.is_none());
}

#[test]
fn task_new_generates_unique_id() {
    let project_id = ProjectId::new();
    let task1 = Task::new(project_id.clone(), "Task 1".to_string());
    let task2 = Task::new(project_id, "Task 2".to_string());

    assert_ne!(task1.id, task2.id);
}

#[test]
fn task_new_sets_timestamps() {
    let before = Utc::now();
    let task = Task::new(ProjectId::new(), "Test".to_string());
    let after = Utc::now();

    assert!(task.created_at >= before);
    assert!(task.created_at <= after);
    assert!(task.updated_at >= before);
    assert!(task.updated_at <= after);
    assert_eq!(task.created_at, task.updated_at);
}

#[test]
fn task_new_defaults_to_backlog_status() {
    let task = Task::new(ProjectId::new(), "Test".to_string());
    assert_eq!(task.internal_status, InternalStatus::Backlog);
}

#[test]
fn task_new_with_category_sets_category() {
    let task = Task::new_with_category(
        ProjectId::new(),
        "Bug Fix".to_string(),
        TaskCategory::Regular,
    );

    assert_eq!(task.category, TaskCategory::Regular);
    assert_eq!(task.title, "Bug Fix");
    assert_eq!(task.internal_status, InternalStatus::Backlog);
}

// ===== Task Method Tests =====

#[test]
fn task_touch_updates_timestamp() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    let original_updated = task.updated_at;
    let original_created = task.created_at;

    // Small delay to ensure time difference
    std::thread::sleep(std::time::Duration::from_millis(10));

    task.touch();

    assert_eq!(task.created_at, original_created);
    assert!(task.updated_at > original_updated);
}

#[test]
fn task_set_description_updates_and_touches() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    let original_updated = task.updated_at;

    std::thread::sleep(std::time::Duration::from_millis(10));

    task.set_description(Some("A detailed description".to_string()));

    assert_eq!(task.description, Some("A detailed description".to_string()));
    assert!(task.updated_at > original_updated);
}

#[test]
fn task_set_description_to_none() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.description = Some("Initial description".to_string());

    task.set_description(None);

    assert!(task.description.is_none());
}

#[test]
fn task_set_priority_updates_and_touches() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    let original_updated = task.updated_at;

    std::thread::sleep(std::time::Duration::from_millis(10));

    task.set_priority(10);

    assert_eq!(task.priority, 10);
    assert!(task.updated_at > original_updated);
}

#[test]
fn task_set_priority_negative() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.set_priority(-5);
    assert_eq!(task.priority, -5);
}

// ===== Task State Helper Tests =====

/// Approved is NOT terminal — it transitions to PendingMerge (still needs merging).
#[test]
fn task_is_not_terminal_for_approved() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.internal_status = InternalStatus::Approved;
    assert!(
        !task.is_terminal(),
        "Approved should NOT be terminal — it must merge to be done"
    );
}

#[test]
fn task_is_terminal_for_failed() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.internal_status = InternalStatus::Failed;
    assert!(task.is_terminal());
}

#[test]
fn task_is_terminal_for_cancelled() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.internal_status = InternalStatus::Cancelled;
    assert!(task.is_terminal());
}

#[test]
fn task_is_terminal_for_stopped() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.internal_status = InternalStatus::Stopped;
    assert!(task.is_terminal());
}

#[test]
fn task_is_terminal_for_merge_incomplete() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.internal_status = InternalStatus::MergeIncomplete;
    assert!(
        task.is_terminal(),
        "MergeIncomplete should be terminal for dependency unblocking"
    );
}

#[test]
fn task_is_not_terminal_for_paused() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.internal_status = InternalStatus::Paused;
    assert!(!task.is_terminal());
}

#[test]
fn task_is_not_terminal_for_active_states() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());

    for status in &[
        InternalStatus::Backlog,
        InternalStatus::Ready,
        InternalStatus::Executing,
        InternalStatus::QaRefining,
        InternalStatus::QaTesting,
        InternalStatus::QaPassed,
        InternalStatus::QaFailed,
        InternalStatus::PendingReview,
        InternalStatus::RevisionNeeded,
        InternalStatus::Blocked,
    ] {
        task.internal_status = *status;
        assert!(!task.is_terminal(), "{:?} should not be terminal", status);
    }
}

#[test]
fn task_is_active_for_working_states() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());

    let active_states = [
        InternalStatus::Executing,
        InternalStatus::QaRefining,
        InternalStatus::QaTesting,
        InternalStatus::PendingReview,
    ];

    for status in &active_states {
        task.internal_status = *status;
        assert!(task.is_active(), "{:?} should be active", status);
    }
}

#[test]
fn task_is_not_active_for_idle_states() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());

    let idle_states = [
        InternalStatus::Backlog,
        InternalStatus::Ready,
        InternalStatus::Blocked,
        InternalStatus::QaPassed,
        InternalStatus::QaFailed,
        InternalStatus::RevisionNeeded,
        InternalStatus::Approved,
        InternalStatus::Failed,
        InternalStatus::Cancelled,
    ];

    for status in &idle_states {
        task.internal_status = *status;
        assert!(!task.is_active(), "{:?} should not be active", status);
    }
}

// ===== Task Serialization Tests =====

#[test]
fn task_serializes_to_json() {
    let task = Task::new(
        ProjectId::from_string("proj-123".to_string()),
        "JSON Test".to_string(),
    );
    let json = serde_json::to_string(&task).expect("Should serialize");

    assert!(json.contains("\"title\":\"JSON Test\""));
    assert!(json.contains("\"project_id\":\"proj-123\""));
    assert!(json.contains("\"category\":\"regular\""));
    assert!(json.contains("\"internal_status\":\"backlog\""));
    assert!(json.contains("\"priority\":0"));
    assert!(json.contains("\"needs_review_point\":false"));
}

#[test]
fn task_deserializes_from_json() {
    let json = r#"{
        "id": "task-id-123",
        "project_id": "proj-id-456",
        "category": "bug",
        "title": "Fix the bug",
        "description": "Detailed description here",
        "priority": 5,
        "internal_status": "executing",
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T13:00:00Z",
        "started_at": "2025-01-24T12:30:00Z",
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).expect("Should deserialize");

    assert_eq!(task.id.as_str(), "task-id-123");
    assert_eq!(task.project_id.as_str(), "proj-id-456");
    // "bug" is a legacy string — maps to Regular via FromStr fallback
    assert_eq!(task.category, TaskCategory::Regular);
    assert_eq!(task.title, "Fix the bug");
    assert_eq!(
        task.description,
        Some("Detailed description here".to_string())
    );
    assert_eq!(task.priority, 5);
    assert_eq!(task.internal_status, InternalStatus::Executing);
    assert!(task.started_at.is_some());
    assert!(task.completed_at.is_none());
}

#[test]
fn task_deserializes_with_null_optionals() {
    let json = r#"{
        "id": "task-id",
        "project_id": "proj-id",
        "category": "feature",
        "title": "Minimal",
        "description": null,
        "priority": 0,
        "internal_status": "backlog",
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T12:00:00Z",
        "started_at": null,
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).expect("Should deserialize");

    assert!(task.description.is_none());
    assert!(task.started_at.is_none());
    assert!(task.completed_at.is_none());
}

#[test]
fn task_roundtrip_serialization() {
    let mut original = Task::new(ProjectId::new(), "Roundtrip".to_string());
    original.description = Some("Test description".to_string());
    original.priority = 42;
    original.internal_status = InternalStatus::Ready;

    let json = serde_json::to_string(&original).expect("Should serialize");
    let restored: Task = serde_json::from_str(&json).expect("Should deserialize");

    assert_eq!(original.id, restored.id);
    assert_eq!(original.project_id, restored.project_id);
    assert_eq!(original.category, restored.category); // TaskCategory::Regular roundtrips correctly
    assert_eq!(original.title, restored.title);
    assert_eq!(original.description, restored.description);
    assert_eq!(original.priority, restored.priority);
    assert_eq!(original.internal_status, restored.internal_status);
}

// ===== Task Clone Tests =====

#[test]
fn task_clone_works() {
    let original = Task::new(ProjectId::new(), "Clone Test".to_string());
    let cloned = original.clone();

    assert_eq!(original.id, cloned.id);
    assert_eq!(original.title, cloned.title);
    assert_eq!(original.project_id, cloned.project_id);
    assert_eq!(original.internal_status, cloned.internal_status);
}

#[test]
fn task_clone_is_independent() {
    let original = Task::new(ProjectId::new(), "Independent".to_string());
    let mut cloned = original.clone();

    cloned.touch();

    // Original should be unchanged
    assert_ne!(original.updated_at, cloned.updated_at);
}

// ===== Task Category Tests =====

#[test]
fn task_supports_various_categories() {
    let project_id = ProjectId::new();

    // Regular category (default)
    let task = Task::new_with_category(
        project_id.clone(),
        "Regular task".to_string(),
        TaskCategory::Regular,
    );
    assert_eq!(task.category, TaskCategory::Regular);

    // PlanMerge category (system-managed)
    let task = Task::new_with_category(
        project_id.clone(),
        "Plan merge task".to_string(),
        TaskCategory::PlanMerge,
    );
    assert_eq!(task.category, TaskCategory::PlanMerge);
}

// ===== Debug Format Test =====

#[test]
fn task_debug_format_works() {
    let task = Task::new(ProjectId::new(), "Debug Test".to_string());
    let debug = format!("{:?}", task);

    assert!(debug.contains("Task"));
    assert!(debug.contains("Debug Test"));
    assert!(debug.contains("Regular"));
    assert!(debug.contains("Backlog"));
}

// ===== parse_datetime Tests =====

#[test]
fn task_parse_datetime_rfc3339() {
    let dt = Task::parse_datetime("2026-01-24T12:30:00Z".to_string());
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 1);
    assert_eq!(dt.day(), 24);
    assert_eq!(dt.hour(), 12);
    assert_eq!(dt.minute(), 30);
}

#[test]
fn task_parse_datetime_rfc3339_with_offset() {
    let dt = Task::parse_datetime("2026-01-24T12:30:00+05:00".to_string());
    // Should convert to UTC
    assert_eq!(dt.hour(), 7); // 12:30 - 5 hours = 7:30 UTC
}

#[test]
fn task_parse_datetime_sqlite_format() {
    let dt = Task::parse_datetime("2026-01-24 15:45:30".to_string());
    assert_eq!(dt.year(), 2026);
    assert_eq!(dt.month(), 1);
    assert_eq!(dt.day(), 24);
    assert_eq!(dt.hour(), 15);
    assert_eq!(dt.minute(), 45);
    assert_eq!(dt.second(), 30);
}

#[test]
fn task_parse_datetime_invalid_returns_now() {
    let before = Utc::now();
    let dt = Task::parse_datetime("not-a-date".to_string());
    let after = Utc::now();

    assert!(dt >= before);
    assert!(dt <= after);
}

// ===== from_row Integration Tests =====

use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute(
        r#"CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL,
            category TEXT NOT NULL DEFAULT 'feature',
            title TEXT NOT NULL,
            description TEXT,
            priority INTEGER DEFAULT 0,
            internal_status TEXT NOT NULL DEFAULT 'backlog',
            needs_review_point INTEGER DEFAULT 0,
            source_proposal_id TEXT,
            plan_artifact_id TEXT,
            ideation_session_id TEXT,
            execution_plan_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            started_at TEXT,
            completed_at TEXT,
            archived_at TEXT,
            blocked_reason TEXT,
            task_branch TEXT,
            task_branch_base_ref TEXT,
            task_branch_base_sha TEXT,
            worktree_path TEXT,
            merge_commit_sha TEXT,
            metadata TEXT,
            merge_pipeline_active TEXT
        )"#,
        [],
    )
    .unwrap();
    conn
}

#[test]
fn task_from_row_with_all_fields() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-123', 'proj-456', 'bug', 'Fix crash', 'Critical bug fix', 10,
           'executing', 1, '2026-01-24T10:00:00Z', '2026-01-24T11:00:00Z',
           '2026-01-24T10:30:00Z', NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-123'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert_eq!(task.id.as_str(), "task-123");
    assert_eq!(task.project_id.as_str(), "proj-456");
    // "bug" is a legacy string — maps to Regular via FromStr fallback
    assert_eq!(task.category, TaskCategory::Regular);
    assert_eq!(task.title, "Fix crash");
    assert_eq!(task.description, Some("Critical bug fix".to_string()));
    assert_eq!(task.priority, 10);
    assert_eq!(task.internal_status, InternalStatus::Executing);
    assert!(task.needs_review_point);
    assert!(task.started_at.is_some());
    assert!(task.completed_at.is_none());
    assert!(task.task_branch_base_ref.is_none());
    assert!(task.task_branch_base_sha.is_none());
}

#[test]
fn task_from_row_with_null_optionals() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-789', 'proj-000', 'feature', 'New feature', NULL, 0,
           'backlog', 0, '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-789'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert!(task.description.is_none());
    assert!(!task.needs_review_point);
    assert!(task.started_at.is_none());
    assert!(task.completed_at.is_none());
}

#[test]
fn task_from_row_with_sqlite_datetime_format() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-sql', 'proj-sql', 'setup', 'Setup', NULL, 5,
           'ready', 0, '2026-01-24 12:00:00', '2026-01-24 12:30:00', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-sql'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert_eq!(task.created_at.hour(), 12);
    assert_eq!(task.updated_at.hour(), 12);
    assert_eq!(task.updated_at.minute(), 30);
}

#[test]
fn task_from_row_with_unknown_status_defaults_to_backlog() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-unk', 'proj-unk', 'feature', 'Test', NULL, 0,
           'unknown_status', 0, '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-unk'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    // Unknown status should default to Backlog
    assert_eq!(task.internal_status, InternalStatus::Backlog);
}

#[test]
fn task_from_row_with_completed_at() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-done', 'proj-done', 'feature', 'Completed', NULL, 0,
           'approved', 0, '2026-01-24T08:00:00Z', '2026-01-24T12:00:00Z',
           '2026-01-24T09:00:00Z', '2026-01-24T11:00:00Z')"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-done'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert_eq!(task.internal_status, InternalStatus::Approved);
    assert!(task.started_at.is_some());
    assert!(task.completed_at.is_some());
    assert_eq!(task.completed_at.unwrap().hour(), 11);
}

#[test]
fn task_from_row_all_14_statuses() {
    let conn = setup_test_db();
    let statuses = [
        "backlog",
        "ready",
        "blocked",
        "executing",
        "qa_refining",
        "qa_testing",
        "qa_passed",
        "qa_failed",
        "pending_review",
        "reviewing",
        "review_passed",
        "revision_needed",
        "re_executing",
        "approved",
        "failed",
        "cancelled",
    ];

    for (i, status) in statuses.iter().enumerate() {
        let id = format!("task-{}", i);
        conn.execute(
            &format!(
                r#"INSERT INTO tasks (id, project_id, category, title, internal_status, needs_review_point, created_at, updated_at)
                   VALUES ('{}', 'proj-1', 'feature', 'Test', '{}', 0, '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z')"#,
                id, status
            ),
            [],
        )
        .unwrap();

        let task: Task = conn
            .query_row(
                &format!("SELECT * FROM tasks WHERE id = '{}'", id),
                [],
                |row| Task::from_row(row),
            )
            .unwrap();

        assert_eq!(task.internal_status.as_str(), *status);
    }
}

// ===== needs_review_point Tests =====

#[test]
fn task_new_defaults_needs_review_point_to_false() {
    let task = Task::new(ProjectId::new(), "Test".to_string());
    assert!(!task.needs_review_point);
}

#[test]
fn task_set_needs_review_point() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    let original_updated = task.updated_at;

    std::thread::sleep(std::time::Duration::from_millis(10));

    task.set_needs_review_point(true);

    assert!(task.needs_review_point);
    assert!(task.updated_at > original_updated);
}

#[test]
fn task_set_needs_review_point_to_false() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.needs_review_point = true;

    task.set_needs_review_point(false);

    assert!(!task.needs_review_point);
}

#[test]
fn task_serializes_needs_review_point() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.needs_review_point = true;

    let json = serde_json::to_string(&task).unwrap();
    assert!(json.contains("\"needs_review_point\":true"));

    task.needs_review_point = false;
    let json = serde_json::to_string(&task).unwrap();
    assert!(json.contains("\"needs_review_point\":false"));
}

#[test]
fn task_deserializes_with_needs_review_point() {
    let json = r#"{
        "id": "task-id",
        "project_id": "proj-id",
        "category": "feature",
        "title": "Test",
        "description": null,
        "priority": 0,
        "internal_status": "backlog",
        "needs_review_point": true,
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T12:00:00Z",
        "started_at": null,
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).unwrap();
    assert!(task.needs_review_point);
}

#[test]
fn task_deserializes_without_needs_review_point_defaults_to_false() {
    // Test backward compatibility - field missing should default to false
    let json = r#"{
        "id": "task-id",
        "project_id": "proj-id",
        "category": "feature",
        "title": "Test",
        "description": null,
        "priority": 0,
        "internal_status": "backlog",
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T12:00:00Z",
        "started_at": null,
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).unwrap();
    assert!(!task.needs_review_point);
}

#[test]
fn task_from_row_with_needs_review_point_true() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-rp', 'proj-rp', 'feature', 'Review Point Task', NULL, 0,
           'backlog', 1, '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-rp'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert!(task.needs_review_point);
}

#[test]
fn task_from_row_with_null_needs_review_point_defaults_to_false() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, created_at, updated_at, started_at, completed_at)
           VALUES ('task-nrp', 'proj-nrp', 'feature', 'No Review Point', NULL, 0,
           'backlog', NULL, '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-nrp'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert!(!task.needs_review_point);
}

// ===== Traceability Fields Tests =====

#[test]
fn task_new_defaults_traceability_fields_to_none() {
    let task = Task::new(ProjectId::new(), "Test".to_string());
    assert!(task.source_proposal_id.is_none());
    assert!(task.plan_artifact_id.is_none());
}

#[test]
fn task_serializes_with_traceability_fields() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.source_proposal_id = Some(TaskProposalId::from_string("proposal-123"));
    task.plan_artifact_id = Some(ArtifactId::from_string("artifact-456"));

    let json = serde_json::to_string(&task).unwrap();
    assert!(json.contains("\"source_proposal_id\":\"proposal-123\""));
    assert!(json.contains("\"plan_artifact_id\":\"artifact-456\""));
}

#[test]
fn task_deserializes_with_traceability_fields() {
    let json = r#"{
        "id": "task-id",
        "project_id": "proj-id",
        "category": "feature",
        "title": "Test",
        "description": null,
        "priority": 0,
        "internal_status": "backlog",
        "needs_review_point": false,
        "source_proposal_id": "proposal-abc",
        "plan_artifact_id": "artifact-xyz",
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T12:00:00Z",
        "started_at": null,
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).unwrap();
    assert_eq!(task.source_proposal_id.unwrap().as_str(), "proposal-abc");
    assert_eq!(task.plan_artifact_id.unwrap().as_str(), "artifact-xyz");
}

#[test]
fn task_from_row_with_traceability_fields() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, source_proposal_id, plan_artifact_id,
           created_at, updated_at, started_at, completed_at)
           VALUES ('task-trace', 'proj-trace', 'feature', 'Traceable Task', NULL, 0,
           'backlog', 0, 'proposal-123', 'artifact-456',
           '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-trace'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert_eq!(task.source_proposal_id.unwrap().as_str(), "proposal-123");
    assert_eq!(task.plan_artifact_id.unwrap().as_str(), "artifact-456");
}

#[test]
fn task_from_row_with_null_traceability_fields() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, source_proposal_id, plan_artifact_id,
           created_at, updated_at, started_at, completed_at)
           VALUES ('task-null', 'proj-null', 'feature', 'No Traceability', NULL, 0,
           'backlog', 0, NULL, NULL,
           '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-null'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert!(task.source_proposal_id.is_none());
    assert!(task.plan_artifact_id.is_none());
}

// ===== blocked_reason Tests =====

#[test]
fn task_new_defaults_blocked_reason_to_none() {
    let task = Task::new(ProjectId::new(), "Test".to_string());
    assert!(task.blocked_reason.is_none());
}

#[test]
fn task_serializes_with_blocked_reason() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.blocked_reason = Some("Waiting for API design".to_string());

    let json = serde_json::to_string(&task).unwrap();
    assert!(json.contains("\"blocked_reason\":\"Waiting for API design\""));
}

#[test]
fn task_deserializes_with_blocked_reason() {
    let json = r#"{
        "id": "task-id",
        "project_id": "proj-id",
        "category": "feature",
        "title": "Test",
        "description": null,
        "priority": 0,
        "internal_status": "blocked",
        "needs_review_point": false,
        "blocked_reason": "Waiting for dependency",
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T12:00:00Z",
        "started_at": null,
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).unwrap();
    assert_eq!(
        task.blocked_reason,
        Some("Waiting for dependency".to_string())
    );
    assert_eq!(task.internal_status, InternalStatus::Blocked);
}

#[test]
fn task_deserializes_without_blocked_reason_defaults_to_none() {
    let json = r#"{
        "id": "task-id",
        "project_id": "proj-id",
        "category": "feature",
        "title": "Test",
        "description": null,
        "priority": 0,
        "internal_status": "backlog",
        "created_at": "2025-01-24T12:00:00Z",
        "updated_at": "2025-01-24T12:00:00Z",
        "started_at": null,
        "completed_at": null
    }"#;

    let task: Task = serde_json::from_str(json).unwrap();
    assert!(task.blocked_reason.is_none());
}

#[test]
fn task_from_row_with_blocked_reason() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, blocked_reason,
           created_at, updated_at, started_at, completed_at)
           VALUES ('task-blocked', 'proj-blocked', 'feature', 'Blocked Task', NULL, 0,
           'blocked', 0, 'Waiting for API design',
           '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-blocked'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert_eq!(
        task.blocked_reason,
        Some("Waiting for API design".to_string())
    );
    assert_eq!(task.internal_status, InternalStatus::Blocked);
}

#[test]
fn task_from_row_with_null_blocked_reason() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO tasks (id, project_id, category, title, description, priority,
           internal_status, needs_review_point, blocked_reason,
           created_at, updated_at, started_at, completed_at)
           VALUES ('task-noblock', 'proj-noblock', 'feature', 'Not Blocked', NULL, 0,
           'ready', 0, NULL,
           '2026-01-24T08:00:00Z', '2026-01-24T08:00:00Z', NULL, NULL)"#,
        [],
    )
    .unwrap();

    let task: Task = conn
        .query_row("SELECT * FROM tasks WHERE id = 'task-noblock'", [], |row| {
            Task::from_row(row)
        })
        .unwrap();

    assert!(task.blocked_reason.is_none());
    assert_eq!(task.internal_status, InternalStatus::Ready);
}
