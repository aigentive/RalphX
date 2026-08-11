use crate::domain::entities::merge_progress_event::{
    MergePhase, MergePhaseStatus, MergeProgressEvent,
};
use ralphx_events::{emit_serialized, EventSink};

pub(crate) fn validation_log_dir(task_id: &str) -> std::path::PathBuf {
    crate::utils::runtime_log_paths::merge_validation_log_dir(task_id)
}

/// Write full stdout/stderr to disk for a failed validation command.
///
/// Returns (stdout_log_path, stderr_log_path). Only writes non-empty outputs.
/// Failures are logged but don't block validation — this is best-effort.
pub(super) fn write_failure_logs(
    task_id: &str,
    command: &str,
    stdout: &str,
    stderr: &str,
) -> (Option<String>, Option<String>) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let dir = validation_log_dir(task_id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(task_id, error = %e, "Failed to create validation log dir");
        return (None, None);
    }

    // Hash command to create a unique, filesystem-safe filename
    let mut hasher = DefaultHasher::new();
    command.hash(&mut hasher);
    let cmd_hash = format!("{:016x}", hasher.finish());

    let stdout_path = if !stdout.is_empty() {
        let path = dir.join(format!("{cmd_hash}_stdout.log"));
        match std::fs::write(&path, stdout) {
            Ok(()) => Some(path.to_string_lossy().to_string()),
            Err(e) => {
                tracing::warn!(task_id, error = %e, "Failed to write stdout log");
                None
            }
        }
    } else {
        None
    };

    let stderr_path = if !stderr.is_empty() {
        let path = dir.join(format!("{cmd_hash}_stderr.log"));
        match std::fs::write(&path, stderr) {
            Ok(()) => Some(path.to_string_lossy().to_string()),
            Err(e) => {
                tracing::warn!(task_id, error = %e, "Failed to write stderr log");
                None
            }
        }
    } else {
        None
    };

    (stdout_path, stderr_path)
}

/// Clean up validation log directory for a task.
pub(crate) fn cleanup_validation_logs(task_id: &str) {
    let dir = validation_log_dir(task_id);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(task_id, error = %e, "Failed to clean up validation logs");
        } else {
            tracing::debug!(task_id, "Cleaned up validation log directory");
        }
    }
}

/// Emit a high-level merge progress event for UI display.
///
/// Note: no frontend listeners is OK for progress/validation events.
///
/// Also stores the event in the global hydration store so the frontend
/// can fetch it on mount (events fire before frontend subscribes).
pub(crate) fn emit_merge_progress(
    event_sink: Option<&dyn EventSink>,
    task_id: &str,
    phase: MergePhase,
    status: MergePhaseStatus,
    message: String,
) {
    let event = MergeProgressEvent::new(task_id.to_string(), phase, status, message);

    // Always store in hydration map (even without event_sink — backend-only merges still need hydration)
    crate::domain::entities::merge_progress_event::store_merge_progress(&event);

    if let Some(sink) = event_sink {
        if let Err(error) = emit_serialized(sink, "task:merge_progress", &event) {
            tracing::warn!(
                task_id,
                error = %error,
                "Failed to serialize merge progress event"
            );
        }
    }
}
