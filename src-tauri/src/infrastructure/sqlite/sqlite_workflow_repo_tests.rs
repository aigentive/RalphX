use super::*;
use crate::domain::entities::{InternalStatus, WorkflowColumn};
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_workflow_repo_tests")
}

fn create_test_workflow() -> WorkflowSchema {
    WorkflowSchema::new(
        "Test Workflow",
        vec![
            WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
            WorkflowColumn::new("ready", "Ready", InternalStatus::Ready),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    )
}

#[tokio::test]
async fn test_create_workflow() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());
    let workflow = create_test_workflow();

    let result = repo.create(workflow.clone()).await;
    assert!(result.is_ok());

    let created = result.unwrap();
    assert_eq!(created.id, workflow.id);
    assert_eq!(created.name, "Test Workflow");
}

#[tokio::test]
async fn test_get_by_id_found() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());
    let workflow = create_test_workflow();

    repo.create(workflow.clone()).await.unwrap();

    let result = repo.get_by_id(&workflow.id).await;
    assert!(result.is_ok());

    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Test Workflow");
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());
    let id = WorkflowId::new();

    let result = repo.get_by_id(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_all_empty() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let result = repo.get_all().await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_all_with_workflows() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let workflow1 = create_test_workflow();
    let mut workflow2 = create_test_workflow();
    workflow2.id = WorkflowId::new();
    workflow2.name = "Another Workflow".to_string();

    repo.create(workflow1).await.unwrap();
    repo.create(workflow2).await.unwrap();

    let result = repo.get_all().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 2);
}

#[tokio::test]
async fn test_get_all_returns_sorted_by_name() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let mut workflow1 = create_test_workflow();
    workflow1.name = "Zebra Workflow".to_string();

    let mut workflow2 = create_test_workflow();
    workflow2.id = WorkflowId::new();
    workflow2.name = "Alpha Workflow".to_string();

    repo.create(workflow1).await.unwrap();
    repo.create(workflow2).await.unwrap();

    let result = repo.get_all().await.unwrap();
    assert_eq!(result[0].name, "Alpha Workflow");
    assert_eq!(result[1].name, "Zebra Workflow");
}

#[tokio::test]
async fn test_get_default_none() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    // Create a non-default workflow
    let workflow = create_test_workflow();
    repo.create(workflow).await.unwrap();

    let result = repo.get_default().await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_default_returns_default() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let workflow = WorkflowSchema::default_ralphx();
    repo.create(workflow).await.unwrap();

    let result = repo.get_default().await;
    assert!(result.is_ok());

    let default = result.unwrap();
    assert!(default.is_some());
    assert!(default.unwrap().is_default);
}

#[tokio::test]
async fn test_update_workflow() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let mut workflow = create_test_workflow();
    repo.create(workflow.clone()).await.unwrap();

    workflow.name = "Updated Name".to_string();
    workflow.description = Some("New description".to_string());

    let result = repo.update(&workflow).await;
    assert!(result.is_ok());

    let updated = repo.get_by_id(&workflow.id).await.unwrap().unwrap();
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.description, Some("New description".to_string()));
}

#[tokio::test]
async fn test_set_default_unsets_previous() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    // Create first workflow as default
    let workflow1 = WorkflowSchema::default_ralphx();
    repo.create(workflow1.clone()).await.unwrap();

    // Create second non-default workflow
    let workflow2 = create_test_workflow();
    repo.create(workflow2.clone()).await.unwrap();

    // Set second as default
    repo.set_default(&workflow2.id).await.unwrap();

    // Verify first is no longer default
    let updated1 = repo.get_by_id(&workflow1.id).await.unwrap().unwrap();
    assert!(!updated1.is_default);

    // Verify second is now default
    let updated2 = repo.get_by_id(&workflow2.id).await.unwrap().unwrap();
    assert!(updated2.is_default);

    // Verify get_default returns the second
    let default = repo.get_default().await.unwrap().unwrap();
    assert_eq!(default.id, workflow2.id);
}

#[tokio::test]
async fn test_workflow_columns_preserved() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let workflow = WorkflowSchema::default_ralphx();
    repo.create(workflow.clone()).await.unwrap();

    let loaded = repo.get_by_id(&workflow.id).await.unwrap().unwrap();
    assert_eq!(loaded.columns.len(), 5);

    // Verify column mappings
    let draft = loaded.columns.iter().find(|c| c.id == "draft");
    assert!(draft.is_some());
    assert_eq!(draft.unwrap().maps_to, InternalStatus::Backlog);

    let done = loaded.columns.iter().find(|c| c.id == "done");
    assert!(done.is_some());
    assert_eq!(done.unwrap().maps_to, InternalStatus::Approved);
}

#[tokio::test]
async fn test_workflow_with_behavior_preserved() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    use crate::domain::entities::ColumnBehavior;

    let mut workflow = create_test_workflow();
    workflow.columns[0] = workflow.columns[0].clone().with_behavior(
        ColumnBehavior::new()
            .with_skip_review(true)
            .with_agent_profile("fast-worker"),
    );

    repo.create(workflow.clone()).await.unwrap();

    let loaded = repo.get_by_id(&workflow.id).await.unwrap().unwrap();
    let behavior = loaded.columns[0].behavior.as_ref().unwrap();
    assert_eq!(behavior.skip_review, Some(true));
    assert_eq!(behavior.agent_profile, Some("fast-worker".to_string()));
}

#[tokio::test]
async fn test_from_shared_connection() {
    let db = setup_test_db();
    let shared = db.shared_conn();

    let repo1 = SqliteWorkflowRepository::from_shared(shared.clone());
    let repo2 = SqliteWorkflowRepository::from_shared(shared.clone());

    // Create via repo1
    let workflow = create_test_workflow();
    repo1.create(workflow.clone()).await.unwrap();

    // Read via repo2
    let found = repo2.get_by_id(&workflow.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== SEEDING TESTS ====================

#[tokio::test]
async fn test_seed_builtin_workflows_creates_all_builtins() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    let count = repo.seed_builtin_workflows().await.unwrap();
    assert_eq!(count, 3);

    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn test_seed_builtin_workflows_creates_default() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    repo.seed_builtin_workflows().await.unwrap();

    let default = repo.get_default().await.unwrap();
    assert!(default.is_some());
    assert_eq!(default.unwrap().name, "RalphX Default");
}

#[tokio::test]
async fn test_seed_builtin_workflows_creates_jira() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    repo.seed_builtin_workflows().await.unwrap();

    let jira_id = crate::domain::entities::WorkflowId::from_string("jira-compat");
    let jira = repo.get_by_id(&jira_id).await.unwrap();
    assert!(jira.is_some());
    assert_eq!(jira.unwrap().name, "Jira Compatible");
}

#[tokio::test]
async fn test_seed_builtin_workflows_creates_linear_with_external_sync() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    repo.seed_builtin_workflows().await.unwrap();

    let linear_id = crate::domain::entities::WorkflowId::from_string("linear-compat");
    let linear = repo.get_by_id(&linear_id).await.unwrap().unwrap();
    let sync = linear.external_sync.as_ref().unwrap();

    assert_eq!(linear.name, "Linear Compatible");
    assert_eq!(sync.provider, crate::domain::entities::SyncProvider::Linear);
    assert_eq!(
        sync.mapping
            .get("In Progress")
            .map(|mapping| mapping.column_id.as_str()),
        Some("in_progress")
    );
}

#[tokio::test]
async fn test_seed_builtin_workflows_is_idempotent() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    // Seed twice
    let count1 = repo.seed_builtin_workflows().await.unwrap();
    let count2 = repo.seed_builtin_workflows().await.unwrap();

    // First seed creates all built-ins, second creates 0.
    assert_eq!(count1, 3);
    assert_eq!(count2, 0);

    // Still only the built-in workflows.
    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn test_seed_builtin_workflows_preserves_existing() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    // Create a custom workflow
    let custom = create_test_workflow();
    repo.create(custom).await.unwrap();

    // Seed built-ins
    repo.seed_builtin_workflows().await.unwrap();

    // Should have the custom workflow plus all built-ins.
    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 4);
}

#[tokio::test]
async fn test_seed_builtin_workflows_skips_existing_builtin() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    // Manually create the default workflow
    let default = WorkflowSchema::default_ralphx();
    repo.create(default).await.unwrap();

    // Seed should only create Jira and Linear (skip default since it exists)
    let count = repo.seed_builtin_workflows().await.unwrap();
    assert_eq!(count, 2);

    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 3);
}

#[tokio::test]
async fn test_seed_builtin_workflows_preserves_default_and_jira_ids() {
    let db = setup_test_db();
    let repo = SqliteWorkflowRepository::from_shared(db.shared_conn());

    repo.seed_builtin_workflows().await.unwrap();

    let default_id = crate::domain::entities::WorkflowId::from_string("ralphx-default");
    let jira_id = crate::domain::entities::WorkflowId::from_string("jira-compat");
    let linear_id = crate::domain::entities::WorkflowId::from_string("linear-compat");

    assert!(repo.get_by_id(&default_id).await.unwrap().is_some());
    assert!(repo.get_by_id(&jira_id).await.unwrap().is_some());
    assert!(repo.get_by_id(&linear_id).await.unwrap().is_some());
}
