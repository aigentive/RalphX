//! Test harness for the agent-workspace completion dispatch fan-out.
//!
//! Lives in the shell because it builds a real Tauri completion runtime from
//! an `AppHandle`. Keeping it in `commands` would force a `crate::shell`
//! import from a lower layer.

use std::sync::Arc;

use ralphx_events::EventSink;
use serde_json::Value;
use tauri::{Listener, Runtime};

use crate::application::completion_correlation::CompletionCorrelationRegistry;
use crate::commands::agent_workspace_completion_dispatch::{
    install_completion_dispatch_with_consumers, CompletionConsumers,
};
use crate::shell::agent_completion_event_runtime::create_agent_completion_event_runtime;

#[doc(hidden)]
pub struct AgentWorkspaceCompletionDispatchTestHandle<R: Runtime> {
    app_handle: tauri::AppHandle<R>,
    listener_ids: Vec<tauri::EventId>,
    bus_task: tauri::async_runtime::JoinHandle<()>,
    sink: Arc<dyn EventSink>,
    correlation: Arc<CompletionCorrelationRegistry>,
    review_count: Arc<std::sync::atomic::AtomicUsize>,
    publish_count: Arc<std::sync::atomic::AtomicUsize>,
    supervision_count: Arc<std::sync::atomic::AtomicUsize>,
}

impl<R: Runtime> AgentWorkspaceCompletionDispatchTestHandle<R> {
    #[doc(hidden)]
    pub fn emit(&self, event_name: &str, payload: Value) {
        self.sink.emit(event_name, payload);
    }

    #[doc(hidden)]
    pub fn observed_fanout_counts(&self) -> (usize, usize, usize) {
        use std::sync::atomic::Ordering;

        (
            self.review_count.load(Ordering::SeqCst),
            self.publish_count.load(Ordering::SeqCst),
            self.supervision_count.load(Ordering::SeqCst),
        )
    }

    /// A zero count proves both the Tauri callback and bus subscriber consumed
    /// the same reserved completion identity.
    #[doc(hidden)]
    pub fn pending_completion_correlations(&self) -> usize {
        self.correlation.len()
    }

    #[doc(hidden)]
    pub async fn shutdown(self) {
        for listener_id in self.listener_ids {
            self.app_handle.unlisten(listener_id);
        }
        self.bus_task.abort();
        let _ = self.bus_task.await;
    }
}

#[doc(hidden)]
pub fn install_agent_workspace_completion_dispatch_for_test<R: Runtime>(
    app_handle: tauri::AppHandle<R>,
) -> AgentWorkspaceCompletionDispatchTestHandle<R> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let runtime = create_agent_completion_event_runtime(app_handle.clone());
    let review_count = Arc::new(AtomicUsize::new(0));
    let publish_count = Arc::new(AtomicUsize::new(0));
    let supervision_count = Arc::new(AtomicUsize::new(0));
    let review_counter = Arc::clone(&review_count);
    let publish_counter = Arc::clone(&publish_count);
    let supervision_counter = Arc::clone(&supervision_count);
    let consumers = CompletionConsumers::new(
        Arc::new(move |_event| {
            review_counter.fetch_add(1, Ordering::SeqCst);
        }),
        Arc::new(move |_event| {
            publish_counter.fetch_add(1, Ordering::SeqCst);
        }),
        Arc::new(move |_event| {
            supervision_counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let correlation = Arc::clone(&runtime.correlation);
    let (listener_ids, bus_task) = install_completion_dispatch_with_consumers(
        &app_handle,
        runtime.bus,
        runtime.correlation,
        consumers,
    );
    AgentWorkspaceCompletionDispatchTestHandle {
        app_handle,
        listener_ids,
        bus_task,
        sink: runtime.sink,
        correlation,
        review_count,
        publish_count,
        supervision_count,
    }
}
