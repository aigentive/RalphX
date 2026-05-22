// Integration tests verifying dual AppState shared state wiring.
//
// Bug prevention: lib.rs creates TWO AppState instances (Tauri + HTTP).
// In-memory state (IPR, message_queue, etc.) MUST be Arc-cloned between them.
// A recent bug had interactive_process_registry NOT shared — registrations
// on one instance were invisible to the other.

use std::sync::Arc;

use crate::application::AppState;
use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::domain::entities::ChatContextType;

/// Helper: create a real stdin pipe via `cat` subprocess for testing writes.
async fn create_test_stdin() -> (tokio::process::ChildStdin, tokio::process::Child) {
    let mut child = tokio::process::Command::new("cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn cat");
    let stdin = child.stdin.take().expect("no stdin");
    (stdin, child)
}

/// Verifies the lib.rs dual-AppState sharing pattern: IPR registered on instance A
/// must be visible on instance B when they share the same Arc<InteractiveProcessRegistry>.
#[tokio::test]
async fn test_shared_interactive_process_registry_visible_across_instances() {
    let a = AppState::new_test();
    let mut b = AppState::new_test();
    b.interactive_process_registry = Arc::clone(&a.interactive_process_registry);

    let key = InteractiveProcessKey::new("ideation", "session-shared");
    let (stdin, _child) = create_test_stdin().await;

    a.interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    assert!(
        b.interactive_process_registry.has_process(&key).await,
        "Registration on A must be visible on B when IPR is shared"
    );
}

/// Proves that without explicit sharing, two AppState instances have independent
/// registries. This is the exact bug scenario: if lib.rs forgets to clone the Arc,
/// Tauri and HTTP server operate on different HashMaps.
#[tokio::test]
async fn test_unshared_registries_are_independent() {
    let a = AppState::new_test();
    let b = AppState::new_test();

    let key = InteractiveProcessKey::new("ideation", "session-independent");
    let (stdin, _child) = create_test_stdin().await;

    a.interactive_process_registry
        .register(key.clone(), stdin)
        .await;

    assert!(
        !b.interactive_process_registry.has_process(&key).await,
        "Unshared registries must be independent — registration on A must NOT appear on B"
    );
}

/// Verifies that streaming_state_cache is shared between the two AppState instances
/// (Tauri + HTTP) using Arc::ptr_eq on the inner Arc. This prevents RC-2 (CRITICAL
/// streaming hydration failure) where the HTTP endpoint always returns empty streaming
/// data even when an agent is actively generating.
#[test]
fn test_streaming_state_cache_shared_arc() {
    let a = AppState::new_test();
    let mut b = AppState::new_test();
    // Simulate lib.rs sharing: b uses a's cache (shallow Arc clone)
    b.streaming_state_cache = a.streaming_state_cache.clone();

    assert!(
        Arc::ptr_eq(
            a.streaming_state_cache.states_arc(),
            b.streaming_state_cache.states_arc()
        ),
        "streaming_state_cache must share the same inner Arc after lib.rs sharing"
    );
}

/// Proves that without explicit sharing, two AppState instances have independent
/// streaming caches — confirms the bug scenario.
#[test]
fn test_unshared_streaming_state_cache_are_independent() {
    let a = AppState::new_test();
    let b = AppState::new_test();

    assert!(
        !Arc::ptr_eq(
            a.streaming_state_cache.states_arc(),
            b.streaming_state_cache.states_arc()
        ),
        "Unshared streaming_state_cache instances must NOT point to the same Arc"
    );
}

/// Verifies that sharing message_queue between two AppState instances allows
/// messages enqueued on one to be dequeued from the other.
#[tokio::test]
async fn test_shared_message_queue_visible_across_instances() {
    let a = AppState::new_test();
    let mut b = AppState::new_test();
    b.message_queue = Arc::clone(&a.message_queue);

    a.message_queue.queue(
        ChatContextType::Ideation,
        "test-session-mq",
        "hello from A".to_string(),
    );

    let popped = b
        .message_queue
        .pop(ChatContextType::Ideation, "test-session-mq");
    assert!(
        popped.is_some(),
        "Message queued on A must be poppable from B when queue is shared"
    );
    assert_eq!(popped.unwrap().content, "hello from A");
}
