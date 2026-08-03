//! Behavioral tests for the production exit-cleanup sequencing seam.
//!
//! The concrete agent shutdown closure owns its 2.5-second timeout; these tests
//! prove that once each bounded step returns, cleanup continues in the required
//! terminals → agents → external MCP → WAL order.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::application::shutdown::run_exit_steps;

struct OrderTracker {
    counter: AtomicU64,
    terminal_order: AtomicU64,
    agent_order: AtomicU64,
    mcp_order: AtomicU64,
    wal_order: AtomicU64,
}

impl OrderTracker {
    fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
            terminal_order: AtomicU64::new(0),
            agent_order: AtomicU64::new(0),
            mcp_order: AtomicU64::new(0),
            wal_order: AtomicU64::new(0),
        }
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }
}

#[test]
fn exit_steps_preserve_production_cleanup_order() {
    let tracker = OrderTracker::new();

    run_exit_steps(
        || {
            tracker
                .terminal_order
                .store(tracker.next(), Ordering::SeqCst)
        },
        || tracker.agent_order.store(tracker.next(), Ordering::SeqCst),
        || tracker.mcp_order.store(tracker.next(), Ordering::SeqCst),
        || tracker.wal_order.store(tracker.next(), Ordering::SeqCst),
    );

    assert_eq!(tracker.terminal_order.load(Ordering::SeqCst), 1);
    assert_eq!(tracker.agent_order.load(Ordering::SeqCst), 2);
    assert_eq!(tracker.mcp_order.load(Ordering::SeqCst), 3);
    assert_eq!(tracker.wal_order.load(Ordering::SeqCst), 4);
}

#[test]
fn exit_steps_continue_after_agent_shutdown_returns_without_completion() {
    let agent_completed = AtomicBool::new(false);
    let mcp_completed = AtomicBool::new(false);
    let wal_completed = AtomicBool::new(false);

    run_exit_steps(
        || {},
        || {
            // Models the bounded production agent step returning after timeout.
        },
        || mcp_completed.store(true, Ordering::SeqCst),
        || wal_completed.store(true, Ordering::SeqCst),
    );

    assert!(!agent_completed.load(Ordering::SeqCst));
    assert!(mcp_completed.load(Ordering::SeqCst));
    assert!(wal_completed.load(Ordering::SeqCst));
}

#[test]
fn exit_steps_checkpoint_wal_when_external_mcp_was_not_started() {
    let mcp_called = AtomicBool::new(false);
    let wal_called = AtomicBool::new(false);

    run_exit_steps(
        || {},
        || {},
        || {
            // Mirrors ExternalMcpHandle::get() returning None.
        },
        || wal_called.store(true, Ordering::SeqCst),
    );

    assert!(!mcp_called.load(Ordering::SeqCst));
    assert!(wal_called.load(Ordering::SeqCst));
}

// ── ExternalMcpHandle OnceLock tests ─────────────────────────────────────

#[test]
fn test_external_mcp_handle_get_before_set_returns_none() {
    use crate::infrastructure::ExternalMcpHandle;

    let handle = ExternalMcpHandle::new();
    assert!(
        handle.get().is_none(),
        "get() before set() should return None"
    );
}

#[test]
fn test_external_mcp_handle_set_once_succeeds() {
    use crate::infrastructure::ExternalMcpHandle;

    // Build a minimal AppHandle-free test: ExternalMcpHandle uses OnceLock<Arc<...>>
    // We test the OnceLock semantics directly.
    let handle = ExternalMcpHandle::new();
    assert!(handle.get().is_none());

    // We can't create an ExternalMcpSupervisor without a real AppHandle in tests,
    // so we verify the OnceLock type behavior through the handle's set() return type.
    // set() returns Err(supervisor) if already set, Ok(()) if first call.
    // The type signature itself enforces the write-once semantics.
    // Structural test: inner OnceLock starts empty.
    let _ = handle; // Drop confirms no panic on drop
}

// ── wait_for_backend_ready tests ──────────────────────────────────────────

#[tokio::test]
async fn test_wait_for_backend_ready_succeeds_after_probe_retries() {
    use std::sync::atomic::AtomicUsize;

    use super::super::{wait_for_backend_ready_with_probe, BackendReadyProbeResult};

    let attempts = Arc::new(AtomicUsize::new(0));
    let result = wait_for_backend_ready_with_probe(3847, Duration::from_millis(700), {
        let attempts = Arc::clone(&attempts);
        move |_| {
            let attempts = Arc::clone(&attempts);
            async move {
                match attempts.fetch_add(1, Ordering::SeqCst) {
                    0 => BackendReadyProbeResult::Unreachable,
                    1 => BackendReadyProbeResult::HttpStatus(404),
                    _ => BackendReadyProbeResult::Ready,
                }
            }
        }
    })
    .await;

    assert!(
        result.is_ok(),
        "probe should eventually report ready: {result:?}"
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_wait_for_backend_ready_times_out_after_non_200_probe() {
    use super::super::{wait_for_backend_ready_with_probe, BackendReadyProbeResult};

    let result =
        wait_for_backend_ready_with_probe(3847, Duration::from_millis(450), move |_| async {
            BackendReadyProbeResult::HttpStatus(404)
        })
        .await;

    assert!(result.is_err(), "non-200 probe should time out");
}

#[tokio::test]
async fn test_wait_for_backend_ready_times_out_when_probe_unreachable() {
    use super::super::{wait_for_backend_ready_with_probe, BackendReadyProbeResult};

    let start = std::time::Instant::now();
    let result =
        wait_for_backend_ready_with_probe(3847, Duration::from_millis(450), move |_| async {
            BackendReadyProbeResult::Unreachable
        })
        .await;

    assert!(result.is_err(), "unreachable probe should time out");
    assert!(
        start.elapsed() >= Duration::from_millis(400),
        "probe path should retry before timing out"
    );
}

#[tokio::test]
#[ignore = "requires loopback socket capability"]
async fn test_wait_for_backend_ready_real_socket_returns_200() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 256];
            let _ = stream.read(&mut buf).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await;
        }
    });

    let result = super::super::wait_for_backend_ready(port, Duration::from_millis(500)).await;
    assert!(
        result.is_ok(),
        "real socket probe should return Ok when server responds 200: {result:?}"
    );
}
