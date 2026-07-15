// Integration test: GSD-specific task fields (wave, checkpoint)
//
// Tests GSD methodology features:
// - Activate GSD methodology
// - Create task with wave grouping and checkpoint_type configuration
// - Query tasks by wave for parallel execution
// - Checkpoint transitions task to blocked status
// - Wave completion verification
//
// Note: GSD wave/checkpoint configuration is stored in methodology hooks_config.
// Tasks use needs_review_point for human-in-loop checkpoints and internal_status
// for checkpoint column transitions (Blocked status).

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    ColumnBehavior, InternalStatus, MethodologyExtension, MethodologyPhase, Project, ProjectId,
    Task, WorkflowColumn, WorkflowSchema,
};
use ralphx_lib::infrastructure::sqlite::{
    SqliteMethodologyRepository, SqliteProjectRepository, SqliteTaskRepository,
    SqliteWorkflowRepository,
};
use ralphx_lib::testing::SqliteStateFixture;

// ============================================================================
// Test Setup Helpers
// ============================================================================

/// Create AppState with memory repositories
fn create_memory_state() -> AppState {
    AppState::new_test()
}

/// Create AppState with SQLite repositories (file-backed temp database)
fn create_sqlite_state() -> SqliteStateFixture {
    SqliteStateFixture::new("gsd-integration", |db, state| {
        let shared_conn = db.shared_conn();
        state.methodology_repo = Arc::new(SqliteMethodologyRepository::from_shared(
            shared_conn.clone(),
        ));
        state.workflow_repo = Arc::new(SqliteWorkflowRepository::from_shared(shared_conn.clone()));
        state.task_repo = Arc::new(SqliteTaskRepository::from_shared(shared_conn.clone()));
        state.project_repo = Arc::new(SqliteProjectRepository::from_shared(shared_conn));
    })
}

/// Create a project and return its ID (for SQLite tests that need FK constraint)
async fn create_project_for_tasks(state: &AppState) -> ProjectId {
    let project = Project::new("GSD Test Project".to_string(), "/tmp/gsd-test".to_string());
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();
    project_id
}

/// Create the full GSD methodology with 11 columns and checkpoint support
fn create_gsd_methodology() -> MethodologyExtension {
    let workflow = WorkflowSchema::new(
        "GSD (Get Shit Done)",
        vec![
            // Initialize Phase
            WorkflowColumn::new("initialize", "Initialize", InternalStatus::Backlog)
                .with_color("#4ade80")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-project-researcher")),
            // Plan Phase
            WorkflowColumn::new("discuss", "Discuss", InternalStatus::Blocked)
                .with_color("#facc15")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-orchestrator")),
            WorkflowColumn::new("research", "Research", InternalStatus::Executing)
                .with_color("#60a5fa")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-phase-researcher")),
            WorkflowColumn::new("planning", "Planning", InternalStatus::Executing)
                .with_color("#a78bfa")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-planner")),
            WorkflowColumn::new("plan-check", "Plan Check", InternalStatus::PendingReview)
                .with_color("#f472b6")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-plan-checker")),
            // Execute Phase (wave-based)
            WorkflowColumn::new("queued", "Queued", InternalStatus::Ready).with_color("#6366f1"),
            WorkflowColumn::new("executing", "Executing", InternalStatus::Executing)
                .with_color("#10b981")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-executor")),
            // Checkpoint column - maps to Blocked for human intervention
            WorkflowColumn::new("checkpoint", "Checkpoint", InternalStatus::Blocked)
                .with_color("#ef4444"),
            // Verify Phase
            WorkflowColumn::new("verifying", "Verifying", InternalStatus::PendingReview)
                .with_color("#14b8a6")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-verifier")),
            WorkflowColumn::new("debugging", "Debugging", InternalStatus::RevisionNeeded)
                .with_color("#f97316")
                .with_behavior(ColumnBehavior::new().with_agent_profile("gsd-debugger")),
            // Complete
            WorkflowColumn::new("done", "Done", InternalStatus::Approved).with_color("#22c55e"),
        ],
    )
    .with_description("Spec-driven development with wave-based parallelization");

    MethodologyExtension::new("GSD (Get Shit Done)", workflow)
        .with_description(
            "Spec-driven development with wave-based parallelization. Features checkpoint \
             protocols (human-verify, decision, human-action) and goal-backward verification \
             with must-haves derived from phase goals.",
        )
        // 11 agent profiles
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
        // 4 phases
        .with_phase(
            MethodologyPhase::new("initialize", "Initialize", 0)
                .with_description("Project research and initialization")
                .with_agent_profile("gsd-project-researcher")
                .with_column("initialize"),
        )
        .with_phase(
            MethodologyPhase::new("plan", "Plan", 1)
                .with_description("Research, planning, and plan verification")
                .with_agent_profile("gsd-phase-researcher")
                .with_agent_profile("gsd-planner")
                .with_agent_profile("gsd-plan-checker")
                .with_column("discuss")
                .with_column("research")
                .with_column("planning")
                .with_column("plan-check"),
        )
        .with_phase(
            MethodologyPhase::new("execute", "Execute", 2)
                .with_description("Wave-based parallel execution with checkpoints")
                .with_agent_profile("gsd-executor")
                .with_column("queued")
                .with_column("executing")
                .with_column("checkpoint"),
        )
        .with_phase(
            MethodologyPhase::new("verify", "Verify", 3)
                .with_description("Verification and debugging")
                .with_agent_profile("gsd-verifier")
                .with_agent_profile("gsd-debugger")
                .with_column("verifying")
                .with_column("debugging")
                .with_column("done"),
        )
}

/// Create a task with GSD wave/checkpoint configuration in description
fn create_gsd_task(
    project_id: ProjectId,
    title: &str,
    wave: u32,
    checkpoint_type: Option<&str>,
) -> Task {
    let mut task = Task::new(project_id, title.to_string());
    // Store wave and checkpoint info in description for GSD
    let wave_info = format!("wave:{}", wave);
    let checkpoint_info = checkpoint_type
        .map(|ct| format!(" checkpoint:{}", ct))
        .unwrap_or_default();
    task.set_description(Some(format!("{}{}", wave_info, checkpoint_info)));

    // Set needs_review_point for human-verify checkpoints
    if checkpoint_type == Some("human-verify") || checkpoint_type == Some("human-action") {
        task.set_needs_review_point(true);
    }
    task
}

// ============================================================================
// Shared Test Logic
// ============================================================================

/// Test 1: Activate GSD methodology and verify workflow
async fn test_activate_gsd_methodology(state: &AppState) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();

    // Create and activate GSD methodology
    state.methodology_repo.create(gsd).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    // Verify GSD is now active
    let active = state.methodology_repo.get_active().await.unwrap();
    assert!(active.is_some());
    let active = active.unwrap();
    assert_eq!(active.name, "GSD (Get Shit Done)");
    assert!(active.is_active);

    // Verify GSD workflow has 11 columns
    assert_eq!(active.workflow.columns.len(), 11);

    // Verify checkpoint column maps to Blocked
    let checkpoint_column = active
        .workflow
        .columns
        .iter()
        .find(|c| c.id == "checkpoint");
    assert!(checkpoint_column.is_some());
    assert_eq!(checkpoint_column.unwrap().maps_to, InternalStatus::Blocked);

    // Verify discuss column also maps to Blocked
    let discuss_column = active.workflow.columns.iter().find(|c| c.id == "discuss");
    assert!(discuss_column.is_some());
    assert_eq!(discuss_column.unwrap().maps_to, InternalStatus::Blocked);
}

/// Test 2: Create tasks with wave=1 and checkpoint_type
async fn test_create_tasks_with_wave_and_checkpoint(
    state: &AppState,
    project_id: Option<ProjectId>,
) {
    let project_id = project_id.unwrap_or_default();

    // Create Wave 1 tasks (parallel execution)
    let task1 = create_gsd_task(project_id.clone(), "Setup database", 1, None);
    let task2 = create_gsd_task(
        project_id.clone(),
        "Configure auth",
        1,
        Some("human-verify"),
    );
    let task3 = create_gsd_task(project_id.clone(), "API scaffolding", 1, None);

    state.task_repo.create(task1.clone()).await.unwrap();
    state.task_repo.create(task2.clone()).await.unwrap();
    state.task_repo.create(task3.clone()).await.unwrap();

    // Verify task with human-verify has needs_review_point
    let fetched = state.task_repo.get_by_id(&task2.id).await.unwrap().unwrap();
    assert!(fetched.needs_review_point);

    // Verify tasks without human checkpoint don't have needs_review_point
    let fetched1 = state.task_repo.get_by_id(&task1.id).await.unwrap().unwrap();
    assert!(!fetched1.needs_review_point);
}

/// Test 3: Query tasks by wave for parallel execution
async fn test_query_tasks_by_wave(state: &AppState, project_id: Option<ProjectId>) {
    let project_id = project_id.unwrap_or_default();

    // Create Wave 1 tasks
    let wave1_task1 = create_gsd_task(project_id.clone(), "Wave 1 Task A", 1, None);
    let wave1_task2 = create_gsd_task(project_id.clone(), "Wave 1 Task B", 1, None);
    let wave1_task3 = create_gsd_task(project_id.clone(), "Wave 1 Task C", 1, None);

    // Create Wave 2 tasks
    let wave2_task1 = create_gsd_task(project_id.clone(), "Wave 2 Task A", 2, None);
    let wave2_task2 = create_gsd_task(project_id.clone(), "Wave 2 Task B", 2, None);

    // Create Wave 3 tasks
    let wave3_task1 = create_gsd_task(project_id.clone(), "Wave 3 Final", 3, Some("human-verify"));

    state.task_repo.create(wave1_task1).await.unwrap();
    state.task_repo.create(wave1_task2).await.unwrap();
    state.task_repo.create(wave1_task3).await.unwrap();
    state.task_repo.create(wave2_task1).await.unwrap();
    state.task_repo.create(wave2_task2).await.unwrap();
    state.task_repo.create(wave3_task1).await.unwrap();

    // Query all tasks for project
    let all_tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(all_tasks.len(), 6);

    // Filter Wave 1 tasks (for parallel execution)
    let wave1_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| {
            t.description
                .as_ref()
                .map(|d| d.contains("wave:1"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(wave1_tasks.len(), 3);

    // Filter Wave 2 tasks
    let wave2_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| {
            t.description
                .as_ref()
                .map(|d| d.contains("wave:2"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(wave2_tasks.len(), 2);

    // Filter Wave 3 tasks (final wave with checkpoint)
    let wave3_tasks: Vec<_> = all_tasks
        .iter()
        .filter(|t| {
            t.description
                .as_ref()
                .map(|d| d.contains("wave:3"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(wave3_tasks.len(), 1);
    assert!(wave3_tasks[0].needs_review_point);
}

/// Test 4: Checkpoint transitions task to Blocked status
async fn test_checkpoint_transitions_to_blocked(state: &AppState, project_id: Option<ProjectId>) {
    // First activate GSD methodology
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    // Create a task in executing state
    let project_id = project_id.unwrap_or_default();
    let mut task = create_gsd_task(
        project_id.clone(),
        "Critical operation",
        1,
        Some("human-verify"),
    );
    task.internal_status = InternalStatus::Executing;

    state.task_repo.create(task.clone()).await.unwrap();

    // Transition to checkpoint (Blocked status)
    // This simulates moving the task to the "checkpoint" column
    let mut updated = state.task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    updated.internal_status = InternalStatus::Blocked;
    state.task_repo.update(&updated).await.unwrap();

    // Verify task is now blocked (waiting at checkpoint)
    let blocked = state.task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(blocked.internal_status, InternalStatus::Blocked);
    assert!(blocked.needs_review_point);
}

/// Test 5: Wave completion verification
async fn test_wave_completion(state: &AppState, project_id: Option<ProjectId>) {
    let project_id = project_id.unwrap_or_default();

    // Create Wave 1 tasks (all should complete before Wave 2 starts)
    let mut wave1_task1 = create_gsd_task(project_id.clone(), "Wave 1 - A", 1, None);
    let mut wave1_task2 = create_gsd_task(project_id.clone(), "Wave 1 - B", 1, None);
    let wave2_task1 = create_gsd_task(project_id.clone(), "Wave 2 - A", 2, None);

    wave1_task1.internal_status = InternalStatus::Executing;
    wave1_task2.internal_status = InternalStatus::Executing;

    state.task_repo.create(wave1_task1.clone()).await.unwrap();
    state.task_repo.create(wave1_task2.clone()).await.unwrap();
    state.task_repo.create(wave2_task1).await.unwrap();

    // Check wave completion status - Wave 1 not complete yet
    let all_tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    let wave1_complete = all_tasks
        .iter()
        .filter(|t| {
            t.description
                .as_ref()
                .map(|d| d.contains("wave:1"))
                .unwrap_or(false)
        })
        .all(|t| t.internal_status == InternalStatus::Approved);
    assert!(!wave1_complete);

    // Complete Wave 1 tasks
    let mut t1 = state
        .task_repo
        .get_by_id(&wave1_task1.id)
        .await
        .unwrap()
        .unwrap();
    t1.internal_status = InternalStatus::Approved;
    state.task_repo.update(&t1).await.unwrap();

    let mut t2 = state
        .task_repo
        .get_by_id(&wave1_task2.id)
        .await
        .unwrap()
        .unwrap();
    t2.internal_status = InternalStatus::Approved;
    state.task_repo.update(&t2).await.unwrap();

    // Verify Wave 1 is now complete
    let all_tasks = state.task_repo.get_by_project(&project_id).await.unwrap();
    let wave1_complete = all_tasks
        .iter()
        .filter(|t| {
            t.description
                .as_ref()
                .map(|d| d.contains("wave:1"))
                .unwrap_or(false)
        })
        .all(|t| t.internal_status == InternalStatus::Approved);
    assert!(wave1_complete);
}

/// Test 6: GSD checkpoint types
async fn test_gsd_checkpoint_types(state: &AppState, project_id: Option<ProjectId>) {
    let project_id = project_id.unwrap_or_default();

    // Create tasks with different checkpoint types
    let auto_task = create_gsd_task(project_id.clone(), "Auto checkpoint", 1, Some("auto"));
    let human_verify = create_gsd_task(project_id.clone(), "Human verify", 1, Some("human-verify"));
    let decision = create_gsd_task(project_id.clone(), "Decision point", 1, Some("decision"));
    let human_action = create_gsd_task(project_id.clone(), "Human action", 1, Some("human-action"));

    state.task_repo.create(auto_task.clone()).await.unwrap();
    state.task_repo.create(human_verify.clone()).await.unwrap();
    state.task_repo.create(decision.clone()).await.unwrap();
    state.task_repo.create(human_action.clone()).await.unwrap();

    // Verify human-verify and human-action set needs_review_point
    let fetched_verify = state
        .task_repo
        .get_by_id(&human_verify.id)
        .await
        .unwrap()
        .unwrap();
    assert!(fetched_verify.needs_review_point);

    let fetched_action = state
        .task_repo
        .get_by_id(&human_action.id)
        .await
        .unwrap()
        .unwrap();
    assert!(fetched_action.needs_review_point);

    // Auto and decision don't automatically set needs_review_point
    let fetched_auto = state
        .task_repo
        .get_by_id(&auto_task.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!fetched_auto.needs_review_point);

    let fetched_decision = state
        .task_repo
        .get_by_id(&decision.id)
        .await
        .unwrap()
        .unwrap();
    assert!(!fetched_decision.needs_review_point);
}

/// Test 7: GSD workflow column behavior (agent profiles)
async fn test_gsd_column_agent_profiles(state: &AppState) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    let active = state.methodology_repo.get_active().await.unwrap().unwrap();

    // Verify specific columns have correct agent profiles
    let executing_col = active
        .workflow
        .columns
        .iter()
        .find(|c| c.id == "executing")
        .unwrap();
    assert!(executing_col.behavior.is_some());
    assert_eq!(
        executing_col.behavior.as_ref().unwrap().agent_profile,
        Some("gsd-executor".to_string())
    );

    let verifying_col = active
        .workflow
        .columns
        .iter()
        .find(|c| c.id == "verifying")
        .unwrap();
    assert_eq!(
        verifying_col.behavior.as_ref().unwrap().agent_profile,
        Some("gsd-verifier".to_string())
    );

    let debugging_col = active
        .workflow
        .columns
        .iter()
        .find(|c| c.id == "debugging")
        .unwrap();
    assert_eq!(
        debugging_col.behavior.as_ref().unwrap().agent_profile,
        Some("gsd-debugger".to_string())
    );
}

/// Test 8: GSD phase verification
async fn test_gsd_phase_structure(state: &AppState) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();

    let methodology = state
        .methodology_repo
        .get_by_id(&gsd_id)
        .await
        .unwrap()
        .unwrap();

    // Verify 4 phases
    assert_eq!(methodology.phases.len(), 4);

    // Verify Initialize phase
    let init_phase = methodology
        .phases
        .iter()
        .find(|p| p.id == "initialize")
        .unwrap();
    assert_eq!(init_phase.order, 0);
    assert!(init_phase.column_ids.contains(&"initialize".to_string()));

    // Verify Plan phase has multiple columns
    let plan_phase = methodology.phases.iter().find(|p| p.id == "plan").unwrap();
    assert_eq!(plan_phase.order, 1);
    assert!(plan_phase.column_ids.contains(&"discuss".to_string()));
    assert!(plan_phase.column_ids.contains(&"research".to_string()));
    assert!(plan_phase.column_ids.contains(&"planning".to_string()));
    assert!(plan_phase.column_ids.contains(&"plan-check".to_string()));

    // Verify Execute phase has checkpoint column
    let exec_phase = methodology
        .phases
        .iter()
        .find(|p| p.id == "execute")
        .unwrap();
    assert_eq!(exec_phase.order, 2);
    assert!(exec_phase.column_ids.contains(&"checkpoint".to_string()));

    // Verify Verify phase
    let verify_phase = methodology
        .phases
        .iter()
        .find(|p| p.id == "verify")
        .unwrap();
    assert_eq!(verify_phase.order, 3);
    assert!(verify_phase.column_ids.contains(&"done".to_string()));
}

/// Test 9: GSD 11 agent profiles
async fn test_gsd_agent_profiles(state: &AppState) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();

    let methodology = state
        .methodology_repo
        .get_by_id(&gsd_id)
        .await
        .unwrap()
        .unwrap();

    // Verify 11 agent profiles
    assert_eq!(methodology.agent_profiles.len(), 11);

    // Verify key agents exist
    assert!(methodology
        .agent_profiles
        .contains(&"gsd-executor".to_string()));
    assert!(methodology
        .agent_profiles
        .contains(&"gsd-verifier".to_string()));
    assert!(methodology
        .agent_profiles
        .contains(&"gsd-planner".to_string()));
    assert!(methodology
        .agent_profiles
        .contains(&"gsd-debugger".to_string()));
    assert!(methodology
        .agent_profiles
        .contains(&"gsd-orchestrator".to_string()));
    assert!(methodology.agent_profiles.contains(&"gsd-qa".to_string()));
}

/// Test 10: Task in checkpoint column with discuss blocked status
async fn test_discuss_column_blocked(state: &AppState, project_id: Option<ProjectId>) {
    let gsd = create_gsd_methodology();
    let gsd_id = gsd.id.clone();
    state.methodology_repo.create(gsd).await.unwrap();
    state.methodology_repo.activate(&gsd_id).await.unwrap();

    // Verify discuss column maps to Blocked (for discussion/clarification)
    let active = state.methodology_repo.get_active().await.unwrap().unwrap();
    let discuss_column = active
        .workflow
        .columns
        .iter()
        .find(|c| c.id == "discuss")
        .unwrap();
    assert_eq!(discuss_column.maps_to, InternalStatus::Blocked);

    // Create a task and move to discuss (blocked)
    let project_id = project_id.unwrap_or_default();
    let mut task = Task::new(project_id, "Needs clarification".to_string());
    task.internal_status = InternalStatus::Blocked;
    state.task_repo.create(task.clone()).await.unwrap();

    let fetched = state.task_repo.get_by_id(&task.id).await.unwrap().unwrap();
    assert_eq!(fetched.internal_status, InternalStatus::Blocked);
}

// ============================================================================
// Memory Repository Tests
// ============================================================================

#[tokio::test]
async fn test_activate_gsd_methodology_with_memory() {
    let state = create_memory_state();
    test_activate_gsd_methodology(&state).await;
}

#[tokio::test]
async fn test_create_tasks_with_wave_and_checkpoint_with_memory() {
    let state = create_memory_state();
    test_create_tasks_with_wave_and_checkpoint(&state, None).await;
}

#[tokio::test]
async fn test_query_tasks_by_wave_with_memory() {
    let state = create_memory_state();
    test_query_tasks_by_wave(&state, None).await;
}

#[tokio::test]
async fn test_checkpoint_transitions_to_blocked_with_memory() {
    let state = create_memory_state();
    test_checkpoint_transitions_to_blocked(&state, None).await;
}

#[tokio::test]
async fn test_wave_completion_with_memory() {
    let state = create_memory_state();
    test_wave_completion(&state, None).await;
}

#[tokio::test]
async fn test_gsd_checkpoint_types_with_memory() {
    let state = create_memory_state();
    test_gsd_checkpoint_types(&state, None).await;
}

#[tokio::test]
async fn test_gsd_column_agent_profiles_with_memory() {
    let state = create_memory_state();
    test_gsd_column_agent_profiles(&state).await;
}

#[tokio::test]
async fn test_gsd_phase_structure_with_memory() {
    let state = create_memory_state();
    test_gsd_phase_structure(&state).await;
}

#[tokio::test]
async fn test_gsd_agent_profiles_with_memory() {
    let state = create_memory_state();
    test_gsd_agent_profiles(&state).await;
}

#[tokio::test]
async fn test_discuss_column_blocked_with_memory() {
    let state = create_memory_state();
    test_discuss_column_blocked(&state, None).await;
}

// ============================================================================
// SQLite Repository Tests
// ============================================================================

#[tokio::test]
async fn test_activate_gsd_methodology_with_sqlite() {
    let state = create_sqlite_state();
    test_activate_gsd_methodology(&state).await;
}

#[tokio::test]
async fn test_create_tasks_with_wave_and_checkpoint_with_sqlite() {
    let state = create_sqlite_state();
    let project_id = create_project_for_tasks(&state).await;
    test_create_tasks_with_wave_and_checkpoint(&state, Some(project_id)).await;
}

#[tokio::test]
async fn test_query_tasks_by_wave_with_sqlite() {
    let state = create_sqlite_state();
    let project_id = create_project_for_tasks(&state).await;
    test_query_tasks_by_wave(&state, Some(project_id)).await;
}

#[tokio::test]
async fn test_checkpoint_transitions_to_blocked_with_sqlite() {
    let state = create_sqlite_state();
    let project_id = create_project_for_tasks(&state).await;
    test_checkpoint_transitions_to_blocked(&state, Some(project_id)).await;
}

#[tokio::test]
async fn test_wave_completion_with_sqlite() {
    let state = create_sqlite_state();
    let project_id = create_project_for_tasks(&state).await;
    test_wave_completion(&state, Some(project_id)).await;
}

#[tokio::test]
async fn test_gsd_checkpoint_types_with_sqlite() {
    let state = create_sqlite_state();
    let project_id = create_project_for_tasks(&state).await;
    test_gsd_checkpoint_types(&state, Some(project_id)).await;
}

#[tokio::test]
async fn test_gsd_column_agent_profiles_with_sqlite() {
    let state = create_sqlite_state();
    test_gsd_column_agent_profiles(&state).await;
}

#[tokio::test]
async fn test_gsd_phase_structure_with_sqlite() {
    let state = create_sqlite_state();
    test_gsd_phase_structure(&state).await;
}

#[tokio::test]
async fn test_gsd_agent_profiles_with_sqlite() {
    let state = create_sqlite_state();
    test_gsd_agent_profiles(&state).await;
}

#[tokio::test]
async fn test_discuss_column_blocked_with_sqlite() {
    let state = create_sqlite_state();
    let project_id = create_project_for_tasks(&state).await;
    test_discuss_column_blocked(&state, Some(project_id)).await;
}
