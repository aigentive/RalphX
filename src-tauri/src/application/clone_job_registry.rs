//! In-memory registry of running project clones.
//!
//! Exists because events alone cannot be trusted for terminal clone state: the
//! frontend event bus only bridges events that reach its callback, so anything
//! emitted before the listener registers is lost outright. A UI that waits for a
//! `clone_completed` it never received would spin forever. Every job's terminal
//! outcome is therefore retained here and re-readable by job id.
//!
//! Tauri-`AppState`-only by design — no MCP or HTTP caller exists today, so the
//! registry is deliberately not cloned into the HTTP object graph.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::application::git_service::clone::ClonePhase;
use crate::infrastructure::git_auth::RepositoryCapability;

/// Cooperative cancellation for one clone.
///
/// A flag plus a notify rather than a bare `Notify`, so a cancel that arrives
/// before the clone starts waiting is still observed.
#[derive(Debug, Default)]
pub struct CloneCancelToken {
    cancelled: AtomicBool,
    notify: tokio::sync::Notify,
}

impl CloneCancelToken {
    /// Request cancellation. Returns `true` only for the call that actually
    /// cancelled, so callers can emit exactly one `cancelled` event.
    pub fn cancel(&self) -> bool {
        let first = !self.cancelled.swap(true, Ordering::SeqCst);
        if first {
            self.notify.notify_waiters();
        }
        first
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve once cancellation has been requested.
    pub async fn cancelled(self: Arc<Self>) {
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return;
            }
            let notified = self.notify.notified();
            // Re-check after registering, so a cancel between the two cannot be
            // missed.
            if self.cancelled.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }
}

/// What the UI can learn about a clone job at any moment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CloneJobStatus {
    #[serde(rename_all = "camelCase")]
    Running {
        phase: Option<ClonePhase>,
        percent: Option<u8>,
    },
    #[serde(rename_all = "camelCase")]
    Completed {
        destination: String,
        default_branch: Option<String>,
        capability: RepositoryCapability,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        code: String,
        message: String,
        cleaned_up: bool,
    },
    #[serde(rename_all = "camelCase")]
    Cancelled { cleaned_up: bool },
    /// The job id is unknown or its retention window has passed. The UI must
    /// treat this as terminal and surface a retry rather than keep waiting.
    Unknown,
}

impl CloneJobStatus {
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Running { .. })
    }
}

struct CloneJobEntry {
    destination: PathBuf,
    cancel: Arc<CloneCancelToken>,
    phase: Option<ClonePhase>,
    percent: Option<u8>,
    terminal: Option<CloneJobStatus>,
    settled_at: Option<Instant>,
}

/// Registry of clone jobs, keyed by job id.
#[derive(Default)]
pub struct CloneJobRegistry {
    jobs: Mutex<HashMap<String, CloneJobEntry>>,
}

impl CloneJobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new job, or return the id of the live job already cloning into
    /// `destination`.
    ///
    /// Deduping by destination is what stops a double-click from starting two
    /// clones that would fight over the same directory.
    pub fn start(
        &self,
        job_id: String,
        destination: PathBuf,
        retention: Duration,
    ) -> CloneJobRegistration {
        let mut jobs = self.jobs.lock().expect("clone job registry poisoned");
        prune_locked(&mut jobs, retention);

        if let Some((existing_id, entry)) = jobs
            .iter()
            .find(|(_, entry)| entry.terminal.is_none() && entry.destination == destination)
        {
            return CloneJobRegistration {
                job_id: existing_id.clone(),
                cancel: Arc::clone(&entry.cancel),
                deduplicated: true,
            };
        }

        let cancel = Arc::new(CloneCancelToken::default());
        jobs.insert(
            job_id.clone(),
            CloneJobEntry {
                destination,
                cancel: Arc::clone(&cancel),
                phase: None,
                percent: None,
                terminal: None,
                settled_at: None,
            },
        );

        CloneJobRegistration {
            job_id,
            cancel,
            deduplicated: false,
        }
    }

    /// Record the latest observed progress so a late-subscribing UI can catch up.
    pub fn record_progress(&self, job_id: &str, phase: ClonePhase, percent: Option<u8>) {
        let mut jobs = self.jobs.lock().expect("clone job registry poisoned");
        if let Some(entry) = jobs.get_mut(job_id) {
            if entry.terminal.is_none() {
                entry.phase = Some(phase);
                entry.percent = percent;
            }
        }
    }

    /// Record a terminal outcome. Ignored if the job already settled, so the
    /// first terminal answer wins.
    pub fn settle(&self, job_id: &str, status: CloneJobStatus) {
        let mut jobs = self.jobs.lock().expect("clone job registry poisoned");
        if let Some(entry) = jobs.get_mut(job_id) {
            if entry.terminal.is_none() {
                entry.terminal = Some(status);
                entry.settled_at = Some(Instant::now());
            }
        }
    }

    /// Read a job's current or retained status.
    ///
    /// An unknown or expired id answers [`CloneJobStatus::Unknown`] rather than
    /// anything the UI could mistake for "still working".
    pub fn status(&self, job_id: &str, retention: Duration) -> CloneJobStatus {
        let mut jobs = self.jobs.lock().expect("clone job registry poisoned");
        prune_locked(&mut jobs, retention);

        match jobs.get(job_id) {
            Some(entry) => entry.terminal.clone().unwrap_or(CloneJobStatus::Running {
                phase: entry.phase,
                percent: entry.percent,
            }),
            None => CloneJobStatus::Unknown,
        }
    }

    /// Request cancellation. Returns `false` when the job is unknown or already
    /// settled or cancelled.
    pub fn cancel(&self, job_id: &str) -> bool {
        let jobs = self.jobs.lock().expect("clone job registry poisoned");
        match jobs.get(job_id) {
            Some(entry) if entry.terminal.is_none() => entry.cancel.cancel(),
            _ => false,
        }
    }

    /// Cancel every live job, used when the app is shutting down.
    pub fn cancel_all(&self) {
        let jobs = self.jobs.lock().expect("clone job registry poisoned");
        for entry in jobs.values() {
            if entry.terminal.is_none() {
                entry.cancel.cancel();
            }
        }
    }

    #[doc(hidden)]
    pub fn live_job_count(&self) -> usize {
        let jobs = self.jobs.lock().expect("clone job registry poisoned");
        jobs.values()
            .filter(|entry| entry.terminal.is_none())
            .count()
    }

    #[doc(hidden)]
    pub fn destination_of(&self, job_id: &str) -> Option<PathBuf> {
        let jobs = self.jobs.lock().expect("clone job registry poisoned");
        jobs.get(job_id).map(|entry| entry.destination.clone())
    }

    /// Whether a live job is already cloning into `destination`.
    pub fn has_live_job_for(&self, destination: &Path) -> bool {
        let jobs = self.jobs.lock().expect("clone job registry poisoned");
        jobs.values()
            .any(|entry| entry.terminal.is_none() && entry.destination == destination)
    }
}

/// The handle returned by [`CloneJobRegistry::start`].
pub struct CloneJobRegistration {
    pub job_id: String,
    pub cancel: Arc<CloneCancelToken>,
    /// `true` when an existing live job for the same destination was returned.
    pub deduplicated: bool,
}

fn prune_locked(jobs: &mut HashMap<String, CloneJobEntry>, retention: Duration) {
    jobs.retain(|_, entry| match entry.settled_at {
        Some(settled_at) => settled_at.elapsed() < retention,
        None => true,
    });
}
