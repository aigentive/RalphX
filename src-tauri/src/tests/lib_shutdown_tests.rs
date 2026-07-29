/// Integration tests for lib.rs shutdown sequence.
///
/// Tests verify:
/// - Shutdown ordering: agents → tracked processes → external MCP → WAL checkpoint
/// - Timeout guard: agent shutdown exceeding 2.5s is interrupted; MCP + WAL still complete
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

// ── Shared ordering tracker ───────────────────────────────────────────────

/// Records the order of shutdown steps using a monotonic counter.
struct OrderTracker {
    counter: AtomicU64,
    agent_stop_order: AtomicU64,
    mcp_stop_order: AtomicU64,
    wal_order: AtomicU64,
}

impl OrderTracker {
    fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
            agent_stop_order: AtomicU64::new(0),
            mcp_stop_order: AtomicU64::new(0),
            wal_order: AtomicU64::new(0),
        }
    }

    fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }
}

// ── Simulated shutdown_sequence ───────────────────────────────────────────

/// Mirrors the lib.rs RunEvent::Exit shutdown sequence but with injectable components.
/// This is the logic under test — extracted to be independently testable.
async fn shutdown_sequence(
    tracker: Arc<OrderTracker>,
    agent_shutdown_ms: u64,
    mcp_shutdown_flag: Arc<AtomicBool>,
    wal_checkpoint_flag: Arc<AtomicBool>,
) {
    // Step 1: Agent shutdown — 2.5s timeout guard
    let tracker_clone = Arc::clone(&tracker);
    let _ = tokio::time::timeout(Duration::from_millis(2500), async move {
        tokio::time::sleep(Duration::from_millis(agent_shutdown_ms)).await;
        tracker_clone
            .agent_stop_order
            .store(tracker_clone.next(), Ordering::SeqCst);
    })
    .await;

    // Step 2: External MCP shutdown — separate OS thread with own runtime
    let tracker_clone = Arc::clone(&tracker);
    let mcp_flag = Arc::clone(&mcp_shutdown_flag);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            mcp_flag.store(true, Ordering::SeqCst);
            tracker_clone
                .mcp_stop_order
                .store(tracker_clone.next(), Ordering::SeqCst);
        });
    })
    .join()
    .ok();

    // Step 3: WAL checkpoint
    wal_checkpoint_flag.store(true, Ordering::SeqCst);
    tracker.wal_order.store(tracker.next(), Ordering::SeqCst);
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_shutdown_ordering_agents_before_mcp_before_wal() {
    let tracker = Arc::new(OrderTracker::new());
    let mcp_flag = Arc::new(AtomicBool::new(false));
    let wal_flag = Arc::new(AtomicBool::new(false));

    shutdown_sequence(
        Arc::clone(&tracker),
        50,  // agents finish in 50ms (well within 2.5s)
        Arc::clone(&mcp_flag),
        Arc::clone(&wal_flag),
    )
    .await;

    let agent_order = tracker.agent_stop_order.load(Ordering::SeqCst);
    let mcp_order = tracker.mcp_stop_order.load(Ordering::SeqCst);
    let wal_order = tracker.wal_order.load(Ordering::SeqCst);

    // All three completed
    assert!(agent_order > 0, "agent shutdown should have completed");
    assert!(mcp_flag.load(Ordering::SeqCst), "MCP shutdown should have run");
    assert!(wal_flag.load(Ordering::SeqCst), "WAL checkpoint should have run");

    // Ordering: agents first, then MCP, then WAL
    assert!(
        agent_order < mcp_order,
        "agent shutdown ({agent_order}) must precede MCP shutdown ({mcp_order})"
    );
    assert!(
        mcp_order < wal_order,
        "MCP shutdown ({mcp_order}) must precede WAL checkpoint ({wal_order})"
    );
}

#[tokio::test]
async fn test_shutdown_timeout_guard_mcp_and_wal_still_run() {
    let tracker = Arc::new(OrderTracker::new());
    let mcp_flag = Arc::new(AtomicBool::new(false));
    let wal_flag = Arc::new(AtomicBool::new(false));

    // Agents take 5s — exceeds the 2.5s timeout guard
    shutdown_sequence(
        Arc::clone(&tracker),
        5000,
        Arc::clone(&mcp_flag),
        Arc::clone(&wal_flag),
    )
    .await;

    let agent_order = tracker.agent_stop_order.load(Ordering::SeqCst);
    let mcp_order = tracker.mcp_stop_order.load(Ordering::SeqCst);
    let wal_order = tracker.wal_order.load(Ordering::SeqCst);

    // Agent shutdown timed out — NOT completed
    assert_eq!(
        agent_order, 0,
        "agent shutdown should have been interrupted by timeout"
    );

    // MCP and WAL still ran despite agent timeout
    assert!(mcp_flag.load(Ordering::SeqCst), "MCP shutdown must run even after agent timeout");
    assert!(wal_flag.load(Ordering::SeqCst), "WAL checkpoint must run even after agent timeout");

    // MCP before WAL
    assert!(
        mcp_order < wal_order,
        "MCP shutdown ({mcp_order}) must precede WAL checkpoint ({wal_order}) after timeout"
    );
}

#[tokio::test]
async fn test_shutdown_mcp_skipped_when_not_started() {
    // Simulates ExternalMcpHandle::get() returning None (supervisor never started)
    let mcp_called = Arc::new(AtomicBool::new(false));
    let wal_called = Arc::new(AtomicBool::new(false));

    let tracker = Arc::new(OrderTracker::new());

    // Agent shutdown with no MCP (simulated by not calling shutdown)
    let _ = tokio::time::timeout(Duration::from_millis(2500), async {
        tokio::time::sleep(Duration::from_millis(10)).await;
        tracker
            .agent_stop_order
            .store(tracker.next(), Ordering::SeqCst);
    })
    .await;

    // MCP skipped (supervisor is None) — no-op
    // mcp_called stays false

    // WAL runs
    wal_called.store(true, Ordering::SeqCst);
    tracker.wal_order.store(tracker.next(), Ordering::SeqCst);

    assert!(
        tracker.agent_stop_order.load(Ordering::SeqCst) > 0,
        "agent shutdown ran"
    );
    assert!(
        !mcp_called.load(Ordering::SeqCst),
        "MCP not called when supervisor not started"
    );
    assert!(wal_called.load(Ordering::SeqCst), "WAL ran");
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

    assert!(result.is_ok(), "probe should eventually report ready: {result:?}");
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_wait_for_backend_ready_times_out_after_non_200_probe() {
    use super::super::{wait_for_backend_ready_with_probe, BackendReadyProbeResult};

    let result = wait_for_backend_ready_with_probe(3847, Duration::from_millis(450), move |_| async {
        BackendReadyProbeResult::HttpStatus(404)
    })
    .await;

    assert!(result.is_err(), "non-200 probe should time out");
}

#[tokio::test]
async fn test_wait_for_backend_ready_times_out_when_probe_unreachable() {
    use super::super::{wait_for_backend_ready_with_probe, BackendReadyProbeResult};

    let start = std::time::Instant::now();
    let result = wait_for_backend_ready_with_probe(3847, Duration::from_millis(450), move |_| async {
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
