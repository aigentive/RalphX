//! Runs a clone job end to end: streams progress as coalesced events, then
//! records exactly one terminal outcome and emits exactly one terminal event.
//!
//! Progress and terminal state are written to the [`CloneJobRegistry`] *before*
//! the matching event is emitted, so a UI that reacts to an event and then
//! queries the registry can never see a staler answer than the event it just
//! received.

use ralphx_events::{emit_serialized, EventSink};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::application::clone_job_registry::{CloneCancelToken, CloneJobRegistry, CloneJobStatus};
use crate::application::git_service::clone::{
    CloneOutcome, ClonePhase, CloneProgress, GitCloneRequest,
};
use crate::application::GitService;
use crate::infrastructure::git_auth::inspect_repository_capability;

pub const CLONE_PROGRESS_EVENT: &str = "project:clone_progress";
pub const CLONE_COMPLETED_EVENT: &str = "project:clone_completed";
pub const CLONE_FAILED_EVENT: &str = "project:clone_failed";
pub const CLONE_CANCELLED_EVENT: &str = "project:clone_cancelled";

/// `git clone --progress` writes many updates per second. One Tauri event per
/// line floods the WebView, so ordinary updates are coalesced to this cadence.
/// Phase changes and the final update before a terminal event always get through.
const PROGRESS_COALESCE_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloneProgressEvent<'a> {
    job_id: &'a str,
    phase: ClonePhase,
    percent: Option<u8>,
    received: Option<u64>,
    total: Option<u64>,
    line: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CloneTerminalEvent<'a> {
    job_id: &'a str,
    #[serde(flatten)]
    status: &'a CloneJobStatus,
}

/// Run one clone to completion.
///
/// Never panics on emission failure: events are fire-and-forget, and the
/// registry is the authoritative record.
pub async fn run_clone_job(
    job_id: String,
    request: GitCloneRequest,
    cancel: Arc<CloneCancelToken>,
    registry: Arc<CloneJobRegistry>,
    events: Arc<dyn EventSink>,
) {
    let mut emitter = ProgressEmitter::new(&job_id, Arc::clone(&events), Arc::clone(&registry));

    let outcome = GitService::clone_repository(request, cancel.cancelled(), |progress| {
        emitter.observe(progress);
    })
    .await;

    // The last observed progress must reach the UI before the terminal event, or
    // a completed clone can appear stuck at whatever the coalescer withheld.
    emitter.flush();

    let (status, event) = match outcome {
        CloneOutcome::Completed {
            destination,
            default_branch,
        } => {
            let capability = inspect_repository_capability(&destination).await;
            (
                CloneJobStatus::Completed {
                    destination: destination.display().to_string(),
                    default_branch,
                    capability,
                },
                CLONE_COMPLETED_EVENT,
            )
        }
        CloneOutcome::Failed {
            code,
            message,
            cleaned_up,
        } => (
            CloneJobStatus::Failed {
                code: code.to_string(),
                message,
                cleaned_up,
            },
            CLONE_FAILED_EVENT,
        ),
        CloneOutcome::Cancelled { cleaned_up } => (
            CloneJobStatus::Cancelled { cleaned_up },
            CLONE_CANCELLED_EVENT,
        ),
    };

    registry.settle(&job_id, status.clone());
    emit(
        events.as_ref(),
        event,
        &CloneTerminalEvent {
            job_id: &job_id,
            status: &status,
        },
    );
}

/// Rate-limits progress events without ever hiding a phase change.
pub(crate) struct ProgressEmitter {
    job_id: String,
    events: Arc<dyn EventSink>,
    registry: Arc<CloneJobRegistry>,
    last_emitted_at: Option<Instant>,
    last_phase: Option<ClonePhase>,
    withheld: Option<CloneProgress>,
}

impl ProgressEmitter {
    pub(crate) fn new(
        job_id: &str,
        events: Arc<dyn EventSink>,
        registry: Arc<CloneJobRegistry>,
    ) -> Self {
        Self {
            job_id: job_id.to_string(),
            events,
            registry,
            last_emitted_at: None,
            last_phase: None,
            withheld: None,
        }
    }

    pub(crate) fn observe(&mut self, progress: CloneProgress) {
        self.registry
            .record_progress(&self.job_id, progress.phase, progress.percent);

        let phase_changed = self.last_phase != Some(progress.phase);
        let interval_elapsed = self
            .last_emitted_at
            .is_none_or(|at| at.elapsed() >= PROGRESS_COALESCE_INTERVAL);

        if phase_changed || interval_elapsed {
            self.emit(&progress);
            self.withheld = None;
        } else {
            self.withheld = Some(progress);
        }
    }

    pub(crate) fn flush(&mut self) {
        if let Some(progress) = self.withheld.take() {
            self.emit(&progress);
        }
    }

    fn emit(&mut self, progress: &CloneProgress) {
        self.last_phase = Some(progress.phase);
        self.last_emitted_at = Some(Instant::now());
        emit(
            self.events.as_ref(),
            CLONE_PROGRESS_EVENT,
            &CloneProgressEvent {
                job_id: &self.job_id,
                phase: progress.phase,
                percent: progress.percent,
                received: progress.received,
                total: progress.total,
                line: &progress.line,
            },
        );
    }
}

fn emit<T: Serialize>(sink: &dyn EventSink, event: &str, payload: &T) {
    if let Err(error) = emit_serialized(sink, event, payload) {
        tracing::warn!(event, error = %error, "Failed to serialize a clone event");
    }
}
