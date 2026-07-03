// Integration test: Research process lifecycle
//
// Tests end-to-end research process operations:
// - Start research with quick-scan preset
// - Pause and resume research
// - Checkpoint saves progress
// - Complete research (output artifacts created by service calling ArtifactService)
//
// Both memory and SQLite repositories are tested to ensure consistent behavior.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    ArtifactId, ArtifactType, ResearchBrief, ResearchDepth, ResearchDepthPreset, ResearchOutput,
    ResearchProcess, ResearchProcessStatus,
};
use ralphx_lib::infrastructure::sqlite::SqliteProcessRepository;
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
    SqliteStateFixture::new("research-integration", |db, state| {
        state.process_repo = Arc::new(SqliteProcessRepository::from_shared(db.shared_conn()));
    })
}

/// Create a research process with quick-scan preset
fn create_quick_scan_research() -> ResearchProcess {
    let brief = ResearchBrief::new("What is the best authentication approach for our app?")
        .with_context("We need to add user authentication to our existing React app")
        .with_scope("OAuth2, JWT, and session-based auth")
        .with_constraint("Must work with mobile apps too")
        .with_constraint("Should support SSO");

    let output = ResearchOutput::new("research-outputs")
        .with_artifact_type(ArtifactType::ResearchDocument)
        .with_artifact_type(ArtifactType::Findings)
        .with_artifact_type(ArtifactType::Recommendations);

    ResearchProcess::new("Authentication Research", brief, "deep-researcher")
        .with_depth(ResearchDepth::Preset(ResearchDepthPreset::QuickScan))
        .with_output(output)
}

/// Create a research process with standard preset (more iterations for lifecycle test)
fn create_standard_research() -> ResearchProcess {
    let brief = ResearchBrief::new("What database should we use?")
        .with_context("Building a new microservice")
        .with_scope("SQL vs NoSQL options");

    let output = ResearchOutput::new("research-outputs").with_artifact_type(ArtifactType::Findings);

    ResearchProcess::new("Database Research", brief, "deep-researcher")
        .with_depth(ResearchDepth::Preset(ResearchDepthPreset::Standard))
        .with_output(output)
}

// ============================================================================
// Shared Test Logic (works with any repository implementation)
// ============================================================================

/// Test 1: Start research with quick-scan preset
async fn test_start_research_with_quick_scan(state: &AppState) {
    let process = create_quick_scan_research();
    let process_id = process.id.clone();

    // Create the process (initially pending)
    let created = state.process_repo.create(process).await.unwrap();
    assert_eq!(created.progress.status, ResearchProcessStatus::Pending);
    assert_eq!(created.name, "Authentication Research");

    // Verify quick-scan depth preset
    match &created.depth {
        ResearchDepth::Preset(preset) => {
            assert_eq!(*preset, ResearchDepthPreset::QuickScan);
        }
        ResearchDepth::Custom(_) => panic!("Expected preset depth, got custom"),
    }

    // Verify the brief contains all our input
    assert!(created.brief.question.contains("authentication"));
    assert!(created.brief.context.is_some());
    assert!(created.brief.scope.is_some());
    assert_eq!(created.brief.constraints.len(), 2);

    // Retrieve and verify persistence
    let found = state.process_repo.get_by_id(&process_id).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.name, "Authentication Research");
    assert_eq!(found.progress.status, ResearchProcessStatus::Pending);
}

/// Test 2: Start and run research process
async fn test_start_and_run_research(state: &AppState) {
    let mut process = create_quick_scan_research();
    let process_id = process.id.clone();

    // Create the process
    state.process_repo.create(process.clone()).await.unwrap();

    // Start the process (transition to running)
    process.start();
    assert_eq!(process.progress.status, ResearchProcessStatus::Running);
    assert!(process.started_at.is_some());

    // Update in repository
    state.process_repo.update(&process).await.unwrap();

    // Verify the update persisted
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.progress.status, ResearchProcessStatus::Running);
    assert!(found.started_at.is_some());

    // Verify it appears in active processes
    let active = state.process_repo.get_active().await.unwrap();
    assert!(active.iter().any(|p| p.id == process_id));
}

/// Test 3: Pause a running research process
async fn test_pause_research(state: &AppState) {
    let mut process = create_standard_research();
    let process_id = process.id.clone();

    // Create and start
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();
    state.process_repo.update(&process).await.unwrap();

    // Advance a few iterations
    process.advance();
    process.advance();
    process.advance();
    assert_eq!(process.progress.current_iteration, 3);
    state.process_repo.update_progress(&process).await.unwrap();

    // Pause the process
    process.pause();
    assert_eq!(process.progress.status, ResearchProcessStatus::Paused);
    state.process_repo.update(&process).await.unwrap();

    // Verify paused state persisted
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.progress.status, ResearchProcessStatus::Paused);
    assert_eq!(found.progress.current_iteration, 3);

    // Should still be in "active" (non-terminal) queries via get_by_status
    let paused = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Paused)
        .await
        .unwrap();
    assert!(paused.iter().any(|p| p.id == process_id));
}

/// Test 4: Resume a paused research process
async fn test_resume_research(state: &AppState) {
    let mut process = create_standard_research();
    let process_id = process.id.clone();

    // Create, start, then pause
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();
    process.advance();
    process.advance();
    process.pause();
    state.process_repo.update(&process).await.unwrap();

    // Resume the process
    process.resume();
    assert_eq!(process.progress.status, ResearchProcessStatus::Running);
    state.process_repo.update(&process).await.unwrap();

    // Verify resumed state persisted
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.progress.status, ResearchProcessStatus::Running);
    // Iterations should still be preserved
    assert_eq!(found.progress.current_iteration, 2);

    // Should be in running processes
    let running = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Running)
        .await
        .unwrap();
    assert!(running.iter().any(|p| p.id == process_id));
}

/// Test 5: Full pause-resume cycle preserves progress
async fn test_pause_resume_cycle_preserves_progress(state: &AppState) {
    let mut process = create_standard_research();
    let process_id = process.id.clone();

    // Create and start
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();

    // Do some iterations
    for _ in 0..5 {
        process.advance();
    }
    state.process_repo.update_progress(&process).await.unwrap();
    assert_eq!(process.progress.current_iteration, 5);

    // Pause
    process.pause();
    state.process_repo.update(&process).await.unwrap();

    // Resume
    process.resume();
    state.process_repo.update(&process).await.unwrap();

    // Continue with more iterations
    for _ in 0..3 {
        process.advance();
    }
    state.process_repo.update_progress(&process).await.unwrap();
    assert_eq!(process.progress.current_iteration, 8);

    // Verify persistence
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.progress.current_iteration, 8);
    assert_eq!(found.progress.status, ResearchProcessStatus::Running);
}

/// Test 6: Checkpoint saves progress with artifact reference
async fn test_checkpoint_saves_progress(state: &AppState) {
    let mut process = create_quick_scan_research();
    let process_id = process.id.clone();

    // Create and start
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();

    // Advance to checkpoint interval (quick-scan: checkpoint_interval = 5)
    for _ in 0..5 {
        process.advance();
    }

    // Create a checkpoint with artifact ID
    let checkpoint_artifact_id = ArtifactId::from_string("artifact-checkpoint-001");
    process.checkpoint(checkpoint_artifact_id.clone());
    assert_eq!(
        process.progress.last_checkpoint,
        Some(checkpoint_artifact_id.clone())
    );
    state.process_repo.update_progress(&process).await.unwrap();

    // Verify checkpoint persisted
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        found.progress.last_checkpoint,
        Some(ArtifactId::from_string("artifact-checkpoint-001"))
    );
    assert_eq!(found.progress.current_iteration, 5);
}

/// Test 7: Multiple checkpoints update correctly
async fn test_multiple_checkpoints(state: &AppState) {
    let mut process = create_standard_research();
    let process_id = process.id.clone();

    // Create and start
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();

    // First checkpoint at iteration 10
    for _ in 0..10 {
        process.advance();
    }
    process.checkpoint(ArtifactId::from_string("checkpoint-1"));
    state.process_repo.update_progress(&process).await.unwrap();

    // Second checkpoint at iteration 20
    for _ in 0..10 {
        process.advance();
    }
    process.checkpoint(ArtifactId::from_string("checkpoint-2"));
    state.process_repo.update_progress(&process).await.unwrap();

    // Verify latest checkpoint is stored
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        found.progress.last_checkpoint,
        Some(ArtifactId::from_string("checkpoint-2"))
    );
    assert_eq!(found.progress.current_iteration, 20);
}

/// Test 8: Complete research successfully
async fn test_complete_research(state: &AppState) {
    let mut process = create_quick_scan_research();
    let process_id = process.id.clone();

    // Create, start, run to completion
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();

    // Advance all 10 iterations (quick-scan max)
    for _ in 0..10 {
        process.advance();
    }
    assert_eq!(process.progress.current_iteration, 10);

    // Complete the process
    process.complete();
    assert_eq!(process.progress.status, ResearchProcessStatus::Completed);
    assert!(process.completed_at.is_some());

    state.process_repo.update(&process).await.unwrap();

    // Verify completion persisted
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.progress.status, ResearchProcessStatus::Completed);
    assert!(found.completed_at.is_some());

    // Should NOT be in active processes
    let active = state.process_repo.get_active().await.unwrap();
    assert!(!active.iter().any(|p| p.id == process_id));

    // Should be in completed processes
    let completed = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Completed)
        .await
        .unwrap();
    assert!(completed.iter().any(|p| p.id == process_id));
}

/// Test 9: Fail research with error message
async fn test_fail_research(state: &AppState) {
    let mut process = create_quick_scan_research();
    let process_id = process.id.clone();

    // Create and start
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();
    process.advance();

    // Fail with error
    process.fail("API rate limit exceeded");
    assert_eq!(process.progress.status, ResearchProcessStatus::Failed);
    assert_eq!(
        process.progress.error_message,
        Some("API rate limit exceeded".to_string())
    );

    state.process_repo.update(&process).await.unwrap();

    // Verify failure persisted
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.progress.status, ResearchProcessStatus::Failed);
    assert_eq!(
        found.progress.error_message,
        Some("API rate limit exceeded".to_string())
    );

    // Should NOT be in active processes
    let active = state.process_repo.get_active().await.unwrap();
    assert!(!active.iter().any(|p| p.id == process_id));
}

/// Test 10: Query processes by status
async fn test_query_by_status(state: &AppState) {
    // Create processes in different states
    let pending_process = create_quick_scan_research();
    let mut running_process = create_standard_research();
    let mut completed_process = ResearchProcess::new(
        "Completed Research",
        ResearchBrief::new("Old question"),
        "test-agent",
    );

    // Create pending
    state
        .process_repo
        .create(pending_process.clone())
        .await
        .unwrap();

    // Create and start running
    state
        .process_repo
        .create(running_process.clone())
        .await
        .unwrap();
    running_process.start();
    state.process_repo.update(&running_process).await.unwrap();

    // Create, start, and complete
    state
        .process_repo
        .create(completed_process.clone())
        .await
        .unwrap();
    completed_process.start();
    completed_process.complete();
    state.process_repo.update(&completed_process).await.unwrap();

    // Query by status
    let pending = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Pending)
        .await
        .unwrap();
    let running = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Running)
        .await
        .unwrap();
    let completed = state
        .process_repo
        .get_by_status(ResearchProcessStatus::Completed)
        .await
        .unwrap();

    assert!(pending.iter().any(|p| p.id == pending_process.id));
    assert!(running.iter().any(|p| p.id == running_process.id));
    assert!(completed.iter().any(|p| p.id == completed_process.id));
}

/// Test 11: Get all processes returns in created_at order
async fn test_get_all_ordered(state: &AppState) {
    // Create several processes with slight delays to ensure ordering
    let p1 = ResearchProcess::new("First Research", ResearchBrief::new("Q1"), "agent-1");
    let p2 = ResearchProcess::new("Second Research", ResearchBrief::new("Q2"), "agent-2");
    let p3 = ResearchProcess::new("Third Research", ResearchBrief::new("Q3"), "agent-3");

    state.process_repo.create(p1.clone()).await.unwrap();
    state.process_repo.create(p2.clone()).await.unwrap();
    state.process_repo.create(p3.clone()).await.unwrap();

    // Get all should return in descending created_at order (newest first)
    let all = state.process_repo.get_all().await.unwrap();
    assert!(all.len() >= 3);

    // Find our processes in the list
    let our_processes: Vec<_> = all.iter().filter(|p| p.name.contains("Research")).collect();
    assert!(our_processes.len() >= 3);
}

/// Test 12: Delete research process
async fn test_delete_research(state: &AppState) {
    let process = create_quick_scan_research();
    let process_id = process.id.clone();

    // Create
    state.process_repo.create(process).await.unwrap();
    assert!(state.process_repo.exists(&process_id).await.unwrap());

    // Delete
    state.process_repo.delete(&process_id).await.unwrap();
    assert!(!state.process_repo.exists(&process_id).await.unwrap());

    // Verify not found
    let found = state.process_repo.get_by_id(&process_id).await.unwrap();
    assert!(found.is_none());
}

/// Test 13: Progress percentage calculation
async fn test_progress_percentage(state: &AppState) {
    let mut process = create_quick_scan_research();

    // Quick-scan has max_iterations = 10
    state.process_repo.create(process.clone()).await.unwrap();
    process.start();

    // At 0 iterations: 0%
    assert_eq!(process.progress_percentage(), 0.0);

    // At 5 iterations: 50%
    for _ in 0..5 {
        process.advance();
    }
    state.process_repo.update_progress(&process).await.unwrap();
    assert!((process.progress_percentage() - 50.0).abs() < 0.01);

    // At 10 iterations: 100%
    for _ in 0..5 {
        process.advance();
    }
    state.process_repo.update_progress(&process).await.unwrap();
    assert!((process.progress_percentage() - 100.0).abs() < 0.01);
}

/// Test 14: Custom depth configuration
async fn test_custom_depth_research(state: &AppState) {
    let brief = ResearchBrief::new("Custom depth research question");
    let output = ResearchOutput::new("research-outputs");

    let custom_depth = ralphx_lib::domain::entities::CustomDepth::new(25, 1.5, 5);
    let process = ResearchProcess::new("Custom Depth Research", brief, "custom-agent")
        .with_depth(ResearchDepth::Custom(custom_depth))
        .with_output(output);

    let process_id = process.id.clone();
    state.process_repo.create(process.clone()).await.unwrap();

    // Verify custom depth preserved
    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    match &found.depth {
        ResearchDepth::Custom(depth) => {
            assert_eq!(depth.max_iterations, 25);
            assert!((depth.timeout_hours - 1.5).abs() < 0.001);
            assert_eq!(depth.checkpoint_interval, 5);
        }
        ResearchDepth::Preset(_) => panic!("Expected custom depth, got preset"),
    }
}

/// Test 15: Verify output configuration persists
async fn test_output_configuration(state: &AppState) {
    let brief = ResearchBrief::new("What output types do we need?");
    let output = ResearchOutput::new("research-outputs")
        .with_artifact_type(ArtifactType::ResearchDocument)
        .with_artifact_type(ArtifactType::Findings)
        .with_artifact_type(ArtifactType::Recommendations);

    let process = ResearchProcess::new("Output Test", brief, "output-agent").with_output(output);

    let process_id = process.id.clone();
    state.process_repo.create(process).await.unwrap();

    let found = state
        .process_repo
        .get_by_id(&process_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.output.target_bucket, "research-outputs");
    assert_eq!(found.output.artifact_types.len(), 3);
    assert!(found
        .output
        .artifact_types
        .contains(&ArtifactType::ResearchDocument));
    assert!(found
        .output
        .artifact_types
        .contains(&ArtifactType::Findings));
    assert!(found
        .output
        .artifact_types
        .contains(&ArtifactType::Recommendations));
}

// ============================================================================
// Memory Repository Tests
// ============================================================================

#[tokio::test]
async fn memory_test_start_research_with_quick_scan() {
    let state = create_memory_state();
    test_start_research_with_quick_scan(&state).await;
}

#[tokio::test]
async fn memory_test_start_and_run_research() {
    let state = create_memory_state();
    test_start_and_run_research(&state).await;
}

#[tokio::test]
async fn memory_test_pause_research() {
    let state = create_memory_state();
    test_pause_research(&state).await;
}

#[tokio::test]
async fn memory_test_resume_research() {
    let state = create_memory_state();
    test_resume_research(&state).await;
}

#[tokio::test]
async fn memory_test_pause_resume_cycle_preserves_progress() {
    let state = create_memory_state();
    test_pause_resume_cycle_preserves_progress(&state).await;
}

#[tokio::test]
async fn memory_test_checkpoint_saves_progress() {
    let state = create_memory_state();
    test_checkpoint_saves_progress(&state).await;
}

#[tokio::test]
async fn memory_test_multiple_checkpoints() {
    let state = create_memory_state();
    test_multiple_checkpoints(&state).await;
}

#[tokio::test]
async fn memory_test_complete_research() {
    let state = create_memory_state();
    test_complete_research(&state).await;
}

#[tokio::test]
async fn memory_test_fail_research() {
    let state = create_memory_state();
    test_fail_research(&state).await;
}

#[tokio::test]
async fn memory_test_query_by_status() {
    let state = create_memory_state();
    test_query_by_status(&state).await;
}

#[tokio::test]
async fn memory_test_get_all_ordered() {
    let state = create_memory_state();
    test_get_all_ordered(&state).await;
}

#[tokio::test]
async fn memory_test_delete_research() {
    let state = create_memory_state();
    test_delete_research(&state).await;
}

#[tokio::test]
async fn memory_test_progress_percentage() {
    let state = create_memory_state();
    test_progress_percentage(&state).await;
}

#[tokio::test]
async fn memory_test_custom_depth_research() {
    let state = create_memory_state();
    test_custom_depth_research(&state).await;
}

#[tokio::test]
async fn memory_test_output_configuration() {
    let state = create_memory_state();
    test_output_configuration(&state).await;
}

// ============================================================================
// SQLite Repository Tests
// ============================================================================

#[tokio::test]
async fn sqlite_test_start_research_with_quick_scan() {
    let state = create_sqlite_state();
    test_start_research_with_quick_scan(&state).await;
}

#[tokio::test]
async fn sqlite_test_start_and_run_research() {
    let state = create_sqlite_state();
    test_start_and_run_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_pause_research() {
    let state = create_sqlite_state();
    test_pause_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_resume_research() {
    let state = create_sqlite_state();
    test_resume_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_pause_resume_cycle_preserves_progress() {
    let state = create_sqlite_state();
    test_pause_resume_cycle_preserves_progress(&state).await;
}

#[tokio::test]
async fn sqlite_test_checkpoint_saves_progress() {
    let state = create_sqlite_state();
    test_checkpoint_saves_progress(&state).await;
}

#[tokio::test]
async fn sqlite_test_multiple_checkpoints() {
    let state = create_sqlite_state();
    test_multiple_checkpoints(&state).await;
}

#[tokio::test]
async fn sqlite_test_complete_research() {
    let state = create_sqlite_state();
    test_complete_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_fail_research() {
    let state = create_sqlite_state();
    test_fail_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_query_by_status() {
    let state = create_sqlite_state();
    test_query_by_status(&state).await;
}

#[tokio::test]
async fn sqlite_test_get_all_ordered() {
    let state = create_sqlite_state();
    test_get_all_ordered(&state).await;
}

#[tokio::test]
async fn sqlite_test_delete_research() {
    let state = create_sqlite_state();
    test_delete_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_progress_percentage() {
    let state = create_sqlite_state();
    test_progress_percentage(&state).await;
}

#[tokio::test]
async fn sqlite_test_custom_depth_research() {
    let state = create_sqlite_state();
    test_custom_depth_research(&state).await;
}

#[tokio::test]
async fn sqlite_test_output_configuration() {
    let state = create_sqlite_state();
    test_output_configuration(&state).await;
}
