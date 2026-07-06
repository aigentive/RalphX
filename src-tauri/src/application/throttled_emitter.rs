// ThrottledEmitter — coalesces batch-prone Tauri events into 100ms windows.
//
// Events like `task:status_changed` and `task:created` can fire 9+ times per second
// during rapid task scheduling. Direct emit() on each call overwhelms the WebView.
// ThrottledEmitter queues these events and flushes them every 100ms from a background task.
//
// Non-batchable events pass through immediately.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

pub struct ThrottledEmitter<R: Runtime = tauri::Wry> {
    handle: AppHandle<R>,
    pending: Mutex<Vec<(String, serde_json::Value)>>,
}

impl<R: Runtime> ThrottledEmitter<R> {
    /// Create a new ThrottledEmitter. Spawns a background task that flushes
    /// pending batchable events every 100ms. The task exits automatically when
    /// the Arc<ThrottledEmitter> is dropped (via Weak reference).
    pub fn new(handle: AppHandle<R>) -> Arc<Self> {
        let emitter = Arc::new(Self {
            handle,
            pending: Mutex::new(Vec::new()),
        });

        let weak = Arc::downgrade(&emitter);
        let handle_clone = emitter.handle.clone();
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(100));
                let Some(strong) = weak.upgrade() else {
                    break;
                };
                let events = {
                    let mut guard = strong
                        .pending
                        .lock()
                        .expect("ThrottledEmitter pending lock poisoned");
                    std::mem::take(&mut *guard)
                };
                drop(strong);
                for (event, payload) in events {
                    let _ = handle_clone.emit(&event, payload);
                }
            }
        });

        emitter
    }

    /// Emit an event. Batchable events are queued for the next 100ms flush;
    /// non-batchable events are emitted immediately.
    pub fn emit(&self, event: &str, payload: serde_json::Value) {
        if Self::is_batchable(event) {
            let mut guard = self
                .pending
                .lock()
                .expect("ThrottledEmitter pending lock poisoned");
            guard.push((event.to_string(), payload));
        } else {
            let _ = self.handle.emit(event, payload);
        }
    }

    /// Returns true for events that benefit from 100ms coalescing.
    pub fn is_batchable(event: &str) -> bool {
        matches!(event, "task:status_changed" | "task:created")
    }
}
