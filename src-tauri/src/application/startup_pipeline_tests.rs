use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::startup_pipeline::{
    publish_runtime_boundary_for, run_startup_orphan_mcp_cleanup, run_startup_owned_phase,
    startup_phase_completed, startup_phase_started, startup_previous_session_cutoff,
};

#[test]
fn startup_previous_session_cutoff_uses_current_time() {
    let before = chrono::Utc::now();
    let cutoff = startup_previous_session_cutoff();
    let after = chrono::Utc::now();

    assert!(cutoff >= before);
    assert!(cutoff <= after);
}

#[test]
fn startup_orphan_mcp_cleanup_reports_killed_count() {
    let killed = run_startup_orphan_mcp_cleanup(|| 2);

    assert_eq!(killed, 2);
}

#[test]
fn startup_orphan_mcp_cleanup_allows_noop_cleanup() {
    let killed = run_startup_orphan_mcp_cleanup(|| 0);

    assert_eq!(killed, 0);
}

#[test]
fn startup_phase_timing_helpers_emit_completion_telemetry() {
    let started = startup_phase_started("test_phase");

    startup_phase_completed("test_phase", started);
}

#[tokio::test]
async fn startup_owned_phase_settles_before_returning() {
    let completed = Arc::new(AtomicBool::new(false));
    let completed_in_task = Arc::clone(&completed);

    run_startup_owned_phase("test_background_phase", Duration::ZERO, async move {
        completed_in_task.store(true, Ordering::SeqCst);
    })
    .await;

    assert!(completed.load(Ordering::SeqCst));
}

#[test]
fn runtime_boundary_publishes_only_after_listener_and_safety_recovery() {
    use crate::application::startup_status::{StartupCoordinator, StartupStage};

    let coordinator = Arc::new(StartupCoordinator::new());
    let attempt = coordinator.current_attempt_id();
    for stage in [
        StartupStage::OpeningDatabase,
        StartupStage::Migrating,
        StartupStage::LoadingSettings,
        StartupStage::StartupCleanup,
        StartupStage::RegisteringState,
    ] {
        coordinator.advance(attempt, stage).unwrap();
    }
    coordinator
        .accept_app_state_registration(attempt, true)
        .unwrap();
    coordinator.install_listeners(attempt, || {}).unwrap();
    coordinator
        .advance(attempt, StartupStage::BindingLocalRuntime)
        .unwrap();
    coordinator.listener_bound(attempt).unwrap();

    publish_runtime_boundary_for(Some(&coordinator), Some(attempt)).unwrap();

    let snapshot = coordinator.snapshot();
    assert!(snapshot.runtime_ready);
    assert!(!snapshot.background_complete);
    assert_eq!(snapshot.stage, StartupStage::BackgroundRecovery);
}
