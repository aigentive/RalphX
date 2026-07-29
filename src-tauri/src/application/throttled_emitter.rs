// ThrottledEmitter — coalesces batch-prone Tauri events into 100ms windows.
//
// Events like `task:status_changed` and `task:created` can fire 9+ times per second
// during rapid task scheduling. Direct emit() on each call overwhelms the WebView.
// ThrottledEmitter queues these events and flushes them every 100ms from a background task.
//
// Non-batchable events pass through immediately.

use ralphx_events::EventSink;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct ThrottledEmitter {
    sink: Arc<dyn EventSink>,
    pending: Mutex<Vec<(String, serde_json::Value)>>,
}

impl ThrottledEmitter {
    /// Create a new ThrottledEmitter. Spawns a background task that flushes
    /// pending batchable events every 100ms. The task exits automatically when
    /// the Arc<ThrottledEmitter> is dropped (via Weak reference).
    pub fn new(sink: Arc<dyn EventSink>) -> Arc<Self> {
        let emitter = Arc::new(Self {
            sink,
            pending: Mutex::new(Vec::new()),
        });

        let weak = Arc::downgrade(&emitter);
        let sink = Arc::clone(&emitter.sink);
        thread::spawn(move || loop {
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
                sink.emit(&event, payload);
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
            self.sink.emit(event, payload);
        }
    }

    /// Returns true for events that benefit from 100ms coalescing.
    pub fn is_batchable(event: &str) -> bool {
        matches!(event, "task:status_changed" | "task:created")
    }
}
