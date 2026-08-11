//! Coverage for the Tauri shutdown handler.
//!
//! The Exit-arm cleanup (agents/terminals/external MCP/WAL) needs the full
//! production AppState and is exercised end-to-end by integration tests; here
//! we cover the [`trigger_http_shutdown`] helper that the new ExitRequested
//! arm calls, since that's the pure-plumbing logic around the
//! [`HttpShutdownHandle`] managed state.
//!
//! We can't directly construct `RunEvent::ExitRequested` from tests — the
//! `api: ExitRequestApi` field has no public constructor in Tauri 2.x —
//! so the match-arm itself stays uncovered. The handler body it calls is
//! covered here.
//!
//! Also covers the Ready (no-op) arm via `handle_run_event` to prove benign
//! events don't accidentally fire shutdown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

use crate::application::shutdown::{
    handle_run_event, trigger_http_shutdown, trigger_startup_cancellation, ExitWatchdog,
};
use crate::application::startup_status::StartupCoordinator;
use crate::application::HttpShutdownHandle;
use crate::AppState;

fn build_mock_app_with_shutdown() -> (tauri::App<tauri::test::MockRuntime>, HttpShutdownHandle) {
    let handle = HttpShutdownHandle::new();
    let app = tauri::test::mock_builder()
        .manage(handle.clone())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    (app, handle)
}

#[tokio::test]
async fn trigger_fires_when_handle_is_managed() {
    let (app, handle) = build_mock_app_with_shutdown();
    let app_handle = app.handle().clone();

    // Park a waiter so we can observe whether the trigger fired.
    let waiter = handle.wait_for_shutdown();
    let task = tokio::spawn(waiter);
    tokio::time::sleep(Duration::from_millis(10)).await;

    trigger_http_shutdown(&app_handle);

    timeout(Duration::from_millis(100), task)
        .await
        .expect("waiter should resolve within 100ms after trigger")
        .expect("task panicked");
}

#[tokio::test]
async fn trigger_is_safe_when_handle_missing() {
    // App without HttpShutdownHandle registered. trigger_http_shutdown must
    // log debug and return cleanly — not panic. Covers the test/early-exit
    // branch of the helper.
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let app_handle = app.handle().clone();

    trigger_http_shutdown(&app_handle);
    // Reaching here without a panic is the assertion.
}

#[tokio::test]
async fn unrelated_run_event_is_a_no_op() {
    // Ready fires on app startup, never on shutdown — handle_run_event must
    // not trigger HTTP drain for it. Indirectly covers the catch-all `_` arm.
    let (app, handle) = build_mock_app_with_shutdown();
    let app_handle = app.handle().clone();

    let waiter = handle.wait_for_shutdown();
    let task = tokio::spawn(waiter);
    tokio::time::sleep(Duration::from_millis(10)).await;

    handle_run_event(&app_handle, &tauri::RunEvent::Ready);

    let result = timeout(Duration::from_millis(50), task).await;
    assert!(
        result.is_err(),
        "shutdown waiter should still be pending after Ready event"
    );
}

#[tokio::test]
async fn exit_event_cancels_startup_and_triggers_http_without_app_state() {
    let shutdown = HttpShutdownHandle::new();
    let coordinator = Arc::new(StartupCoordinator::new());
    let app = tauri::test::mock_builder()
        .manage(shutdown.clone())
        .manage(Arc::clone(&coordinator))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let waiter = tokio::spawn(shutdown.wait_for_shutdown());
    tokio::time::sleep(Duration::from_millis(10)).await;

    handle_run_event(app.handle(), &tauri::RunEvent::Exit);

    assert!(coordinator.is_cancelled());
    timeout(Duration::from_millis(100), waiter)
        .await
        .expect("HTTP shutdown should trigger during exit")
        .expect("shutdown waiter task panicked");
}

#[test]
fn exit_event_runs_full_cleanup_with_test_app_state() {
    let coordinator = Arc::new(StartupCoordinator::new());
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&coordinator))
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    handle_run_event(app.handle(), &tauri::RunEvent::Exit);

    assert!(coordinator.is_cancelled());
}

#[test]
fn early_shutdown_cancels_startup_before_app_state_registration() {
    let coordinator = Arc::new(StartupCoordinator::new());
    let app = tauri::test::mock_builder()
        .manage(Arc::clone(&coordinator))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");

    trigger_startup_cancellation(app.handle());

    assert!(coordinator.is_cancelled());
}

#[test]
fn exit_watchdog_fires_after_deadline() {
    let fired = Arc::new(AtomicBool::new(false));
    let fired_on_timeout = Arc::clone(&fired);
    let _watchdog = ExitWatchdog::arm_with(Duration::from_millis(50), move || {
        fired_on_timeout.store(true, Ordering::SeqCst);
    });

    std::thread::sleep(Duration::from_millis(200));

    assert!(fired.load(Ordering::SeqCst));
}

#[test]
fn exit_watchdog_disarm_prevents_fire() {
    let fired = Arc::new(AtomicBool::new(false));
    let fired_on_timeout = Arc::clone(&fired);
    let watchdog = ExitWatchdog::arm_with(Duration::from_millis(50), move || {
        fired_on_timeout.store(true, Ordering::SeqCst);
    });

    watchdog.disarm();
    std::thread::sleep(Duration::from_millis(200));

    assert!(!fired.load(Ordering::SeqCst));
}
