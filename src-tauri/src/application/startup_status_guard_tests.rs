use super::startup_status::{
    StartupCoordinator, StartupFrontendMilestone, StartupStage, StartupStatusError,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn advance_to_runtime_ready(coordinator: &StartupCoordinator, attempt_id: u64) {
    for stage in [
        StartupStage::OpeningDatabase,
        StartupStage::Migrating,
        StartupStage::LoadingSettings,
        StartupStage::StartupCleanup,
        StartupStage::RegisteringState,
    ] {
        coordinator
            .advance(attempt_id, stage)
            .expect("startup prerequisite should advance");
    }
    coordinator
        .accept_app_state_registration(attempt_id, true)
        .expect("AppState registration should be accepted");
    coordinator
        .install_listeners(attempt_id, || {})
        .expect("listeners should install");
    coordinator
        .advance(attempt_id, StartupStage::BindingLocalRuntime)
        .expect("runtime binding should begin");
    coordinator
        .listener_bound(attempt_id)
        .expect("listener binding should be acknowledged");
    coordinator
        .complete_safety_barrier(attempt_id)
        .expect("safety barrier should complete");
    coordinator
        .publish_runtime_ready(attempt_id)
        .expect("runtime readiness should publish");
}

#[test]
fn every_out_of_order_stage_family_is_rejected_without_mutating_startup() {
    let coordinator = StartupCoordinator::default();
    let attempt_id = coordinator.current_attempt_id();

    assert!(!coordinator.can_retry());
    assert_eq!(coordinator.ensure_current(attempt_id), Ok(()));
    assert_eq!(
        coordinator.ensure_current(attempt_id + 1),
        Err(StartupStatusError::StaleAttempt)
    );

    for stage in [
        StartupStage::Migrating,
        StartupStage::LoadingSettings,
        StartupStage::StartupCleanup,
        StartupStage::RegisteringState,
        StartupStage::AppStateReady,
        StartupStage::BindingLocalRuntime,
        StartupStage::SafetyRecovery,
        StartupStage::RuntimeReady,
        StartupStage::BackgroundRecovery,
        StartupStage::Ready,
        StartupStage::Degraded,
        StartupStage::Failed,
    ] {
        assert_eq!(
            coordinator.advance(attempt_id, stage),
            Err(StartupStatusError::InvalidTransition)
        );
        assert_eq!(coordinator.snapshot().stage, StartupStage::CreatingWindow);
    }

    coordinator
        .advance(attempt_id, StartupStage::OpeningDatabase)
        .expect("first startup edge should remain valid");
    assert_eq!(
        coordinator.advance(attempt_id, StartupStage::CreatingWindow),
        Err(StartupStatusError::StageRegression)
    );
    assert_eq!(coordinator.snapshot().stage, StartupStage::OpeningDatabase);
}

#[test]
fn registration_and_runtime_guards_reject_wrong_phase_and_duplicate_effects() {
    let coordinator = StartupCoordinator::new();
    let attempt_id = coordinator.current_attempt_id();
    let registrations = AtomicUsize::new(0);

    assert_eq!(
        coordinator.register_app_state(attempt_id, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(registrations.load(Ordering::SeqCst), 0);
    assert_eq!(
        coordinator.install_listeners(attempt_id, || {}),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(
        coordinator.listener_bound(attempt_id),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(
        coordinator.complete_safety_barrier(attempt_id),
        Err(StartupStatusError::InvalidTransition)
    );

    for stage in [
        StartupStage::OpeningDatabase,
        StartupStage::Migrating,
        StartupStage::LoadingSettings,
        StartupStage::StartupCleanup,
        StartupStage::RegisteringState,
    ] {
        coordinator
            .advance(attempt_id, stage)
            .expect("startup prerequisite should advance");
    }
    coordinator
        .register_app_state(attempt_id, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        })
        .expect("first registration should succeed");
    assert_eq!(
        coordinator.register_app_state(attempt_id, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(StartupStatusError::AppStateAlreadyRegistered)
    );
    assert_eq!(registrations.load(Ordering::SeqCst), 1);
    assert!(coordinator.snapshot().app_state_ready);
}

#[test]
fn frontend_milestone_requires_the_current_runtime_ready_boot() {
    let coordinator = StartupCoordinator::new();
    let attempt_id = coordinator.current_attempt_id();

    assert_eq!(
        coordinator.accept_shell_paint("wrong-boot", attempt_id),
        Err(StartupStatusError::InvalidTransition)
    );

    advance_to_runtime_ready(&coordinator, attempt_id);
    let boot_id = coordinator.snapshot().boot_id;

    assert_eq!(
        coordinator.accept_shell_paint("wrong-boot", attempt_id),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(
        coordinator.accept_frontend_milestone(
            &boot_id,
            attempt_id,
            StartupFrontendMilestone::ShellPainted,
        ),
        Ok(())
    );

    coordinator.cancel();
    coordinator.cancel();
    assert!(coordinator.is_cancelled());
    assert_eq!(
        coordinator.ensure_current(attempt_id),
        Err(StartupStatusError::Cancelled)
    );
}
