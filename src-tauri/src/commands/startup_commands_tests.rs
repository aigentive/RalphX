use std::sync::atomic::{AtomicUsize, Ordering};

use super::startup_commands::{
    build_startup_diagnostics, open_startup_log_directory, retry_startup_with_launcher,
    startup_runtime_log_directory,
};
use crate::application::startup_status::{StartupCoordinator, StartupFailureCode, StartupStage};

fn advance_to_registering_state(coordinator: &StartupCoordinator, attempt: u64) {
    for stage in [
        StartupStage::OpeningDatabase,
        StartupStage::Migrating,
        StartupStage::LoadingSettings,
        StartupStage::StartupCleanup,
        StartupStage::RegisteringState,
    ] {
        coordinator
            .advance(attempt, stage)
            .expect("legal startup transition");
    }
}

fn assert_retry_does_not_launch(coordinator: &StartupCoordinator) {
    let launches = AtomicUsize::new(0);

    assert!(retry_startup_with_launcher(coordinator, |_| {
        launches.fetch_add(1, Ordering::SeqCst);
    })
    .is_err());
    assert_eq!(launches.load(Ordering::SeqCst), 0);
}

#[test]
fn startup_diagnostics_are_redacted_by_construction() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    coordinator.fail(
        attempt,
        StartupFailureCode::AppStateConstruction,
        "Authorization: Bearer secret-startup-token",
    );

    let snapshot = coordinator.snapshot();
    let diagnostics = build_startup_diagnostics(snapshot.clone());
    let serialized = serde_json::to_string(&diagnostics).expect("serialize diagnostics");

    assert_eq!(
        diagnostics.failure_code,
        Some(StartupFailureCode::AppStateConstruction)
    );
    assert_eq!(diagnostics.can_retry, snapshot.retry_allowed);
    assert!(!serialized.contains("secret-startup-token"));
    assert!(!serialized.contains("Authorization"));
    assert!(!serialized.contains("diagnostic_summary"));
    assert!(!serialized.contains("boot_id"));
    assert!(!serialized.contains("path"));
}

#[test]
fn retry_launches_only_after_a_quiesced_pre_registration_failure() {
    let coordinator = StartupCoordinator::new();
    let first_attempt = coordinator.current_attempt_id();
    let launches = AtomicUsize::new(0);
    coordinator.fail(
        first_attempt,
        StartupFailureCode::AppStateConstruction,
        "construction failed",
    );

    let snapshot = retry_startup_with_launcher(&coordinator, |attempt_id| {
        assert_eq!(attempt_id, first_attempt + 1);
        launches.fetch_add(1, Ordering::SeqCst);
    })
    .expect("pre-registration failure can retry");

    assert_eq!(snapshot.attempt_id, first_attempt + 1);
    assert_eq!(launches.load(Ordering::SeqCst), 1);
}

#[test]
fn post_registration_partial_registration_and_bind_failures_cannot_relaunch() {
    let post_registration = StartupCoordinator::new();
    let post_registration_attempt = post_registration.current_attempt_id();
    advance_to_registering_state(&post_registration, post_registration_attempt);
    post_registration
        .accept_app_state_registration(post_registration_attempt, true)
        .expect("completed registration");
    post_registration.fail(
        post_registration_attempt,
        StartupFailureCode::LocalRuntimeBind,
        "later failure",
    );
    assert_retry_does_not_launch(&post_registration);

    let partial_registration = StartupCoordinator::new();
    let partial_registration_attempt = partial_registration.current_attempt_id();
    advance_to_registering_state(&partial_registration, partial_registration_attempt);
    assert!(partial_registration
        .register_app_state(partial_registration_attempt, |effects| {
            effects.record_side_effect();
            false
        })
        .is_err());
    assert_retry_does_not_launch(&partial_registration);

    let bind_failure = StartupCoordinator::new();
    let bind_attempt = bind_failure.current_attempt_id();
    advance_to_registering_state(&bind_failure, bind_attempt);
    bind_failure
        .accept_app_state_registration(bind_attempt, true)
        .expect("completed registration");
    bind_failure
        .install_listeners(bind_attempt, || {})
        .expect("listener installation");
    bind_failure
        .advance(bind_attempt, StartupStage::BindingLocalRuntime)
        .expect("binding stage");
    bind_failure.fail(
        bind_attempt,
        StartupFailureCode::LocalRuntimeBind,
        "bind failed",
    );
    assert_retry_does_not_launch(&bind_failure);
}

#[test]
fn cancelled_startup_cannot_launch_a_retry_attempt() {
    let coordinator = StartupCoordinator::new();

    coordinator.cancel();

    assert_retry_does_not_launch(&coordinator);
}

#[test]
fn open_logs_uses_only_the_process_owned_runtime_log_directory() {
    let expected_log_directory = crate::utils::runtime_log_paths::app_log_dir();

    assert_eq!(startup_runtime_log_directory(), expected_log_directory);
    open_startup_log_directory(|directory| {
        assert_eq!(directory, expected_log_directory.as_path());
        Ok(())
    })
    .expect("process-owned log directory opens");
}
