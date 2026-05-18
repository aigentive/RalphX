use super::HttpShutdownHandle;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn trigger_wakes_pending_waiter() {
    let handle = HttpShutdownHandle::new();
    let waiter = handle.wait_for_shutdown();
    let task = tokio::spawn(waiter);

    // Give the waiter a moment to register on the Notify.
    tokio::time::sleep(Duration::from_millis(10)).await;
    handle.trigger();

    timeout(Duration::from_millis(100), task)
        .await
        .expect("waiter should resolve within 100ms after trigger")
        .expect("task panicked");
}

#[tokio::test]
async fn cloned_handle_shares_state() {
    let handle = HttpShutdownHandle::new();
    let other = handle.clone();

    let waiter = handle.wait_for_shutdown();
    let task = tokio::spawn(waiter);

    tokio::time::sleep(Duration::from_millis(10)).await;
    other.trigger();

    timeout(Duration::from_millis(100), task)
        .await
        .expect("waiter on cloned handle should resolve after trigger from other clone")
        .expect("task panicked");
}

#[tokio::test]
async fn trigger_is_idempotent() {
    let handle = HttpShutdownHandle::new();
    // Triggering before any waiter exists must not panic.
    handle.trigger();
    handle.trigger();
    handle.trigger();
}
