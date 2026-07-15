// Integration test: Workflow CRUD and column rendering
//
// Tests end-to-end workflow operations:
// - Create custom workflow with 5 columns
// - Set as default workflow
// - Verify TaskBoard renders correct columns
// - Delete workflow and verify fallback to default
//
// Both memory and SQLite repositories are tested to ensure consistent behavior.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    ColumnBehavior, InternalStatus, WorkflowColumn, WorkflowSchema,
};
use ralphx_lib::infrastructure::sqlite::SqliteWorkflowRepository;
use ralphx_lib::testing::SqliteStateFixture;

// ============================================================================
// Test Setup Helpers
// ============================================================================

/// Helper to create AppState with memory repositories
fn create_memory_state() -> AppState {
    AppState::new_test()
}

/// Helper to create AppState with SQLite repositories (file-backed temp database)
fn create_sqlite_state() -> SqliteStateFixture {
    SqliteStateFixture::new("workflow-integration", |db, state| {
        state.workflow_repo = Arc::new(SqliteWorkflowRepository::from_shared(db.shared_conn()));
    })
}

/// Create a custom workflow with 5 columns for testing
fn create_custom_5_column_workflow() -> WorkflowSchema {
    WorkflowSchema::new(
        "Custom 5-Column Workflow",
        vec![
            WorkflowColumn::new("ideas", "Ideas", InternalStatus::Backlog).with_color("#8B5CF6"),
            WorkflowColumn::new("selected", "Selected", InternalStatus::Ready)
                .with_color("#3B82F6"),
            WorkflowColumn::new("in_dev", "In Development", InternalStatus::Executing)
                .with_color("#F59E0B")
                .with_behavior(ColumnBehavior::new().with_agent_profile("fast-worker")),
            WorkflowColumn::new("testing", "Testing", InternalStatus::PendingReview)
                .with_color("#10B981"),
            WorkflowColumn::new("shipped", "Shipped", InternalStatus::Approved)
                .with_color("#6366F1"),
        ],
    )
    .with_description("A custom 5-column development workflow")
}

// ============================================================================
// Shared Test Logic (works with any repository implementation)
// ============================================================================

/// Test 1: Create custom workflow with 5 columns
async fn test_create_custom_workflow(state: &AppState) {
    let workflow = create_custom_5_column_workflow();
    let workflow_id = workflow.id.clone();

    // Create the workflow
    let created = state.workflow_repo.create(workflow).await.unwrap();

    // Verify workflow was created with correct attributes
    assert_eq!(created.name, "Custom 5-Column Workflow");
    assert_eq!(created.columns.len(), 5);
    assert_eq!(
        created.description,
        Some("A custom 5-column development workflow".to_string())
    );
    assert!(!created.is_default);

    // Verify we can retrieve it
    let found = state.workflow_repo.get_by_id(&workflow_id).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();

    // Verify column details are preserved
    assert_eq!(found.columns[0].id, "ideas");
    assert_eq!(found.columns[0].name, "Ideas");
    assert_eq!(found.columns[0].maps_to, InternalStatus::Backlog);
    assert_eq!(found.columns[0].color, Some("#8B5CF6".to_string()));

    assert_eq!(found.columns[2].id, "in_dev");
    assert_eq!(found.columns[2].maps_to, InternalStatus::Executing);
    let behavior = found.columns[2].behavior.as_ref().unwrap();
    assert_eq!(behavior.agent_profile, Some("fast-worker".to_string()));

    assert_eq!(found.columns[4].id, "shipped");
    assert_eq!(found.columns[4].maps_to, InternalStatus::Approved);
}

/// Test 2: Set as default workflow
async fn test_set_default_workflow(state: &AppState) {
    // Create the default RalphX workflow first
    let default_workflow = WorkflowSchema::default_ralphx();
    state
        .workflow_repo
        .create(default_workflow.clone())
        .await
        .unwrap();

    // Create our custom workflow
    let custom = create_custom_5_column_workflow();
    let custom_id = custom.id.clone();
    state.workflow_repo.create(custom).await.unwrap();

    // Verify RalphX default is currently default
    let current_default = state.workflow_repo.get_default().await.unwrap();
    assert!(current_default.is_some());
    assert_eq!(current_default.unwrap().name, "RalphX Default");

    // Set custom as default
    state.workflow_repo.set_default(&custom_id).await.unwrap();

    // Verify custom is now default
    let new_default = state.workflow_repo.get_default().await.unwrap();
    assert!(new_default.is_some());
    let new_default = new_default.unwrap();
    assert_eq!(new_default.name, "Custom 5-Column Workflow");
    assert!(new_default.is_default);

    // Verify old default is no longer default
    let old_default = state
        .workflow_repo
        .get_by_id(&default_workflow.id)
        .await
        .unwrap();
    assert!(old_default.is_some());
    assert!(!old_default.unwrap().is_default);
}

/// Test 3: Verify TaskBoard gets correct columns from workflow
async fn test_get_columns_for_workflow(state: &AppState) {
    // Create and set a custom workflow as default
    let custom = create_custom_5_column_workflow();
    let custom_id = custom.id.clone();
    state.workflow_repo.create(custom).await.unwrap();
    state.workflow_repo.set_default(&custom_id).await.unwrap();

    // Get the default workflow
    let default = state.workflow_repo.get_default().await.unwrap();
    assert!(default.is_some());
    let default = default.unwrap();

    // Verify column count matches
    assert_eq!(default.columns.len(), 5);

    // Verify columns can be used for TaskBoard rendering
    // (simulating what TaskBoardWithHeader does)
    let column_ids: Vec<&str> = default.columns.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        column_ids,
        vec!["ideas", "selected", "in_dev", "testing", "shipped"]
    );

    // Verify column names for display
    let column_names: Vec<&str> = default.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        column_names,
        vec!["Ideas", "Selected", "In Development", "Testing", "Shipped"]
    );

    // Verify internal status mappings (used for side effects)
    let column_statuses: Vec<InternalStatus> = default.columns.iter().map(|c| c.maps_to).collect();
    assert_eq!(
        column_statuses,
        vec![
            InternalStatus::Backlog,
            InternalStatus::Ready,
            InternalStatus::Executing,
            InternalStatus::PendingReview,
            InternalStatus::Approved,
        ]
    );
}

/// Test 4: Delete workflow and verify fallback to default
async fn test_delete_workflow_fallback(state: &AppState) {
    // Create two workflows: default RalphX and custom
    let default_workflow = WorkflowSchema::default_ralphx();
    let default_id = default_workflow.id.clone();
    state.workflow_repo.create(default_workflow).await.unwrap();

    let custom = create_custom_5_column_workflow();
    let custom_id = custom.id.clone();
    state.workflow_repo.create(custom).await.unwrap();

    // Set custom as default
    state.workflow_repo.set_default(&custom_id).await.unwrap();

    // Verify custom is default
    let current_default = state.workflow_repo.get_default().await.unwrap();
    assert!(current_default.is_some());
    assert_eq!(current_default.unwrap().id, custom_id);

    // Delete the custom workflow
    state.workflow_repo.delete(&custom_id).await.unwrap();

    // Verify custom is deleted
    let deleted = state.workflow_repo.get_by_id(&custom_id).await.unwrap();
    assert!(deleted.is_none());

    // Since custom was default and is now deleted, there's no default
    // The application should fall back to RalphX default when no default exists
    let remaining_default = state.workflow_repo.get_default().await.unwrap();
    // After deletion, no workflow is default (the app handles fallback in WorkflowService)
    assert!(remaining_default.is_none());

    // Verify we can still get the RalphX workflow by ID
    let ralphx = state.workflow_repo.get_by_id(&default_id).await.unwrap();
    assert!(ralphx.is_some());
    assert_eq!(ralphx.unwrap().name, "RalphX Default");

    // Set RalphX back as default (mimicking what the app would do)
    state.workflow_repo.set_default(&default_id).await.unwrap();

    // Verify RalphX is now default
    let new_default = state.workflow_repo.get_default().await.unwrap();
    assert!(new_default.is_some());
    assert_eq!(new_default.unwrap().name, "RalphX Default");
}

/// Test 5: Complete workflow CRUD cycle
async fn test_workflow_crud_cycle(state: &AppState) {
    // CREATE
    let workflow = create_custom_5_column_workflow();
    let workflow_id = workflow.id.clone();
    let created = state.workflow_repo.create(workflow).await.unwrap();
    assert_eq!(created.name, "Custom 5-Column Workflow");

    // READ
    let found = state.workflow_repo.get_by_id(&workflow_id).await.unwrap();
    assert!(found.is_some());
    let mut workflow = found.unwrap();

    // UPDATE - change name and add a column
    workflow.name = "Updated Workflow Name".to_string();
    workflow.columns.push(
        WorkflowColumn::new("cancelled", "Cancelled", InternalStatus::Cancelled)
            .with_color("#6B7280"),
    );
    state.workflow_repo.update(&workflow).await.unwrap();

    // Verify update
    let updated = state
        .workflow_repo
        .get_by_id(&workflow_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.name, "Updated Workflow Name");
    assert_eq!(updated.columns.len(), 6);
    assert_eq!(updated.columns[5].id, "cancelled");

    // DELETE
    state.workflow_repo.delete(&workflow_id).await.unwrap();

    // Verify deletion
    let deleted = state.workflow_repo.get_by_id(&workflow_id).await.unwrap();
    assert!(deleted.is_none());
}

/// Test 6: Multiple workflows coexist
async fn test_multiple_workflows(state: &AppState) {
    // Create multiple workflows
    let default = WorkflowSchema::default_ralphx();
    let jira = WorkflowSchema::jira_compatible();
    let custom = create_custom_5_column_workflow();

    state.workflow_repo.create(default.clone()).await.unwrap();
    state.workflow_repo.create(jira.clone()).await.unwrap();
    state.workflow_repo.create(custom.clone()).await.unwrap();

    // Get all workflows
    let all = state.workflow_repo.get_all().await.unwrap();
    assert_eq!(all.len(), 3);

    // Verify each has correct column count
    let default_found = all.iter().find(|w| w.name == "RalphX Default").unwrap();
    assert_eq!(default_found.columns.len(), 5);

    let jira_found = all.iter().find(|w| w.name == "Jira Compatible").unwrap();
    assert_eq!(jira_found.columns.len(), 5);

    let custom_found = all
        .iter()
        .find(|w| w.name == "Custom 5-Column Workflow")
        .unwrap();
    assert_eq!(custom_found.columns.len(), 5);
}

/// Test 7: Column behavior preservation
async fn test_column_behavior_preservation(state: &AppState) {
    let workflow = WorkflowSchema::new(
        "Behavior Test Workflow",
        vec![
            WorkflowColumn::new("backlog", "Backlog", InternalStatus::Backlog),
            WorkflowColumn::new("fast", "Fast Track", InternalStatus::Executing).with_behavior(
                ColumnBehavior::new()
                    .with_skip_review(true)
                    .with_auto_advance(true)
                    .with_agent_profile("speed-worker"),
            ),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    );
    let id = workflow.id.clone();

    state.workflow_repo.create(workflow).await.unwrap();

    // Retrieve and verify behavior
    let found = state.workflow_repo.get_by_id(&id).await.unwrap().unwrap();
    let fast_column = &found.columns[1];

    assert!(fast_column.behavior.is_some());
    let behavior = fast_column.behavior.as_ref().unwrap();
    assert_eq!(behavior.skip_review, Some(true));
    assert_eq!(behavior.auto_advance, Some(true));
    assert_eq!(behavior.agent_profile, Some("speed-worker".to_string()));
}

// ============================================================================
// Memory Repository Tests
// ============================================================================

#[tokio::test]
async fn test_create_custom_workflow_with_memory() {
    let state = create_memory_state();
    test_create_custom_workflow(&state).await;
}

#[tokio::test]
async fn test_set_default_workflow_with_memory() {
    let state = create_memory_state();
    test_set_default_workflow(&state).await;
}

#[tokio::test]
async fn test_get_columns_for_workflow_with_memory() {
    let state = create_memory_state();
    test_get_columns_for_workflow(&state).await;
}

#[tokio::test]
async fn test_delete_workflow_fallback_with_memory() {
    let state = create_memory_state();
    test_delete_workflow_fallback(&state).await;
}

#[tokio::test]
async fn test_workflow_crud_cycle_with_memory() {
    let state = create_memory_state();
    test_workflow_crud_cycle(&state).await;
}

#[tokio::test]
async fn test_multiple_workflows_with_memory() {
    let state = create_memory_state();
    test_multiple_workflows(&state).await;
}

#[tokio::test]
async fn test_column_behavior_preservation_with_memory() {
    let state = create_memory_state();
    test_column_behavior_preservation(&state).await;
}

// ============================================================================
// SQLite Repository Tests
// ============================================================================

#[tokio::test]
async fn test_create_custom_workflow_with_sqlite() {
    let state = create_sqlite_state();
    test_create_custom_workflow(&state).await;
}

#[tokio::test]
async fn test_set_default_workflow_with_sqlite() {
    let state = create_sqlite_state();
    test_set_default_workflow(&state).await;
}

#[tokio::test]
async fn test_get_columns_for_workflow_with_sqlite() {
    let state = create_sqlite_state();
    test_get_columns_for_workflow(&state).await;
}

#[tokio::test]
async fn test_delete_workflow_fallback_with_sqlite() {
    let state = create_sqlite_state();
    test_delete_workflow_fallback(&state).await;
}

#[tokio::test]
async fn test_workflow_crud_cycle_with_sqlite() {
    let state = create_sqlite_state();
    test_workflow_crud_cycle(&state).await;
}

#[tokio::test]
async fn test_multiple_workflows_with_sqlite() {
    let state = create_sqlite_state();
    test_multiple_workflows(&state).await;
}

#[tokio::test]
async fn test_column_behavior_preservation_with_sqlite() {
    let state = create_sqlite_state();
    test_column_behavior_preservation(&state).await;
}
