// Integration test: Methodology activation and deactivation
//
// Tests end-to-end methodology operations:
// - Activate BMAD methodology
// - Verify workflow columns match BMAD definition
// - Verify agent profiles loaded
// - Deactivate methodology returns to default
//
// Both memory and SQLite repositories are tested to ensure consistent behavior.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    InternalStatus, MethodologyExtension, MethodologyId, MethodologyPhase, WorkflowColumn,
    WorkflowSchema,
};
use ralphx_lib::infrastructure::sqlite::{
    SqliteMethodologyRepository, SqliteWorkflowRepository,
};
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
    SqliteStateFixture::new("methodology-integration", |db, state| {
        let shared_conn = db.shared_conn();
        state.methodology_repo = Arc::new(SqliteMethodologyRepository::from_shared(Arc::clone(
            &shared_conn,
        )));
        state.workflow_repo = Arc::new(SqliteWorkflowRepository::from_shared(shared_conn));
    })
}

/// Create the BMAD methodology for testing
fn create_bmad_methodology() -> MethodologyExtension {
    // BMAD - Breakthrough Method for Agile AI-Driven Development
    let workflow = WorkflowSchema::new(
        "BMAD Method",
        vec![
            // Phase 1: Analysis
            WorkflowColumn::new("brainstorm", "Brainstorm", InternalStatus::Backlog),
            WorkflowColumn::new("research", "Research", InternalStatus::Executing),
            // Phase 2: Planning
            WorkflowColumn::new("prd-draft", "PRD Draft", InternalStatus::Executing),
            WorkflowColumn::new("prd-review", "PRD Review", InternalStatus::PendingReview),
            WorkflowColumn::new("ux-design", "UX Design", InternalStatus::Executing),
            // Phase 3: Solutioning
            WorkflowColumn::new("architecture", "Architecture", InternalStatus::Executing),
            WorkflowColumn::new("stories", "Stories", InternalStatus::Ready),
            // Phase 4: Implementation
            WorkflowColumn::new("sprint", "Sprint", InternalStatus::Executing),
            WorkflowColumn::new("code-review", "Code Review", InternalStatus::PendingReview),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    )
    .with_description("Breakthrough Method for Agile AI-Driven Development workflow");

    MethodologyExtension::new("BMAD Method", workflow)
        .with_description("Breakthrough Method for Agile AI-Driven Development")
        .with_agent_profile("bmad-analyst")
        .with_agent_profile("bmad-pm")
        .with_agent_profile("bmad-architect")
        .with_agent_profile("bmad-ux")
        .with_agent_profile("bmad-developer")
        .with_agent_profile("bmad-scrum-master")
        .with_agent_profile("bmad-tea")
        .with_agent_profile("bmad-tech-writer")
        .with_phase(MethodologyPhase::new("analysis", "Analysis", 0))
        .with_phase(MethodologyPhase::new("planning", "Planning", 1))
        .with_phase(MethodologyPhase::new("solutioning", "Solutioning", 2))
        .with_phase(MethodologyPhase::new("implementation", "Implementation", 3))
        .with_skill("skills/prd-creation")
        .with_skill("skills/architecture-design")
        .with_skill("skills/ux-review")
        .with_skill("skills/story-writing")
}

/// Create the GSD methodology for testing
fn create_gsd_methodology() -> MethodologyExtension {
    // GSD - Get Shit Done
    let workflow = WorkflowSchema::new(
        "GSD Method",
        vec![
            WorkflowColumn::new("initialize", "Initialize", InternalStatus::Backlog),
            WorkflowColumn::new("discuss", "Discuss", InternalStatus::Blocked),
            WorkflowColumn::new("research", "Research", InternalStatus::Executing),
            WorkflowColumn::new("planning", "Planning", InternalStatus::Executing),
            WorkflowColumn::new("plan-check", "Plan Check", InternalStatus::PendingReview),
            WorkflowColumn::new("queued", "Queued", InternalStatus::Ready),
            WorkflowColumn::new("executing", "Executing", InternalStatus::Executing),
            WorkflowColumn::new("checkpoint", "Checkpoint", InternalStatus::Blocked),
            WorkflowColumn::new("verifying", "Verifying", InternalStatus::PendingReview),
            WorkflowColumn::new("debugging", "Debugging", InternalStatus::RevisionNeeded),
            WorkflowColumn::new("done", "Done", InternalStatus::Approved),
        ],
    )
    .with_description("Get Shit Done methodology workflow");

    MethodologyExtension::new("GSD Method", workflow)
        .with_description("Get Shit Done - Spec-driven development with wave-based parallelization")
        .with_agent_profile("gsd-project-researcher")
        .with_agent_profile("gsd-phase-researcher")
        .with_agent_profile("gsd-planner")
        .with_agent_profile("gsd-plan-checker")
        .with_agent_profile("gsd-executor")
        .with_agent_profile("gsd-verifier")
        .with_agent_profile("gsd-debugger")
        .with_agent_profile("gsd-orchestrator")
        .with_agent_profile("gsd-monitor")
        .with_agent_profile("gsd-qa")
        .with_agent_profile("gsd-docs")
        .with_phase(MethodologyPhase::new("initialize", "Initialize", 0))
        .with_phase(MethodologyPhase::new("plan", "Plan", 1))
        .with_phase(MethodologyPhase::new("execute", "Execute", 2))
        .with_phase(MethodologyPhase::new("verify", "Verify", 3))
}

// ============================================================================
// Shared Test Logic (works with any repository implementation)
// ============================================================================

/// Test 1: Activate BMAD methodology
async fn test_activate_bmad_methodology(state: &AppState) {
    // Create BMAD methodology
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();

    state.methodology_repo.create(bmad).await.unwrap();

    // Initially no active methodology
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_none());

    // Activate BMAD
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    // Verify BMAD is now active
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_some());
    let active = active.unwrap();
    assert_eq!(active.id, bmad_id);
    assert_eq!(active.name, "BMAD Method");
    assert!(active.is_active);
}

/// Test 2: Verify BMAD workflow columns match definition
async fn test_verify_bmad_workflow_columns(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad.clone()).await.unwrap();
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    // Get active methodology
    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // Verify workflow has 10 columns (as defined in BMAD)
    assert_eq!(active.workflow.columns.len(), 10);

    // Verify column names and mappings
    assert_eq!(active.workflow.columns[0].id, "brainstorm");
    assert_eq!(active.workflow.columns[0].name, "Brainstorm");
    assert_eq!(active.workflow.columns[0].maps_to, InternalStatus::Backlog);

    assert_eq!(active.workflow.columns[1].id, "research");
    assert_eq!(
        active.workflow.columns[1].maps_to,
        InternalStatus::Executing
    );

    assert_eq!(active.workflow.columns[3].id, "prd-review");
    assert_eq!(
        active.workflow.columns[3].maps_to,
        InternalStatus::PendingReview
    );

    assert_eq!(active.workflow.columns[9].id, "done");
    assert_eq!(active.workflow.columns[9].maps_to, InternalStatus::Approved);
}

/// Test 3: Verify BMAD agent profiles loaded
async fn test_verify_bmad_agent_profiles(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // Verify 8 agent profiles
    assert_eq!(active.agent_profiles.len(), 8);
    assert!(active.agent_profiles.contains(&"bmad-analyst".to_string()));
    assert!(active.agent_profiles.contains(&"bmad-pm".to_string()));
    assert!(active
        .agent_profiles
        .contains(&"bmad-architect".to_string()));
    assert!(active.agent_profiles.contains(&"bmad-ux".to_string()));
    assert!(active
        .agent_profiles
        .contains(&"bmad-developer".to_string()));
    assert!(active
        .agent_profiles
        .contains(&"bmad-scrum-master".to_string()));
    assert!(active.agent_profiles.contains(&"bmad-tea".to_string()));
    assert!(active
        .agent_profiles
        .contains(&"bmad-tech-writer".to_string()));
}

/// Test 4: Verify BMAD phases
async fn test_verify_bmad_phases(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // Verify 4 phases in correct order
    assert_eq!(active.phases.len(), 4);
    assert_eq!(active.phases[0].id, "analysis");
    assert_eq!(active.phases[0].order, 0);
    assert_eq!(active.phases[1].id, "planning");
    assert_eq!(active.phases[1].order, 1);
    assert_eq!(active.phases[2].id, "solutioning");
    assert_eq!(active.phases[2].order, 2);
    assert_eq!(active.phases[3].id, "implementation");
    assert_eq!(active.phases[3].order, 3);
}

/// Test 5: Verify BMAD skills
async fn test_verify_bmad_skills(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // Verify skills
    assert_eq!(active.skills.len(), 4);
    assert!(active.skills.contains(&"skills/prd-creation".to_string()));
    assert!(active
        .skills
        .contains(&"skills/architecture-design".to_string()));
    assert!(active.skills.contains(&"skills/ux-review".to_string()));
    assert!(active.skills.contains(&"skills/story-writing".to_string()));
}

/// Test 6: Deactivate methodology returns to no active
async fn test_deactivate_methodology(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    // Verify active before deactivation
    assert!(state.methodology_repo.get_active().await.unwrap().is_some());

    // Deactivate
    state.methodology_repo.deactivate(&bmad_id).await.unwrap();

    // Verify no active methodology
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_none());

    // Verify methodology still exists but is_active = false
    let methodology = state
        .methodology_repo
        .get_by_id(&bmad_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!methodology.is_active);
}

/// Test 7: Switch from BMAD to GSD
async fn test_switch_methodology(state: &AppState) {
    // Create both methodologies
    let bmad = create_bmad_methodology();
    let gsd = create_gsd_methodology();
    let bmad_id = bmad.id.clone();
    let gsd_id = gsd.id.clone();

    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.create(gsd).await.unwrap();

    // Activate BMAD first
    state.methodology_repo.activate(&bmad_id).await.unwrap();
    let active = state.methodology_repo.get_active().await.unwrap().unwrap();
    assert_eq!(active.name, "BMAD Method");

    // Deactivate BMAD and activate GSD
    state.methodology_repo.deactivate(&bmad_id).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    // Verify GSD is now active
    let active = state.methodology_repo.get_active().await.unwrap().unwrap();
    assert_eq!(active.name, "GSD Method");
    assert_eq!(active.workflow.columns.len(), 11); // GSD has 11 columns

    // Verify BMAD is no longer active
    let bmad_methodology = state
        .methodology_repo
        .get_by_id(&bmad_id)
        .await
        .unwrap()
        .unwrap();
    assert!(!bmad_methodology.is_active);
}

/// Test 8: Verify GSD workflow columns
async fn test_verify_gsd_workflow_columns(state: &AppState) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // GSD has 11 columns
    assert_eq!(active.workflow.columns.len(), 11);

    // Verify key columns
    assert_eq!(active.workflow.columns[0].id, "initialize");
    assert_eq!(active.workflow.columns[1].id, "discuss");
    assert_eq!(active.workflow.columns[1].maps_to, InternalStatus::Blocked);
    assert_eq!(active.workflow.columns[7].id, "checkpoint");
    assert_eq!(active.workflow.columns[7].maps_to, InternalStatus::Blocked);
    assert_eq!(active.workflow.columns[9].id, "debugging");
    assert_eq!(
        active.workflow.columns[9].maps_to,
        InternalStatus::RevisionNeeded
    );
}

/// Test 9: Verify GSD agent profiles (11 agents)
async fn test_verify_gsd_agent_profiles(state: &AppState) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // GSD has 11 agent profiles
    assert_eq!(active.agent_profiles.len(), 11);
    assert!(active
        .agent_profiles
        .contains(&"gsd-project-researcher".to_string()));
    assert!(active.agent_profiles.contains(&"gsd-executor".to_string()));
    assert!(active.agent_profiles.contains(&"gsd-verifier".to_string()));
    assert!(active.agent_profiles.contains(&"gsd-debugger".to_string()));
}

/// Test 10: Get all methodologies
async fn test_get_all_methodologies(state: &AppState) {
    let bmad = create_bmad_methodology();
    let gsd = create_gsd_methodology();

    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.create(gsd).await.unwrap();

    let all = state.methodology_repo.get_all().await.unwrap();
    assert_eq!(all.len(), 2);

    let names: Vec<&str> = all.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"BMAD Method"));
    assert!(names.contains(&"GSD Method"));
}

/// Test 11: Methodology exists check
async fn test_methodology_exists(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad).await.unwrap();

    assert!(state.methodology_repo.exists(&bmad_id).await.unwrap());
    assert!(!state
        .methodology_repo
        .exists(&MethodologyId::from_string("nonexistent"))
        .await
        .unwrap());
}

/// Test 12: Delete methodology
async fn test_delete_methodology(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad).await.unwrap();

    assert!(state.methodology_repo.exists(&bmad_id).await.unwrap());

    state.methodology_repo.delete(&bmad_id).await.unwrap();

    assert!(!state.methodology_repo.exists(&bmad_id).await.unwrap());
    assert!(state
        .methodology_repo
        .get_by_id(&bmad_id)
        .await
        .unwrap()
        .is_none());
}

/// Test 13: Update methodology
async fn test_update_methodology(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad.clone()).await.unwrap();

    // Update the description
    let mut updated = bmad;
    updated.description = Some("Updated BMAD description".to_string());
    state.methodology_repo.update(&updated).await.unwrap();

    let found = state
        .methodology_repo
        .get_by_id(&bmad_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        found.description,
        Some("Updated BMAD description".to_string())
    );
}

/// Test 14: Only one methodology can be active at a time
async fn test_single_active_methodology(state: &AppState) {
    let bmad = create_bmad_methodology();
    let gsd = create_gsd_methodology();
    let bmad_id = bmad.id.clone();
    let gsd_id = gsd.id.clone();

    state.methodology_repo.create(bmad).await.unwrap();
    state.methodology_repo.create(gsd).await.unwrap();

    // Activate BMAD
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    // Try to activate GSD without deactivating BMAD first
    // (Implementation note: in actual service, this would auto-deactivate BMAD)
    // For repository level, we manually deactivate first
    state.methodology_repo.deactivate(&bmad_id).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    // Only GSD should be active
    let all = state.methodology_repo.get_all().await.unwrap();
    let active_count = all.iter().filter(|m| m.is_active).count();
    assert_eq!(active_count, 1);

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();
    assert_eq!(active.id, gsd_id);
}

/// Test 15: Activation result contains workflow and agent profiles
async fn test_activation_returns_components(state: &AppState) {
    let bmad = create_bmad_methodology();
    let bmad_id = bmad.id.clone();
    state.methodology_repo.create(bmad.clone()).await.unwrap();
    state.methodology_repo.activate(&bmad_id).await.unwrap();

    // Get the activated methodology
    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // Verify all components are accessible
    assert_eq!(active.workflow.name, "BMAD Method");
    assert_eq!(active.workflow.columns.len(), 10);
    assert_eq!(active.agent_profiles.len(), 8);
    assert_eq!(active.skills.len(), 4);
    assert_eq!(active.phases.len(), 4);
}

// ============================================================================
// Memory Repository Tests
// ============================================================================

#[tokio::test]
async fn memory_test_activate_bmad_methodology() {
    let state = create_memory_state();
    test_activate_bmad_methodology(&state).await;
}

#[tokio::test]
async fn memory_test_verify_bmad_workflow_columns() {
    let state = create_memory_state();
    test_verify_bmad_workflow_columns(&state).await;
}

#[tokio::test]
async fn memory_test_verify_bmad_agent_profiles() {
    let state = create_memory_state();
    test_verify_bmad_agent_profiles(&state).await;
}

#[tokio::test]
async fn memory_test_verify_bmad_phases() {
    let state = create_memory_state();
    test_verify_bmad_phases(&state).await;
}

#[tokio::test]
async fn memory_test_verify_bmad_skills() {
    let state = create_memory_state();
    test_verify_bmad_skills(&state).await;
}

#[tokio::test]
async fn memory_test_deactivate_methodology() {
    let state = create_memory_state();
    test_deactivate_methodology(&state).await;
}

#[tokio::test]
async fn memory_test_switch_methodology() {
    let state = create_memory_state();
    test_switch_methodology(&state).await;
}

#[tokio::test]
async fn memory_test_verify_gsd_workflow_columns() {
    let state = create_memory_state();
    test_verify_gsd_workflow_columns(&state).await;
}

#[tokio::test]
async fn memory_test_verify_gsd_agent_profiles() {
    let state = create_memory_state();
    test_verify_gsd_agent_profiles(&state).await;
}

#[tokio::test]
async fn memory_test_get_all_methodologies() {
    let state = create_memory_state();
    test_get_all_methodologies(&state).await;
}

#[tokio::test]
async fn memory_test_methodology_exists() {
    let state = create_memory_state();
    test_methodology_exists(&state).await;
}

#[tokio::test]
async fn memory_test_delete_methodology() {
    let state = create_memory_state();
    test_delete_methodology(&state).await;
}

#[tokio::test]
async fn memory_test_update_methodology() {
    let state = create_memory_state();
    test_update_methodology(&state).await;
}

#[tokio::test]
async fn memory_test_single_active_methodology() {
    let state = create_memory_state();
    test_single_active_methodology(&state).await;
}

#[tokio::test]
async fn memory_test_activation_returns_components() {
    let state = create_memory_state();
    test_activation_returns_components(&state).await;
}

// ============================================================================
// SQLite Repository Tests
// ============================================================================

#[tokio::test]
async fn sqlite_test_activate_bmad_methodology() {
    let state = create_sqlite_state();
    test_activate_bmad_methodology(&state).await;
}

#[tokio::test]
async fn sqlite_test_verify_bmad_workflow_columns() {
    let state = create_sqlite_state();
    test_verify_bmad_workflow_columns(&state).await;
}

#[tokio::test]
async fn sqlite_test_verify_bmad_agent_profiles() {
    let state = create_sqlite_state();
    test_verify_bmad_agent_profiles(&state).await;
}

#[tokio::test]
async fn sqlite_test_verify_bmad_phases() {
    let state = create_sqlite_state();
    test_verify_bmad_phases(&state).await;
}

#[tokio::test]
async fn sqlite_test_verify_bmad_skills() {
    let state = create_sqlite_state();
    test_verify_bmad_skills(&state).await;
}

#[tokio::test]
async fn sqlite_test_deactivate_methodology() {
    let state = create_sqlite_state();
    test_deactivate_methodology(&state).await;
}

#[tokio::test]
async fn sqlite_test_switch_methodology() {
    let state = create_sqlite_state();
    test_switch_methodology(&state).await;
}

#[tokio::test]
async fn sqlite_test_verify_gsd_workflow_columns() {
    let state = create_sqlite_state();
    test_verify_gsd_workflow_columns(&state).await;
}

#[tokio::test]
async fn sqlite_test_verify_gsd_agent_profiles() {
    let state = create_sqlite_state();
    test_verify_gsd_agent_profiles(&state).await;
}

#[tokio::test]
async fn sqlite_test_get_all_methodologies() {
    let state = create_sqlite_state();
    test_get_all_methodologies(&state).await;
}

#[tokio::test]
async fn sqlite_test_methodology_exists() {
    let state = create_sqlite_state();
    test_methodology_exists(&state).await;
}

#[tokio::test]
async fn sqlite_test_delete_methodology() {
    let state = create_sqlite_state();
    test_delete_methodology(&state).await;
}

#[tokio::test]
async fn sqlite_test_update_methodology() {
    let state = create_sqlite_state();
    test_update_methodology(&state).await;
}

#[tokio::test]
async fn sqlite_test_single_active_methodology() {
    let state = create_sqlite_state();
    test_single_active_methodology(&state).await;
}

#[tokio::test]
async fn sqlite_test_activation_returns_components() {
    let state = create_sqlite_state();
    test_activation_returns_components(&state).await;
}
