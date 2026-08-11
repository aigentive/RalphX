use ralphx_events::EventSink;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct TauriEventSink {
    app_handle: AppHandle,
}

impl TauriEventSink {
    pub fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

impl From<&AppHandle> for TauriEventSink {
    fn from(app_handle: &AppHandle) -> Self {
        Self::new(app_handle.clone())
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.app_handle.emit(event, payload);
    }
}
