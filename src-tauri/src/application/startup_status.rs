use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

/// The process-local startup boundary currently being restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupStage {
    CreatingWindow,
    OpeningDatabase,
    /// Only entered when a compaction will actually run: `VACUUM INTO` on a multi-GB
    /// database takes minutes, and the generic "Opening local workspace data" copy
    /// would present that as a hang.
    CompactingDatabase,
    Migrating,
    LoadingSettings,
    StartupCleanup,
    RegisteringState,
    AppStateReady,
    BindingLocalRuntime,
    SafetyRecovery,
    RuntimeReady,
    BackgroundRecovery,
    Ready,
    Degraded,
    Failed,
}

impl StartupStage {
    fn order(self) -> u8 {
        match self {
            Self::CreatingWindow => 0,
            Self::OpeningDatabase => 1,
            Self::CompactingDatabase => 2,
            Self::Migrating => 3,
            Self::LoadingSettings => 4,
            Self::StartupCleanup => 5,
            Self::RegisteringState => 6,
            Self::AppStateReady => 7,
            Self::BindingLocalRuntime => 8,
            Self::SafetyRecovery => 9,
            Self::RuntimeReady => 10,
            Self::BackgroundRecovery => 11,
            Self::Ready | Self::Degraded | Self::Failed => 12,
        }
    }

    fn allows_advance_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::CreatingWindow, Self::OpeningDatabase)
                | (Self::OpeningDatabase, Self::CompactingDatabase)
                | (Self::CompactingDatabase, Self::Migrating)
                // Kept: the skip path, when no compaction runs.
                | (Self::OpeningDatabase, Self::Migrating)
                | (Self::Migrating, Self::LoadingSettings)
                | (Self::LoadingSettings, Self::StartupCleanup)
                | (Self::StartupCleanup, Self::RegisteringState)
                | (Self::AppStateReady, Self::BindingLocalRuntime)
                | (Self::RuntimeReady, Self::BackgroundRecovery)
                | (Self::BackgroundRecovery, Self::Ready | Self::Degraded)
        )
    }

    fn is_terminal(self) -> bool {
        matches!(self, Self::Ready | Self::Degraded | Self::Failed)
    }
}

/// Stable startup failures that are safe to show to the bootstrap surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFailureCode {
    AppStateConstruction,
    AppStateRegistration,
    /// The workspace itself is fine; the machine ran out of room to upgrade it.
    /// Split from `AppStateConstruction` because the user can act on it.
    InsufficientDiskSpace,
    LocalRuntimeBind,
    SafetyRecovery,
    BootstrapCancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupFailure {
    pub code: StartupFailureCode,
    pub diagnostic_summary: String,
}

/// Typed process-local snapshot consumed by the bootstrap UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StartupSnapshot {
    pub boot_id: String,
    pub attempt_id: u64,
    pub stage: StartupStage,
    pub started_at: String,
    pub stage_started_at: String,
    pub completed_at: Option<String>,
    pub app_state_ready: bool,
    pub runtime_ready: bool,
    pub background_complete: bool,
    pub retry_allowed: bool,
    pub progress: Option<StartupProgress>,
    pub message_code: String,
    pub failure_code: Option<StartupFailureCode>,
    pub diagnostic_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StartupProgress {
    pub completed_units: u32,
    pub total_units: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFrontendMilestone {
    ShellPainted,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum StartupStatusError {
    #[error("startup attempt is stale")]
    StaleAttempt,
    #[error("startup attempt has been cancelled")]
    Cancelled,
    #[error("startup stage transition is invalid")]
    InvalidTransition,
    #[error("startup stage cannot move backwards")]
    StageRegression,
    #[error("AppState registration was rejected")]
    AppStateRegistrationRejected,
    #[error("AppState has already been registered")]
    AppStateAlreadyRegistered,
    #[error("startup retry is not valid for the current phase")]
    RetryNotAllowed,
}

/// Tracks successful process-global `manage` calls made during one dynamic
/// AppState registration attempt. Tauri cannot unregister these values, so a
/// failed registration after any effect requires a fresh process.
#[derive(Debug, Default)]
pub(crate) struct StartupRegistrationEffects {
    side_effects_started: bool,
}

impl StartupRegistrationEffects {
    pub(crate) fn record_side_effect(&mut self) {
        self.side_effects_started = true;
    }
}

#[derive(Debug)]
struct StartupState {
    boot_id: String,
    attempt_id: u64,
    stage: StartupStage,
    started_at: String,
    stage_started_at: String,
    completed_at: Option<String>,
    app_state_ready: bool,
    runtime_ready: bool,
    background_complete: bool,
    progress: Option<StartupProgress>,
    shell_painted: bool,
    cancelled: bool,
    registration_started: bool,
    registration_side_effects: bool,
    listeners_installed: bool,
    listener_bound: bool,
    safety_barrier_complete: bool,
    message_code: String,
    failure: Option<StartupFailure>,
    cancellation: CancellationToken,
}

impl StartupState {
    fn new(boot_id: String, attempt_id: u64) -> Self {
        Self {
            boot_id,
            attempt_id,
            stage: StartupStage::CreatingWindow,
            started_at: startup_timestamp(),
            stage_started_at: startup_timestamp(),
            completed_at: None,
            app_state_ready: false,
            runtime_ready: false,
            background_complete: false,
            progress: None,
            shell_painted: false,
            cancelled: false,
            registration_started: false,
            registration_side_effects: false,
            listeners_installed: false,
            listener_bound: false,
            safety_barrier_complete: false,
            message_code: message_code(StartupStage::CreatingWindow).to_string(),
            failure: None,
            cancellation: CancellationToken::new(),
        }
    }

    fn snapshot(&self) -> StartupSnapshot {
        StartupSnapshot {
            boot_id: self.boot_id.clone(),
            attempt_id: self.attempt_id,
            stage: self.stage,
            started_at: self.started_at.clone(),
            stage_started_at: self.stage_started_at.clone(),
            completed_at: self.completed_at.clone(),
            app_state_ready: self.app_state_ready,
            runtime_ready: self.runtime_ready,
            background_complete: self.background_complete,
            retry_allowed: self.retry_allowed(),
            progress: self.progress.clone(),
            message_code: self.message_code.clone(),
            failure_code: self.failure.as_ref().map(|failure| failure.code),
            diagnostic_summary: self
                .failure
                .as_ref()
                .map(|failure| failure.diagnostic_summary.clone()),
        }
    }

    fn assert_current(&self, attempt_id: u64) -> Result<(), StartupStatusError> {
        if attempt_id != self.attempt_id {
            return Err(StartupStatusError::StaleAttempt);
        }
        if self.cancelled || self.cancellation.is_cancelled() {
            return Err(StartupStatusError::Cancelled);
        }
        Ok(())
    }

    fn set_stage(&mut self, stage: StartupStage) {
        self.stage = stage;
        if stage != StartupStage::Migrating {
            self.progress = None;
        }
        self.stage_started_at = startup_timestamp();
        if stage.is_terminal() {
            self.completed_at = Some(self.stage_started_at.clone());
        }
        self.message_code = message_code(stage).to_string();
    }

    fn retry_allowed(&self) -> bool {
        !self.cancelled
            && self.stage == StartupStage::Failed
            && !self.registration_started
            && !self.registration_side_effects
            && !self.app_state_ready
            && !self.listeners_installed
            && !self.listener_bound
            && !self.safety_barrier_complete
    }
}

fn startup_timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn message_code(stage: StartupStage) -> &'static str {
    match stage {
        StartupStage::CreatingWindow => "startup_creating_window",
        StartupStage::OpeningDatabase | StartupStage::Migrating => "startup_upgrading_workspace",
        StartupStage::CompactingDatabase => "startup_compacting_database",
        StartupStage::LoadingSettings => "startup_loading_settings",
        StartupStage::StartupCleanup => "startup_cleaning_previous_session",
        StartupStage::RegisteringState | StartupStage::AppStateReady => "startup_preparing_app",
        StartupStage::BindingLocalRuntime | StartupStage::SafetyRecovery => {
            "startup_restoring_local_services"
        }
        StartupStage::RuntimeReady | StartupStage::BackgroundRecovery => {
            "startup_restoring_interrupted_work"
        }
        StartupStage::Ready => "startup_ready",
        StartupStage::Degraded => "startup_degraded",
        StartupStage::Failed => "startup_failed",
    }
}

/// Single-writer authority for one RalphX process boot.
#[derive(Debug)]
pub struct StartupCoordinator {
    state: Mutex<StartupState>,
}

impl Default for StartupCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

impl StartupCoordinator {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(StartupState::new(uuid::Uuid::new_v4().to_string(), 1)),
        }
    }

    pub fn current_attempt_id(&self) -> u64 {
        self.lock().attempt_id
    }

    pub fn snapshot(&self) -> StartupSnapshot {
        self.lock().snapshot()
    }

    pub fn can_retry(&self) -> bool {
        self.lock().retry_allowed()
    }

    pub fn is_cancelled(&self) -> bool {
        let state = self.lock();
        state.cancelled || state.cancellation.is_cancelled()
    }

    pub fn ensure_current(&self, attempt_id: u64) -> Result<(), StartupStatusError> {
        self.lock().assert_current(attempt_id)
    }

    pub fn cancellation_token(
        &self,
        attempt_id: u64,
    ) -> Result<CancellationToken, StartupStatusError> {
        let state = self.lock();
        state.assert_current(attempt_id)?;
        Ok(state.cancellation.clone())
    }

    /// Advances only through the ordinary legal startup edges. Registration,
    /// listener bind, and runtime readiness are dedicated transitions below.
    pub fn advance(&self, attempt_id: u64, stage: StartupStage) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if !state.stage.allows_advance_to(stage) {
            return Err(if stage.order() <= state.stage.order() {
                StartupStatusError::StageRegression
            } else {
                StartupStatusError::InvalidTransition
            });
        }
        state.set_stage(stage);
        if matches!(stage, StartupStage::Ready | StartupStage::Degraded) {
            state.background_complete = true;
        }
        Ok(())
    }

    pub fn report_progress(
        &self,
        attempt_id: u64,
        completed_units: u32,
        total_units: u32,
    ) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if state.stage != StartupStage::Migrating || completed_units > total_units {
            return Err(StartupStatusError::InvalidTransition);
        }
        if let Some(current) = state.progress.as_ref() {
            if current.total_units != total_units {
                return Err(StartupStatusError::InvalidTransition);
            }
            if completed_units < current.completed_units {
                return Err(StartupStatusError::StageRegression);
            }
        }
        state.progress = Some(StartupProgress {
            completed_units,
            total_units,
        });
        Ok(())
    }

    /// Accepts the sole dynamic AppState registration while holding startup
    /// authority so shutdown cannot interleave a late registration.
    pub(crate) fn register_app_state(
        &self,
        attempt_id: u64,
        register: impl FnOnce(&mut StartupRegistrationEffects) -> bool,
    ) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if state.registration_started || state.app_state_ready {
            return Err(StartupStatusError::AppStateAlreadyRegistered);
        }
        if state.stage != StartupStage::RegisteringState {
            return Err(StartupStatusError::InvalidTransition);
        }
        state.registration_started = true;
        let mut effects = StartupRegistrationEffects::default();
        if !register(&mut effects) {
            state.registration_side_effects |= effects.side_effects_started;
            state.set_stage(StartupStage::Failed);
            state.failure = Some(StartupFailure {
                code: StartupFailureCode::AppStateRegistration,
                diagnostic_summary: "RalphX could not register its application state.".to_string(),
            });
            state.cancellation.cancel();
            return Err(StartupStatusError::AppStateRegistrationRejected);
        }

        state.registration_side_effects |= effects.side_effects_started;
        state.app_state_ready = true;
        state.set_stage(StartupStage::AppStateReady);
        Ok(())
    }

    pub fn accept_app_state_registration(
        &self,
        attempt_id: u64,
        accepted: bool,
    ) -> Result<(), StartupStatusError> {
        self.register_app_state(attempt_id, |_| accepted)
    }

    pub fn listener_bound(&self, attempt_id: u64) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if state.stage != StartupStage::BindingLocalRuntime || !state.listeners_installed {
            return Err(StartupStatusError::InvalidTransition);
        }
        state.listener_bound = true;
        state.set_stage(StartupStage::SafetyRecovery);
        Ok(())
    }

    /// Records that the caller-owned safety barrier completed. The subsequent
    /// runtime-ready transition stays unavailable until this acknowledgement.
    pub fn complete_safety_barrier(&self, attempt_id: u64) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if state.stage != StartupStage::SafetyRecovery
            || !state.listener_bound
            || state.safety_barrier_complete
        {
            return Err(StartupStatusError::InvalidTransition);
        }
        state.safety_barrier_complete = true;
        Ok(())
    }

    /// Publishes runtime readiness only after the complete startup boundary is
    /// recorded. Later safety/recovery work plugs in before the barrier above.
    pub fn publish_runtime_ready(&self, attempt_id: u64) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if state.stage != StartupStage::SafetyRecovery
            || !state.app_state_ready
            || !state.listeners_installed
            || !state.listener_bound
            || !state.safety_barrier_complete
        {
            return Err(StartupStatusError::InvalidTransition);
        }
        state.runtime_ready = true;
        state.set_stage(StartupStage::RuntimeReady);
        Ok(())
    }

    /// Accepts the frontend handoff only for the current runtime-ready boot.
    pub fn accept_shell_paint(
        &self,
        boot_id: &str,
        attempt_id: u64,
    ) -> Result<(), StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if state.boot_id != boot_id || !state.runtime_ready {
            return Err(StartupStatusError::InvalidTransition);
        }
        state.shell_painted = true;
        Ok(())
    }

    pub fn accept_frontend_milestone(
        &self,
        boot_id: &str,
        attempt_id: u64,
        milestone: StartupFrontendMilestone,
    ) -> Result<(), StartupStatusError> {
        match milestone {
            StartupFrontendMilestone::ShellPainted => self.accept_shell_paint(boot_id, attempt_id),
        }
    }

    /// Installs post-registration listeners exactly once while holding startup
    /// authority, so cancellation cannot interleave a late listener effect.
    pub fn install_listeners(
        &self,
        attempt_id: u64,
        install: impl FnOnce(),
    ) -> Result<bool, StartupStatusError> {
        let mut state = self.lock();
        state.assert_current(attempt_id)?;
        if !state.app_state_ready || state.stage != StartupStage::AppStateReady {
            return Err(StartupStatusError::InvalidTransition);
        }
        if state.listeners_installed {
            return Ok(false);
        }
        state.listeners_installed = true;
        install();
        Ok(true)
    }

    pub fn fail(&self, attempt_id: u64, code: StartupFailureCode, summary: impl Into<String>) {
        let mut state = self.lock();
        if attempt_id != state.attempt_id || state.cancelled || state.stage.is_terminal() {
            return;
        }
        state.set_stage(StartupStage::Failed);
        state.failure = Some(StartupFailure {
            code,
            diagnostic_summary: summary.into(),
        });
        state.cancellation.cancel();
    }

    /// Cancels the active boot before optional AppState/HTTP cleanup runs.
    pub fn cancel(&self) {
        let mut state = self.lock();
        if state.cancelled {
            return;
        }
        state.cancelled = true;
        state.cancellation.cancel();
    }

    /// Starts a new pre-registration attempt after a terminal failure.
    pub fn begin_retry(&self) -> Result<u64, StartupStatusError> {
        let mut state = self.lock();
        if !state.retry_allowed() {
            return Err(StartupStatusError::RetryNotAllowed);
        }
        state.cancellation.cancel();
        let next_attempt = state.attempt_id.saturating_add(1);
        let boot_id = state.boot_id.clone();
        *state = StartupState::new(boot_id, next_attempt);
        Ok(next_attempt)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, StartupState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Handle used to (re)launch a startup attempt.
///
/// The shell composition root builds the closure during `run_app_setup` and
/// registers this as Tauri managed state; `commands::startup_commands` invokes
/// it for user-triggered retries. It lives in `application` so that neither
/// side needs an upward import of the shell layer.
pub struct StartupAttemptLauncher {
    launch: Arc<dyn Fn(u64) + Send + Sync>,
}

impl StartupAttemptLauncher {
    pub fn new(launch: Arc<dyn Fn(u64) + Send + Sync>) -> Self {
        Self { launch }
    }

    pub fn launch(&self, attempt_id: u64) {
        (self.launch)(attempt_id);
    }
}
