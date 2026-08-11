use tokio::sync::oneshot;

use super::server_boot::settle_listener_bind_handshake;
use crate::application::startup_status::{StartupCoordinator, StartupFailureCode, StartupStage};
use crate::AppError;

fn advance_to_bind(coordinator: &StartupCoordinator, attempt: u64) {
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
    coordinator
        .accept_app_state_registration(attempt, true)
        .expect("state registration");
    coordinator
        .install_listeners(attempt, || {})
        .expect("listener installation");
    coordinator
        .advance(attempt, StartupStage::BindingLocalRuntime)
        .expect("binding stage");
}

#[tokio::test]
async fn successful_listener_handshake_advances_only_the_current_attempt() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    advance_to_bind(&coordinator, attempt);

    let (sender, receiver) = oneshot::channel();
    sender.send(Ok(())).expect("listener readiness send");

    settle_listener_bind_handshake(receiver, &coordinator, attempt)
        .await
        .expect("listener handshake");

    assert_eq!(coordinator.snapshot().stage, StartupStage::SafetyRecovery);
    assert!(!coordinator.snapshot().runtime_ready);
}

#[tokio::test]
async fn failed_listener_handshake_never_publishes_runtime_readiness() {
    let coordinator = StartupCoordinator::new();
    let attempt = coordinator.current_attempt_id();
    advance_to_bind(&coordinator, attempt);

    let (sender, receiver) = oneshot::channel();
    sender
        .send(Err(AppError::Infrastructure("bind failed".to_string())))
        .expect("listener failure send");

    assert!(
        settle_listener_bind_handshake(receiver, &coordinator, attempt)
            .await
            .is_err()
    );
    let snapshot = coordinator.snapshot();
    assert_eq!(snapshot.stage, StartupStage::Failed);
    assert_eq!(
        snapshot.failure_code,
        Some(StartupFailureCode::LocalRuntimeBind)
    );
    assert!(!snapshot.runtime_ready);
    assert!(
        coordinator.begin_retry().is_err(),
        "a listener-bind failure is post-registration and cannot start another server"
    );
}
