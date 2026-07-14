// Testing utilities
// Cost-optimized test prompts and helpers

pub mod sqlite_test_db;
pub mod test_prompts;

// Re-export commonly used items
pub use sqlite_test_db::{SqliteStateFixture, SqliteTestDb};
pub use test_prompts::{
    assert_marker, contains_marker, iteration_expected, iteration_test_prompt, ECHO_MARKER,
    QA_PREP_TEST, QA_REFINER_TEST, QA_TESTER_TEST, REVIEWER_TEST, WORKER_SPAWN_TEST,
};

#[cfg(any(test, feature = "test-utils"))]
pub use crate::infrastructure::sqlite::{
    SqliteBranchUpdateRepository, SqliteTaskRepository,
};

#[cfg(any(test, feature = "test-utils"))]
pub fn memory_branch_update_repository(
) -> std::sync::Arc<dyn crate::domain::repositories::BranchUpdateRepository> {
    std::sync::Arc::new(crate::infrastructure::memory::MemoryBranchUpdateRepository::new())
}

#[cfg(any(test, feature = "test-utils"))]
pub async fn seeded_memory_branch_update_repository(
    task_id: crate::domain::entities::TaskId,
    status: crate::domain::entities::InternalStatus,
) -> std::sync::Arc<dyn crate::domain::repositories::BranchUpdateRepository> {
    let repository =
        std::sync::Arc::new(crate::infrastructure::memory::MemoryBranchUpdateRepository::new());
    repository.seed_task_status(task_id, status).await;
    repository
}

#[cfg(any(test, feature = "test-utils"))]
pub fn memory_branch_update_repository_with_task_repository(
    task_repository: std::sync::Arc<dyn crate::domain::repositories::TaskRepository>,
) -> std::sync::Arc<dyn crate::domain::repositories::BranchUpdateRepository> {
    std::sync::Arc::new(
        crate::infrastructure::memory::MemoryBranchUpdateRepository::new()
            .with_task_repository(task_repository),
    )
}

#[cfg(any(test, feature = "test-utils"))]
pub fn branch_update_workflow(
    chat_service: std::sync::Arc<dyn crate::application::ChatService>,
) -> std::sync::Arc<dyn crate::domain::state_machine::services::BranchUpdateWorkflow> {
    std::sync::Arc::new(
        crate::application::branch_update_workflow::ApplicationBranchUpdateWorkflow::new(
            chat_service,
        ),
    )
}

// Re-export merge validation helpers for integration testing
pub use crate::domain::state_machine::transition_handler::{
    run_pre_execution_setup, PreExecSetupResult,
};

#[cfg(feature = "test-utils")]
pub mod mock_app;

#[cfg(feature = "test-utils")]
pub use mock_app::{create_mock_app, create_mock_app_handle};

/// Seed fake harness probes so integration tests do not depend on installed provider CLIs.
#[cfg(feature = "test-utils")]
pub fn seed_available_harness_probes_for_test() {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test();
}

/// Seed fake harness probes pinned to a caller-owned fake CLI path, for tests
/// that exercise real spawn paths which validate CLI existence on disk.
#[cfg(feature = "test-utils")]
pub fn seed_available_harness_probes_for_test_at(binary_path: &str) {
    crate::application::harness_runtime_registry::seed_available_harness_probes_for_test_at(
        binary_path,
    );
}
