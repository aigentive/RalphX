use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::Notify;

/// Trigger for graceful HTTP server shutdown.
///
/// Stored as Tauri managed state so the Tauri-runtime shutdown handler in
/// The shell shutdown module can fire it from `RunEvent::ExitRequested`
/// without coupling [`crate::AppState`] to the HTTP transport layer.
///
/// Once triggered, axum's `with_graceful_shutdown` stops accepting new
/// connections, closes idle keep-alive sockets, and lets in-flight requests
/// drain. This shrinks the pool of TIME_WAIT sockets the kernel must orphan
/// when the process is later killed (which is what exhausted the macOS
/// ephemeral port pool in the 2026-05-18 incident).
#[derive(Clone)]
pub struct HttpShutdownHandle {
    notify: Arc<Notify>,
    triggered: Arc<AtomicBool>,
}

impl HttpShutdownHandle {
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
            triggered: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns a future that resolves when shutdown is signaled.
    ///
    /// Pass into
    /// `axum::serve(listener, app).with_graceful_shutdown(handle.wait_for_shutdown())`.
    pub fn wait_for_shutdown(&self) -> impl std::future::Future<Output = ()> {
        let notify = Arc::clone(&self.notify);
        let triggered = Arc::clone(&self.triggered);
        async move {
            while !triggered.load(Ordering::SeqCst) {
                notify.notified().await;
            }
        }
    }

    /// Trigger graceful shutdown.
    ///
    /// Idempotent — calling multiple times is safe. Only waiters that have
    /// not yet received a notification will wake on the next call.
    pub fn trigger(&self) {
        self.triggered.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn inner(&self) -> &Self {
        self
    }
}

impl Default for HttpShutdownHandle {
    fn default() -> Self {
        Self::new()
    }
}
