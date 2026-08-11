use super::startup_status::{
    StartupCoordinator, StartupFailureCode, StartupStage, StartupStatusError,
};
use std::sync::atomic::{AtomicUsize, Ordering};

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

fn register_and_install_listeners(coordinator: &StartupCoordinator, attempt: u64) {
    advance_to_registering_state(coordinator, attempt);
    coordinator
        .accept_app_state_registration(attempt, true)
        .expect("accepted registration");
    coordinator
        .install_listeners(attempt, || {})
        .expect("listener installation");
}

fn advance_to_terminal_stage(coordinator: &StartupCoordinator, terminal: StartupStage) {
    let attempt = coordinator.current_attempt_id();
    register_and_install_listeners(coordinator, attempt);
    coordinator
        .advance(attempt, StartupStage::BindingLocalRuntime)
        .expect("binding stage");
    coordinator
        .listener_bound(attempt)
        .expect("listener bind acknowledgement");
    coordinator
        .complete_safety_barrier(attempt)
        .expect("safety barrier");
    coordinator
        .publish_runtime_ready(attempt)
        .expect("runtime readiness");
    coordinator
        .advance(attempt, StartupStage::BackgroundRecovery)
        .expect("background recovery");
    coordinator
        .advance(attempt, terminal)
        .expect("terminal settlement");
}

fn complete_background_recovery(
    coordinator: &StartupCoordinator,
    attempt: u64,
    terminal_stage: StartupStage,
) {
    register_and_install_listeners(coordinator, attempt);
    coordinator
        .advance(attempt, StartupStage::BindingLocalRuntime)
        .expect("binding stage");
    coordinator
        .listener_bound(attempt)
        .expect("listener bind acknowledgement");
    coordinator
        .complete_safety_barrier(attempt)
        .expect("safety barrier completion");
    coordinator
        .publish_runtime_ready(attempt)
        .expect("runtime readiness");
    coordinator
        .advance(attempt, StartupStage::BackgroundRecovery)
        .expect("background recovery");
    coordinator
        .advance(attempt, terminal_stage)
        .expect("terminal startup stage");
}

#[test]
fn startup_stages_reject_skips_and_dedicated_boundaries() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    assert_eq!(
        coordinator.advance(attempt, StartupStage::Migrating),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(coordinator.snapshot().stage, StartupStage::CreatingWindow);

    coordinator
        .advance(attempt, StartupStage::OpeningDatabase)
        .expect("opening database is the first legal edge");
    assert_eq!(
        coordinator.advance(attempt, StartupStage::LoadingSettings),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(
        coordinator.advance(attempt, StartupStage::AppStateReady),
        Err(StartupStatusError::InvalidTransition)
    );
}

#[test]
fn runtime_ready_cannot_be_published_early_or_without_the_safety_barrier() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    assert_eq!(
        coordinator.advance(attempt, StartupStage::RuntimeReady),
        Err(StartupStatusError::InvalidTransition)
    );
    assert!(!coordinator.snapshot().runtime_ready);

    register_and_install_listeners(&coordinator, attempt);
    coordinator
        .advance(attempt, StartupStage::BindingLocalRuntime)
        .expect("binding stage");
    coordinator
        .listener_bound(attempt)
        .expect("listener bind acknowledgement");

    assert_eq!(
        coordinator.publish_runtime_ready(attempt),
        Err(StartupStatusError::InvalidTransition)
    );
    assert!(!coordinator.snapshot().runtime_ready);
    assert_eq!(coordinator.snapshot().stage, StartupStage::SafetyRecovery);

    coordinator
        .complete_safety_barrier(attempt)
        .expect("caller completes the safety barrier");
    coordinator
        .publish_runtime_ready(attempt)
        .expect("guarded runtime readiness");
    assert!(coordinator.snapshot().runtime_ready);
    assert_eq!(coordinator.snapshot().stage, StartupStage::RuntimeReady);
}

#[test]
fn migration_progress_is_monotonic_and_stage_scoped() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    coordinator
        .advance(attempt, StartupStage::OpeningDatabase)
        .unwrap();
    coordinator
        .advance(attempt, StartupStage::Migrating)
        .unwrap();

    coordinator.report_progress(attempt, 0, 2).unwrap();
    coordinator.report_progress(attempt, 1, 2).unwrap();
    assert_eq!(
        coordinator.snapshot().progress,
        Some(super::startup_status::StartupProgress {
            completed_units: 1,
            total_units: 2,
        })
    );
    assert_eq!(
        coordinator.report_progress(attempt, 0, 2),
        Err(StartupStatusError::StageRegression)
    );
    assert_eq!(
        coordinator.report_progress(attempt, 1, 3),
        Err(StartupStatusError::InvalidTransition)
    );
    assert_eq!(
        coordinator.report_progress(attempt, 3, 2),
        Err(StartupStatusError::InvalidTransition)
    );

    coordinator
        .advance(attempt, StartupStage::LoadingSettings)
        .unwrap();
    assert_eq!(coordinator.snapshot().progress, None);
    assert_eq!(
        coordinator.report_progress(attempt, 2, 2),
        Err(StartupStatusError::InvalidTransition)
    );
}

#[test]
fn app_state_ready_requires_one_accepted_registration() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    advance_to_registering_state(&coordinator, attempt);
    assert_eq!(
        coordinator.accept_app_state_registration(attempt, false),
        Err(StartupStatusError::AppStateRegistrationRejected)
    );

    let snapshot = coordinator.snapshot();
    assert!(!snapshot.app_state_ready);
    assert_eq!(snapshot.stage, StartupStage::Failed);
    assert_eq!(
        snapshot.failure_code,
        Some(StartupFailureCode::AppStateRegistration)
    );
    assert_eq!(
        snapshot.diagnostic_summary.as_deref(),
        Some("RalphX could not register its application state.")
    );
}

#[test]
fn partial_registration_failure_is_terminal_and_relaunch_only() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    let registrations = AtomicUsize::new(0);

    advance_to_registering_state(&coordinator, attempt);
    assert_eq!(
        coordinator.register_app_state(attempt, |effects| {
            registrations.fetch_add(1, Ordering::SeqCst);
            effects.record_side_effect();
            false
        }),
        Err(StartupStatusError::AppStateRegistrationRejected)
    );

    assert_eq!(registrations.load(Ordering::SeqCst), 1);
    assert!(!coordinator.snapshot().app_state_ready);
    assert_eq!(coordinator.snapshot().stage, StartupStage::Failed);
    assert_eq!(
        coordinator.begin_retry(),
        Err(StartupStatusError::RetryNotAllowed),
        "a partial Tauri managed-state registration cannot be undone in-process"
    );
}

#[test]
fn first_failure_cancels_its_attempt_but_allows_clean_pre_registration_retry() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    let cancellation = coordinator
        .cancellation_token(attempt)
        .expect("current attempt token");
    let registrations = AtomicUsize::new(0);

    coordinator.fail(
        attempt,
        StartupFailureCode::AppStateConstruction,
        "construction failed",
    );

    assert!(cancellation.is_cancelled());
    assert_eq!(
        coordinator.advance(attempt, StartupStage::OpeningDatabase),
        Err(StartupStatusError::Cancelled)
    );
    assert_eq!(
        coordinator.register_app_state(attempt, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(StartupStatusError::Cancelled)
    );
    assert_eq!(registrations.load(Ordering::SeqCst), 0);
    assert!(coordinator.snapshot().retry_allowed);
    assert_eq!(coordinator.begin_retry(), Ok(attempt + 1));
}

#[test]
fn late_failure_cannot_replace_a_terminal_startup_stage() {
    for terminal_stage in [StartupStage::Ready, StartupStage::Degraded] {
        let coordinator = StartupCoordinator::new();
        let attempt = coordinator.current_attempt_id();
        complete_background_recovery(&coordinator, attempt, terminal_stage);

        coordinator.fail(
            attempt,
            StartupFailureCode::LocalRuntimeBind,
            "late failure",
        );

        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.stage, terminal_stage);
        assert_eq!(snapshot.failure_code, None);
    }

    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    coordinator.fail(
        attempt,
        StartupFailureCode::AppStateConstruction,
        "first failure",
    );
    coordinator.fail(
        attempt,
        StartupFailureCode::LocalRuntimeBind,
        "late failure",
    );

    let snapshot = coordinator.snapshot();
    assert_eq!(snapshot.stage, StartupStage::Failed);
    assert_eq!(
        snapshot.failure_code,
        Some(StartupFailureCode::AppStateConstruction)
    );
    assert_eq!(
        snapshot.diagnostic_summary.as_deref(),
        Some("first failure")
    );
}

#[test]
fn stale_attempt_cannot_publish_readiness_or_replace_registered_state() {
    let coordinator = StartupCoordinator::new();
    let first_attempt = coordinator.current_attempt_id();
    let registrations = AtomicUsize::new(0);
    coordinator.fail(
        first_attempt,
        StartupFailureCode::AppStateConstruction,
        "construction failed",
    );
    let second_attempt = coordinator.begin_retry().expect("retry should be admitted");

    assert_eq!(
        coordinator.advance(first_attempt, StartupStage::AppStateReady),
        Err(StartupStatusError::StaleAttempt)
    );
    assert_eq!(
        coordinator.register_app_state(first_attempt, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(StartupStatusError::StaleAttempt)
    );
    assert_eq!(registrations.load(Ordering::SeqCst), 0);
    assert_eq!(coordinator.snapshot().attempt_id, second_attempt);
    assert!(!coordinator.snapshot().app_state_ready);
    assert!(!coordinator.snapshot().runtime_ready);
}

#[test]
fn failure_cancels_late_effects_but_keeps_clean_pre_registration_retry_available() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    let token = coordinator
        .cancellation_token(attempt)
        .expect("current attempt token");

    coordinator.fail(
        attempt,
        StartupFailureCode::AppStateConstruction,
        "construction failed",
    );

    assert!(token.is_cancelled());
    assert_eq!(
        coordinator.advance(attempt, StartupStage::OpeningDatabase),
        Err(StartupStatusError::Cancelled)
    );
    assert!(coordinator.snapshot().retry_allowed);
    assert_eq!(
        coordinator.begin_retry(),
        Ok(attempt + 1),
        "a quiesced pre-registration failure remains retryable"
    );
}

#[test]
fn late_failure_cannot_overwrite_terminal_outcomes() {
    for terminal in [
        StartupStage::Ready,
        StartupStage::Degraded,
        StartupStage::Failed,
    ] {
        let coordinator = StartupCoordinator::new();
        let attempt = coordinator.current_attempt_id();
        if terminal == StartupStage::Failed {
            coordinator.fail(
                attempt,
                StartupFailureCode::AppStateConstruction,
                "first failure",
            );
        } else {
            advance_to_terminal_stage(&coordinator, terminal);
        }
        let before = coordinator.snapshot();

        coordinator.fail(
            attempt,
            StartupFailureCode::LocalRuntimeBind,
            "late failure",
        );

        assert_eq!(coordinator.snapshot(), before);
    }
}

#[test]
fn shutdown_cancels_attempt_and_forbids_late_effects() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    let registrations = AtomicUsize::new(0);

    coordinator.cancel();

    assert_eq!(
        coordinator.advance(attempt, StartupStage::OpeningDatabase),
        Err(StartupStatusError::Cancelled)
    );
    assert_eq!(
        coordinator.register_app_state(attempt, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Err(StartupStatusError::Cancelled)
    );
    assert_eq!(registrations.load(Ordering::SeqCst), 0);
    assert!(coordinator.is_cancelled());
    assert!(!coordinator.snapshot().runtime_ready);
}

#[test]
fn listener_binding_never_authorizes_runtime_ready_on_its_own() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    register_and_install_listeners(&coordinator, attempt);
    coordinator
        .advance(attempt, StartupStage::BindingLocalRuntime)
        .expect("binding stage");
    coordinator
        .listener_bound(attempt)
        .expect("listener-bound stage");

    let snapshot = coordinator.snapshot();
    assert_eq!(snapshot.stage, StartupStage::SafetyRecovery);
    assert!(snapshot.app_state_ready);
    assert!(!snapshot.runtime_ready);
}

#[test]
fn current_attempt_registers_state_and_listeners_exactly_once() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    let registrations = AtomicUsize::new(0);
    let listeners = AtomicUsize::new(0);

    advance_to_registering_state(&coordinator, attempt);
    coordinator
        .register_app_state(attempt, |_| {
            registrations.fetch_add(1, Ordering::SeqCst);
            true
        })
        .expect("first registration succeeds");
    coordinator
        .install_listeners(attempt, || {
            listeners.fetch_add(1, Ordering::SeqCst);
        })
        .expect("first listener installation succeeds");

    assert_eq!(
        coordinator.install_listeners(attempt, || {
            listeners.fetch_add(1, Ordering::SeqCst);
        }),
        Ok(false)
    );
    assert_eq!(registrations.load(Ordering::SeqCst), 1);
    assert_eq!(listeners.load(Ordering::SeqCst), 1);
}

#[test]
fn snapshot_uses_the_frontend_snake_case_wire_contract() {
    let coordinator = StartupCoordinator::new();
    let snapshot =
        serde_json::to_value(coordinator.snapshot()).expect("serialize startup snapshot");

    assert!(snapshot["boot_id"].as_str().is_some());
    assert_eq!(snapshot["attempt_id"], 1);
    assert_eq!(snapshot["stage"], "creating_window");
    assert!(snapshot["started_at"].as_str().is_some());
    assert!(snapshot["stage_started_at"].as_str().is_some());
    assert!(snapshot["completed_at"].is_null());
    assert_eq!(snapshot["app_state_ready"], false);
    assert_eq!(snapshot["runtime_ready"], false);
    assert_eq!(snapshot["background_complete"], false);
    assert_eq!(snapshot["retry_allowed"], false);
    assert_eq!(snapshot["retry_allowed"], false);
    assert!(snapshot["progress"].is_null());
    assert_eq!(snapshot["failure_code"], serde_json::Value::Null);
    assert_eq!(snapshot["diagnostic_summary"], serde_json::Value::Null);
}

#[test]
fn snapshot_derives_retry_allowed_from_the_current_startup_phase() {
    let pre_registration_failure = StartupCoordinator::new();
    let pre_registration_attempt = pre_registration_failure.current_attempt_id();
    pre_registration_failure.fail(
        pre_registration_attempt,
        StartupFailureCode::AppStateConstruction,
        "construction failed",
    );
    assert!(pre_registration_failure.snapshot().retry_allowed);

    let post_registration_failure = StartupCoordinator::new();
    let post_registration_attempt = post_registration_failure.current_attempt_id();
    advance_to_registering_state(&post_registration_failure, post_registration_attempt);
    post_registration_failure
        .accept_app_state_registration(post_registration_attempt, true)
        .expect("registration completed");
    post_registration_failure.fail(
        post_registration_attempt,
        StartupFailureCode::LocalRuntimeBind,
        "post-registration failure",
    );
    assert!(!post_registration_failure.snapshot().retry_allowed);
}

#[test]
fn compaction_stage_sits_between_opening_the_database_and_migrating() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    coordinator
        .advance(attempt, StartupStage::OpeningDatabase)
        .expect("opening the database");
    coordinator
        .advance(attempt, StartupStage::CompactingDatabase)
        .expect("compaction runs before migrations");
    coordinator
        .advance(attempt, StartupStage::Migrating)
        .expect("migrations follow compaction");

    assert_eq!(coordinator.snapshot().stage, StartupStage::Migrating);
}

#[test]
fn startups_without_a_compaction_still_go_straight_to_migrating() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    coordinator
        .advance(attempt, StartupStage::OpeningDatabase)
        .expect("opening the database");
    coordinator
        .advance(attempt, StartupStage::Migrating)
        .expect("the skip path must remain legal");
}

#[test]
fn the_compaction_stage_is_not_reachable_from_anywhere_else() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();

    assert!(
        coordinator
            .advance(attempt, StartupStage::CompactingDatabase)
            .is_err(),
        "compaction must not be reachable from CreatingWindow"
    );

    coordinator
        .advance(attempt, StartupStage::OpeningDatabase)
        .expect("opening the database");
    coordinator
        .advance(attempt, StartupStage::Migrating)
        .expect("skip path");
    assert!(
        coordinator
            .advance(attempt, StartupStage::CompactingDatabase)
            .is_err(),
        "compaction must not be reachable after migrations start"
    );
}
