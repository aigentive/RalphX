use crate::application::HttpShutdownHandle;
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

#[tokio::test]
async fn default_constructs_independent_handle() {
    // Default() must produce a fresh, independent Notify — not share state with
    // another instance's. This guards against an accidental `Default for Arc<Notify>`
    // that would alias all default handles together.
    let a = HttpShutdownHandle::default();
    let b = HttpShutdownHandle::default();

    let waiter_a = a.wait_for_shutdown();
    let task_a = tokio::spawn(waiter_a);

    // Trigger b — a's waiter must NOT fire.
    tokio::time::sleep(Duration::from_millis(10)).await;
    b.trigger();

    let result = timeout(Duration::from_millis(50), task_a).await;
    assert!(
        result.is_err(),
        "waiter on `a` should still be pending after triggering `b`"
    );
}
