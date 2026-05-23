use serde_json::json;

use super::MemoryAgentTaskRepository;
use crate::domain::entities::{AgentTaskCreate, AgentTaskPatch, AgentTaskScope, AgentTaskState};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};

fn scope() -> AgentTaskScope {
    AgentTaskScope::new("conversation", "conv-1")
}

fn create(title: &str) -> AgentTaskCreate {
    AgentTaskCreate {
        title: title.to_string(),
        details: format!("Details for {title}"),
        active_label: None,
        owner_agent: None,
        metadata: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
    }
}

#[tokio::test]
async fn create_assigns_monotonic_task_numbers() {
    let repo = MemoryAgentTaskRepository::new();

    let first = repo.create_task(&scope(), create("First")).await.unwrap();
    let second = repo.create_task(&scope(), create("Second")).await.unwrap();

    assert_eq!(first.task.task_number, 1);
    assert_eq!(second.task.task_number, 2);
}

#[tokio::test]
async fn create_rolls_over_after_current_list_is_terminal() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();

    repo.create_task(&scope, create("First slice"))
        .await
        .unwrap();
    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            state: Some(AgentTaskState::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let second_slice = repo
        .create_task(&scope, create("Second slice"))
        .await
        .unwrap();
    assert_eq!(second_slice.task.task_number, 1);

    let current_tasks = repo
        .list_tasks(&scope, AgentTaskListOptions { include_done: true })
        .await
        .unwrap();
    assert_eq!(current_tasks.len(), 1);
    assert_eq!(current_tasks[0].title, "Second slice");
}

#[tokio::test]
async fn list_task_lists_and_fetch_previous_list_tasks() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();

    repo.create_task(&scope, create("First slice"))
        .await
        .unwrap();
    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            state: Some(AgentTaskState::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    repo.create_task(&scope, create("Second slice"))
        .await
        .unwrap();

    let lists = repo.list_task_lists(&scope).await.unwrap();
    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].list_sequence, 2);
    assert_eq!(lists[0].open_count, 1);
    assert_eq!(lists[1].list_sequence, 1);
    assert_eq!(lists[1].done_count, 1);

    let previous_tasks = repo
        .list_tasks_for_list(
            &scope,
            &lists[1].list_id,
            AgentTaskListOptions { include_done: true },
        )
        .await
        .unwrap();
    assert_eq!(previous_tasks.len(), 1);
    assert_eq!(previous_tasks[0].title, "First slice");
}

#[tokio::test]
async fn list_hides_resolved_blockers_but_get_keeps_full_dependencies() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(&scope, create("Design")).await.unwrap();
    repo.create_task(&scope, create("Build")).await.unwrap();

    repo.update_task(
        &scope,
        "2",
        AgentTaskPatch {
            add_blocked_by: vec!["1".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert_eq!(listed[1].blocked_by, vec!["1"]);

    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            state: Some(AgentTaskState::Done),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let listed = repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert!(listed[0].blocked_by.is_empty());
    let build = repo.get_task(&scope, "2").await.unwrap().unwrap();
    assert_eq!(build.blocked_by, vec!["1"]);
    assert!(build.unresolved_blocked_by.is_empty());
}

#[tokio::test]
async fn metadata_patch_merges_and_removes_null_keys() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(
        &scope,
        AgentTaskCreate {
            metadata: Some(json!({"priority": "high", "stale": true})),
            ..create("Track metadata")
        },
    )
    .await
    .unwrap();

    let updated = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                metadata_patch: Some(json!({"owner": "planner", "stale": null})),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        updated.task.metadata,
        Some(json!({"priority": "high", "owner": "planner"}))
    );
}

#[tokio::test]
async fn dependency_cycle_is_rejected() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(&scope, create("A")).await.unwrap();
    repo.create_task(&scope, create("B")).await.unwrap();

    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            add_blocks: vec!["2".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let err = repo
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                add_blocks: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("cycle"));
}

#[tokio::test]
async fn update_changes_all_mutable_fields_and_supports_id_refs() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    let created = repo.create_task(&scope, create("Draft")).await.unwrap();
    let task_id = created.task.task_id.to_string();

    let updated = repo
        .update_task(
            &scope,
            &task_id,
            AgentTaskPatch {
                title: Some("Draft refined".to_string()),
                details: Some("Updated details".to_string()),
                active_label: Some(Some("Refining".to_string())),
                owner_agent: Some(Some("planner".to_string())),
                state: Some(AgentTaskState::Active),
                metadata_patch: Some(json!({"priority": "high"})),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        updated.changed_fields,
        vec![
            "active_label",
            "details",
            "metadata",
            "owner_agent",
            "state",
            "title"
        ]
    );
    assert_eq!(updated.task.title, "Draft refined");
    assert_eq!(updated.task.details, "Updated details");
    assert_eq!(updated.task.active_label.as_deref(), Some("Refining"));
    assert_eq!(updated.task.owner_agent.as_deref(), Some("planner"));
    assert_eq!(updated.task.state, AgentTaskState::Active);
    assert_eq!(updated.task.version, 2);
    assert!(updated.task.completed_at.is_none());
    assert_eq!(updated.state_change.unwrap().from, AgentTaskState::Open);

    let fetched = repo.get_task(&scope, &task_id).await.unwrap().unwrap();
    assert_eq!(fetched.task_number, 1);
}

#[tokio::test]
async fn remove_dependencies_and_filter_done_tasks() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();
    repo.create_task(&scope, create("Blocker")).await.unwrap();
    repo.create_task(
        &scope,
        AgentTaskCreate {
            blocked_by: vec!["1".to_string()],
            ..create("Blocked")
        },
    )
    .await
    .unwrap();

    let blocked = repo.get_task(&scope, "2").await.unwrap().unwrap();
    assert_eq!(blocked.blocked_by, vec!["1"]);
    assert_eq!(blocked.unresolved_blocked_by, vec!["1"]);

    let removed = repo
        .update_task(
            &scope,
            "2",
            AgentTaskPatch {
                remove_blocked_by: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(removed.changed_fields, vec!["dependencies"]);
    assert!(removed.task.blocked_by.is_empty());

    let completed = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                add_blocks: vec!["2".to_string()],
                state: Some(AgentTaskState::Done),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(completed.task.completed_at.is_some());

    let active_only = repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap();
    assert_eq!(active_only.len(), 1);
    assert_eq!(active_only[0].task_number, 2);

    let all_tasks = repo
        .list_tasks(&scope, AgentTaskListOptions { include_done: true })
        .await
        .unwrap();
    assert_eq!(all_tasks.len(), 2);

    repo.update_task(
        &scope,
        "1",
        AgentTaskPatch {
            remove_blocks: vec!["2".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(repo
        .get_task(&scope, "1")
        .await
        .unwrap()
        .unwrap()
        .blocks
        .is_empty());
}

#[tokio::test]
async fn validates_missing_tasks_and_blank_fields() {
    let repo = MemoryAgentTaskRepository::new();
    let scope = scope();

    assert!(repo.get_task(&scope, "1").await.unwrap().is_none());
    assert!(repo
        .list_tasks(&scope, AgentTaskListOptions::default())
        .await
        .unwrap()
        .is_empty());
    assert!(repo
        .update_task(&scope, "1", AgentTaskPatch::default())
        .await
        .unwrap()
        .is_none());

    let title_err = repo.create_task(&scope, create("")).await.unwrap_err();
    assert!(title_err.to_string().contains("title"));

    repo.create_task(&scope, create("Valid")).await.unwrap();
    let details_err = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                details: Some(" ".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(details_err.to_string().contains("details"));

    let missing_dependency = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                add_blocked_by: vec!["missing".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(missing_dependency
        .to_string()
        .contains("dependency not found"));

    let self_dependency = repo
        .update_task(
            &scope,
            "1",
            AgentTaskPatch {
                add_blocks: vec!["1".to_string()],
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(self_dependency.to_string().contains("reference itself"));
}
