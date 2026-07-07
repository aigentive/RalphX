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

// Re-export merge validation helpers for integration testing
pub use crate::domain::state_machine::transition_handler::{
    PreExecSetupResult, run_pre_execution_setup,
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
