// TeamService — wraps TeamStateTracker + AppHandle to emit events on mutations
//
// Pattern: service wrapper (like TransitionHandler). Every mutation delegates to
// TeamStateTracker then emits the corresponding team:* event via AppHandle.
// Read-only methods delegate directly without emission.
// Persistence: fire-and-forget writes to session/message repos (tracing::warn on failure).

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use tauri::AppHandle;
use tracing::warn;

use crate::domain::entities::team::{
    TeamMessageRecord, TeamSession, TeamSessionId, TeammateSnapshot,
};
use crate::domain::repositories::{TeamMessageRepository, TeamSessionRepository};

use super::team_events;
use super::team_state_tracker::{
    TeamMessage, TeamMessageResponse, TeamMessageType, TeamStateTracker, TeamStatusResponse,
    TeamTrackerError, TeammateCost, TeammateCostResponse, TeammateHandle, TeammateStatus,
};

/// Service layer wrapping TeamStateTracker with event emission.
///
/// Holds an `Arc<TeamStateTracker>` (shared state) and an optional `AppHandle`
/// for emitting Tauri events to the frontend. When `app_handle` is `None`
/// (e.g. in tests), mutations still succeed but events are silently skipped.
/// Repos are optional — when present, mutations persist to DB (fire-and-forget).
pub struct TeamService {
    tracker: Arc<TeamStateTracker>,
    app_handle: Option<AppHandle>,
    session_repo: Option<Arc<dyn TeamSessionRepository>>,
    message_repo: Option<Arc<dyn TeamMessageRepository>>,
    /// Cache: team_name → TeamSessionId (avoids repeated DB lookups)
    session_id_cache: Arc<RwLock<HashMap<String, TeamSessionId>>>,
}

impl TeamService {
    /// Create a new TeamService with event emission.
    pub fn new(tracker: Arc<TeamStateTracker>, app_handle: AppHandle) -> Self {
        Self {
            tracker,
            app_handle: Some(app_handle),
            session_repo: None,
            message_repo: None,
            session_id_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a TeamService with event emission and persistence repos.
    pub fn new_with_repos(
        tracker: Arc<TeamStateTracker>,
        app_handle: AppHandle,
        session_repo: Arc<dyn TeamSessionRepository>,
        message_repo: Arc<dyn TeamMessageRepository>,
    ) -> Self {
        Self {
            tracker,
            app_handle: Some(app_handle),
            session_repo: Some(session_repo),
            message_repo: Some(message_repo),
            session_id_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a TeamService without event emission (for tests).
    pub fn new_without_events(tracker: Arc<TeamStateTracker>) -> Self {
        Self {
            tracker,
            app_handle: None,
            session_repo: None,
            message_repo: None,
            session_id_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a TeamService with repos but without event emission (for tests).
    ///
    /// Allows testing DB persistence logic without requiring a real Tauri AppHandle.
    #[cfg(test)]
    pub fn new_with_repos_for_testing(
        tracker: Arc<TeamStateTracker>,
        session_repo: Arc<dyn crate::domain::repositories::TeamSessionRepository>,
        message_repo: Arc<dyn crate::domain::repositories::TeamMessageRepository>,
    ) -> Self {
        Self {
            tracker,
            app_handle: None,
            session_repo: Some(session_repo),
            message_repo: Some(message_repo),
            session_id_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Access the underlying tracker (for read-only delegation or handle ops).
    pub fn tracker(&self) -> &TeamStateTracker {
        &self.tracker
    }

    /// Get an Arc reference to the underlying tracker (for spawning stream processors).
    pub fn tracker_arc(&self) -> Arc<TeamStateTracker> {
        self.tracker.clone()
    }

    // ========================================================================
    // Mutation methods (tracker op + event emit)
    // ========================================================================

    /// Create a new team and emit team:created.
    pub async fn create_team(
        &self,
        name: &str,
        context_id: &str,
        context_type: &str,
    ) -> Result<(), TeamTrackerError> {
        self.tracker
            .create_team(name, context_id, context_type)
            .await?;

        // Persist session to DB
        if let Some(ref repo) = self.session_repo {
            let session = TeamSession::new(name, context_id, context_type);
            let sid = session.id.clone();
            if let Err(e) = repo.create(session).await {
                warn!("Failed to persist team session: {e}");
            } else {
                self.session_id_cache
                    .write()
                    .await
                    .insert(name.to_string(), sid);
            }
        }

        if let Some(ref handle) = self.app_handle {
            team_events::emit_team_created(handle, name, context_id, context_type);
        }
        Ok(())
    }

    /// Add a teammate and emit team:teammate_spawned.
    pub async fn add_teammate(
        &self,
        team_name: &str,
        name: &str,
        color: &str,
        model: &str,
        role: &str,
    ) -> Result<(), TeamTrackerError> {
        self.tracker
            .add_teammate(team_name, name, color, model, role)
            .await?;

        self.persist_teammates(team_name).await;

        if let Some(ref handle) = self.app_handle {
            let (ctx_type, ctx_id) = self.get_team_context(team_name).await?;
            team_events::emit_teammate_spawned(
                handle, team_name, name, color, model, role, &ctx_type, &ctx_id, None,
            );
        }
        Ok(())
    }

    /// Update teammate status and emit the corresponding status event.
    pub async fn update_teammate_status(
        &self,
        team_name: &str,
        teammate_name: &str,
        status: TeammateStatus,
    ) -> Result<(), TeamTrackerError> {
        self.tracker
            .update_teammate_status(team_name, teammate_name, status)
            .await?;

        self.persist_teammates(team_name).await;

        if let Some(ref handle) = self.app_handle {
            let (ctx_type, ctx_id) = self.get_team_context(team_name).await?;
            team_events::emit_teammate_status_change(
                handle,
                team_name,
                teammate_name,
                status,
                &ctx_type,
                &ctx_id,
            );
        }
        Ok(())
    }

    /// Update teammate cost and emit team:cost_update.
    pub async fn update_teammate_cost(
        &self,
        team_name: &str,
        teammate_name: &str,
        cost: TeammateCost,
    ) -> Result<(), TeamTrackerError> {
        let input_tokens = cost.input_tokens;
        let output_tokens = cost.output_tokens;
        let estimated_usd = cost.estimated_usd;

        self.tracker
            .update_teammate_cost(team_name, teammate_name, cost)
            .await?;

        self.persist_teammates(team_name).await;

        if let Some(ref handle) = self.app_handle {
            let (ctx_type, ctx_id) = self.get_team_context(team_name).await?;
            team_events::emit_team_cost_update(
                handle,
                team_name,
                teammate_name,
                input_tokens,
                output_tokens,
                estimated_usd,
                &ctx_type,
                &ctx_id,
            );
        }
        Ok(())
    }

    /// Send a user message and emit team:message.
    pub async fn send_user_message(
        &self,
        team_name: &str,
        content: &str,
    ) -> Result<TeamMessage, TeamTrackerError> {
        let msg = self.tracker.send_user_message(team_name, content).await?;

        self.persist_message(team_name, &msg).await;

        if let Some(ref handle) = self.app_handle {
            let (ctx_type, ctx_id) = self.get_team_context(team_name).await?;
            team_events::emit_team_message(handle, &msg, &ctx_type, &ctx_id);
        }
        Ok(msg)
    }

    /// Add a teammate message and emit team:message.
    pub async fn add_teammate_message(
        &self,
        team_name: &str,
        sender: &str,
        recipient: Option<&str>,
        content: &str,
        message_type: TeamMessageType,
    ) -> Result<TeamMessage, TeamTrackerError> {
        let msg = self
            .tracker
            .add_teammate_message(team_name, sender, recipient, content, message_type)
            .await?;

        self.persist_message(team_name, &msg).await;

        if let Some(ref handle) = self.app_handle {
            let (ctx_type, ctx_id) = self.get_team_context(team_name).await?;
            team_events::emit_team_message(handle, &msg, &ctx_type, &ctx_id);
        }
        Ok(msg)
    }

    /// Stop a specific teammate and emit team:teammate_shutdown.
    pub async fn stop_teammate(
        &self,
        team_name: &str,
        teammate_name: &str,
    ) -> Result<(), TeamTrackerError> {
        // Capture context before the mutation (team must exist)
        let ctx = if self.app_handle.is_some() {
            Some(self.get_team_context(team_name).await?)
        } else {
            None
        };

        self.tracker.stop_teammate(team_name, teammate_name).await?;

        // Persist AFTER stop so snapshot captures "shutdown" status
        self.persist_teammates(team_name).await;

        if let (Some(ref handle), Some((ctx_type, ctx_id))) = (&self.app_handle, ctx) {
            team_events::emit_teammate_shutdown(
                handle,
                team_name,
                teammate_name,
                &ctx_type,
                &ctx_id,
            );
        }
        Ok(())
    }

    /// Stop all teammates and emit per-teammate shutdown + team:disbanded events.
    pub async fn stop_team(&self, team_name: &str) -> Result<(), TeamTrackerError> {
        // Capture teammate names and context before mutation
        let (teammate_names, ctx) = if self.app_handle.is_some() {
            let status = self.tracker.get_team_status(team_name).await?;
            let names: Vec<String> = status.teammates.iter().map(|t| t.name.clone()).collect();
            let ctx = (status.context_type.clone(), status.context_id.clone());
            (names, Some(ctx))
        } else {
            (vec![], None)
        };

        self.tracker.stop_team(team_name).await?;

        // Persist AFTER stop so snapshot captures "shutdown" status (not stale "idle")
        self.persist_teammates(team_name).await;

        if let (Some(ref handle), Some((ctx_type, ctx_id))) = (&self.app_handle, ctx) {
            for name in &teammate_names {
                team_events::emit_teammate_shutdown(handle, team_name, name, &ctx_type, &ctx_id);
            }
        }
        Ok(())
    }

    /// Disband a team (stop all + mark disbanded) and emit team:disbanded.
    pub async fn disband_team(&self, team_name: &str) -> Result<(), TeamTrackerError> {
        // Capture context and teammate names before mutation for event emission
        let ctx = if self.app_handle.is_some() {
            Some(self.get_team_context(team_name).await?)
        } else {
            None
        };
        let teammate_names: Vec<String> = if self.app_handle.is_some() {
            self.tracker
                .get_team_status(team_name)
                .await
                .map(|s| s.teammates.iter().map(|t| t.name.clone()).collect())
                .unwrap_or_default()
        } else {
            vec![]
        };

        // Resolve session ID before disbanding (tracker still has team context)
        let disbanded_sid = self.resolve_session_id(team_name).await;

        self.tracker.disband_team(team_name).await?;

        // Persist AFTER disband so snapshot captures final "shutdown" status
        self.persist_teammates(team_name).await;

        // Persist disbanded state
        if let (Some(ref repo), Some(sid)) = (&self.session_repo, disbanded_sid) {
            if let Err(e) = repo.set_disbanded(&sid).await {
                warn!("Failed to persist team disbanded: {e}");
            }
        }

        if let (Some(ref handle), Some((ctx_type, ctx_id))) = (&self.app_handle, ctx) {
            // Emit per-teammate shutdown events before the team:disbanded event
            for name in &teammate_names {
                team_events::emit_teammate_shutdown(handle, team_name, name, &ctx_type, &ctx_id);
            }
            team_events::emit_team_disbanded(handle, team_name, &ctx_type, &ctx_id);
        }
        Ok(())
    }

    // ========================================================================
    // Delegate-only methods (no event emission)
    // ========================================================================

    /// Set the teammate handle (process + stream task).
    pub async fn set_teammate_handle(
        &self,
        team_name: &str,
        teammate_name: &str,
        handle: TeammateHandle,
    ) -> Result<(), TeamTrackerError> {
        self.tracker
            .set_teammate_handle(team_name, teammate_name, handle)
            .await
    }

    /// Get team status (serializable snapshot).
    pub async fn get_team_status(
        &self,
        team_name: &str,
    ) -> Result<TeamStatusResponse, TeamTrackerError> {
        self.tracker.get_team_status(team_name).await
    }

    /// Get teammate cost.
    pub async fn get_teammate_cost(
        &self,
        team_name: &str,
        teammate_name: &str,
    ) -> Result<TeammateCostResponse, TeamTrackerError> {
        self.tracker
            .get_teammate_cost(team_name, teammate_name)
            .await
    }

    /// Get team messages (serializable).
    pub async fn get_team_messages(
        &self,
        team_name: &str,
        limit: Option<usize>,
    ) -> Result<Vec<TeamMessageResponse>, TeamTrackerError> {
        self.tracker.get_team_messages(team_name, limit).await
    }

    /// List all active team names.
    pub async fn list_teams(&self) -> Vec<String> {
        self.tracker.list_teams().await
    }

    /// Check if a team exists.
    pub async fn team_exists(&self, team_name: &str) -> bool {
        self.tracker.team_exists(team_name).await
    }

    /// Disband any existing teams for the given context_id before starting a new run.
    ///
    /// Called at the start of spawn_send_message_background to ensure stale teams from
    /// a previous execution (e.g. team-mode → solo-mode switch) are cleaned up before
    /// a new agent run begins. Silently skips errors (best-effort cleanup).
    pub async fn cleanup_stale_teams_for_context(&self, context_id: &str) {
        let teams = self.tracker.list_teams().await;
        for team_name in &teams {
            if let Ok(status) = self.tracker.get_team_status(team_name).await {
                if status.context_id == context_id {
                    tracing::info!(
                        team = %team_name,
                        context_id = %context_id,
                        "Pre-spawn cleanup: disbanding stale team"
                    );
                    let _ = self.disband_team(team_name).await;
                }
            }
        }
    }

    /// Send a message to a teammate's stdin (interactive mode).
    pub async fn send_stdin_message(
        &self,
        team_name: &str,
        teammate_name: &str,
        message: &str,
    ) -> Result<(), TeamTrackerError> {
        self.tracker
            .send_stdin_message(team_name, teammate_name, message)
            .await
    }

    /// Remove a teammate from a team.
    pub async fn remove_teammate(
        &self,
        team_name: &str,
        teammate_name: &str,
    ) -> Result<(), TeamTrackerError> {
        // Capture context before mutation
        let ctx = if self.app_handle.is_some() {
            Some(self.get_team_context(team_name).await?)
        } else {
            None
        };

        self.tracker
            .remove_teammate(team_name, teammate_name)
            .await?;

        if let (Some(ref handle), Some((ctx_type, ctx_id))) = (&self.app_handle, ctx) {
            team_events::emit_teammate_shutdown(
                handle,
                team_name,
                teammate_name,
                &ctx_type,
                &ctx_id,
            );
        }
        Ok(())
    }

    /// Get teammate count for a team.
    pub async fn get_teammate_count(&self, team_name: &str) -> Result<usize, TeamTrackerError> {
        self.tracker.get_teammate_count(team_name).await
    }

    /// Find an active (non-disbanded) team by its context_id.
    pub async fn find_team_by_context_id(&self, context_id: &str) -> Option<String> {
        self.tracker.find_team_by_context_id(context_id).await
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Look up context_type and context_id from the team's TeamState.
    async fn get_team_context(
        &self,
        team_name: &str,
    ) -> Result<(String, String), TeamTrackerError> {
        let status = self.tracker.get_team_status(team_name).await?;
        Ok((status.context_type, status.context_id))
    }

    /// Get cached session ID for a team, with DB fallback on cache miss.
    ///
    /// The in-memory cache is populated by `create_team`. However, two separate
    /// `TeamService` instances share the same `TeamStateTracker` (shared Arc) —
    /// the Tauri command service and the HTTP server service. If `create_team` was
    /// called on one instance (e.g. Tauri service via chat_service_streaming), the
    /// other instance's cache is empty. This DB fallback prevents silent message loss
    /// when `persist_message` or `persist_teammates` is called on the non-creating
    /// instance (e.g. HTTP service's stream processor).
    async fn resolve_session_id(&self, team_name: &str) -> Option<TeamSessionId> {
        // Fast path: in-memory cache
        if let Some(sid) = self.session_id_cache.read().await.get(team_name).cloned() {
            return Some(sid);
        }
        // Slow path: query DB via session repo (only possible when repo is available)
        let repo = self.session_repo.as_ref()?;
        let (ctx_type, ctx_id) = self.get_team_context(team_name).await.ok()?;
        let sessions = repo.get_by_context(&ctx_type, &ctx_id).await.ok()?;
        let session = sessions.into_iter().last()?;
        let sid = session.id.clone();
        // Populate cache for future calls
        self.session_id_cache
            .write()
            .await
            .insert(team_name.to_string(), sid.clone());
        Some(sid)
    }

    /// Snapshot current teammates from tracker and persist to DB.
    async fn persist_teammates(&self, team_name: &str) {
        let repo = match self.session_repo {
            Some(ref r) => r,
            None => return,
        };
        let sid = match self.resolve_session_id(team_name).await {
            Some(s) => s,
            None => return,
        };
        // Get current teammate list from tracker
        let status = match self.tracker.get_team_status(team_name).await {
            Ok(s) => s,
            Err(_) => return,
        };
        let snapshots: Vec<TeammateSnapshot> = status
            .teammates
            .iter()
            .map(|t| TeammateSnapshot {
                name: t.name.clone(),
                color: t.color.clone(),
                model: t.model.clone(),
                role: t.role.clone(),
                status: t.status.to_string(),
                cost: t.cost.clone(),
                spawned_at: t.spawned_at.clone(),
                last_activity_at: t.last_activity_at.clone(),
                conversation_id: t.conversation_id.clone(),
            })
            .collect();
        if let Err(e) = repo.update_teammates(&sid, &snapshots).await {
            warn!("Failed to persist teammates: {e}");
        }
    }

    /// Persist a team message to DB.
    async fn persist_message(&self, team_name: &str, msg: &TeamMessage) {
        let repo = match self.message_repo {
            Some(ref r) => r,
            None => return,
        };
        let sid = match self.resolve_session_id(team_name).await {
            Some(s) => s,
            None => return,
        };
        let mut record = TeamMessageRecord::new(sid, &msg.sender, &msg.content);
        record.recipient = msg.recipient.clone();
        record.message_type = match msg.message_type {
            TeamMessageType::UserMessage => "user_message".to_string(),
            TeamMessageType::TeammateMessage => "teammate_message".to_string(),
            TeamMessageType::Broadcast => "broadcast".to_string(),
            TeamMessageType::System => "system".to_string(),
        };
        record.created_at = msg.timestamp;
        if let Err(e) = repo.create(record).await {
            warn!("Failed to persist team message: {e}");
        }
    }
}

#[cfg(test)]
#[path = "team_service_tests.rs"]
mod tests;
