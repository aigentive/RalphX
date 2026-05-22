// TeamStateTracker — tracks active teams, teammates, and their state
//
// Thread-safe service for managing agent team lifecycle:
// - Create/disband teams
// - Add/remove teammates
// - Track teammate status (Spawning → Running → Idle → Shutdown)
// - Store team messages for frontend consumption
// - Track cost per teammate

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::ChildStdin;
use tokio::sync::{oneshot, watch, RwLock};
use tokio::task::JoinHandle;

pub use crate::domain::entities::TeammateCost;

// ============================================================================
// Types
// ============================================================================

/// Status of a teammate agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeammateStatus {
    Spawning,
    Running,
    Idle,
    Completed,
    Failed,
    Shutdown,
}

impl std::fmt::Display for TeammateStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeammateStatus::Spawning => write!(f, "spawning"),
            TeammateStatus::Running => write!(f, "running"),
            TeammateStatus::Idle => write!(f, "idle"),
            TeammateStatus::Completed => write!(f, "completed"),
            TeammateStatus::Failed => write!(f, "failed"),
            TeammateStatus::Shutdown => write!(f, "shutdown"),
        }
    }
}

/// Handle to a spawned teammate process (not serializable).
///
/// Owns the kill channel and stdin pipe for the teammate's CLI process.
/// The process itself is managed by a background monitoring task that owns
/// the `Child` and signals this handle via `exit_signal` when the process exits.
///
/// This design prevents grandchild processes (e.g., Node.js MCP server) from
/// holding the stdout pipe open after Claude exits: the monitoring task detects
/// Claude's exit via `child.wait()` and signals the stream processor to stop,
/// regardless of whether the pipe has reached EOF.
pub struct TeammateHandle {
    /// Send `()` to request process termination (triggers `child.kill()` in monitor task).
    pub kill_tx: Option<oneshot::Sender<()>>,
    pub stream_task: Option<JoinHandle<()>>,
    /// Explicit stdin pipe for interactive mode messaging.
    /// When set, messages can be written directly to the teammate's stdin.
    pub stdin: Option<ChildStdin>,
    /// PID of the spawned process for debug display.
    pub child_pid: Option<u32>,
}

impl TeammateHandle {
    /// Write a message to the teammate's stdin pipe (interactive mode).
    ///
    /// Returns an error if the teammate has no stdin pipe or the write fails.
    pub async fn write_message(&mut self, message: &str) -> Result<(), std::io::Error> {
        if let Some(ref mut stdin) = self.stdin {
            stdin.write_all(message.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "Teammate stdin not available (non-interactive mode)",
            ))
        }
    }

    /// Check if this handle supports interactive messaging
    pub fn is_interactive(&self) -> bool {
        self.stdin.is_some()
    }
}

impl std::fmt::Debug for TeammateHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeammateHandle")
            .field("child_pid", &self.child_pid)
            .field("interactive", &self.stdin.is_some())
            .finish()
    }
}

/// State of a single teammate
pub struct TeammateState {
    pub name: String,
    pub color: String,
    pub model: String,
    pub role: String,
    pub status: TeammateStatus,
    pub cost: TeammateCost,
    pub handle: Option<TeammateHandle>,
    pub spawned_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    /// Conversation ID for this teammate's chat history (set by stream processor)
    pub conversation_id: Option<String>,
}

impl std::fmt::Debug for TeammateState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeammateState")
            .field("name", &self.name)
            .field("color", &self.color)
            .field("model", &self.model)
            .field("role", &self.role)
            .field("status", &self.status)
            .field("cost", &self.cost)
            .field("has_handle", &self.handle.is_some())
            .field("conversation_id", &self.conversation_id)
            .finish()
    }
}

/// A message in the team conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMessage {
    pub id: String,
    pub team_name: String,
    pub sender: String,
    pub recipient: Option<String>,
    pub content: String,
    pub message_type: TeamMessageType,
    pub timestamp: DateTime<Utc>,
}

/// Type of team message
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamMessageType {
    UserMessage,
    TeammateMessage,
    Broadcast,
    System,
}

/// State of an active team
pub struct TeamState {
    pub name: String,
    pub context_id: String,
    pub context_type: String,
    pub lead_name: Option<String>,
    pub teammates: HashMap<String, TeammateState>,
    pub messages: Vec<TeamMessage>,
    pub created_at: DateTime<Utc>,
    pub phase: TeamPhase,
}

impl std::fmt::Debug for TeamState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamState")
            .field("name", &self.name)
            .field("context_id", &self.context_id)
            .field("lead_name", &self.lead_name)
            .field("teammate_count", &self.teammates.len())
            .field("message_count", &self.messages.len())
            .field("phase", &self.phase)
            .finish()
    }
}

/// Phase of the team lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamPhase {
    Forming,
    Active,
    Winding,
    Disbanded,
}

impl std::fmt::Display for TeamPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TeamPhase::Forming => write!(f, "forming"),
            TeamPhase::Active => write!(f, "active"),
            TeamPhase::Winding => write!(f, "winding"),
            TeamPhase::Disbanded => write!(f, "disbanded"),
        }
    }
}

// ============================================================================
// Serializable response types (for IPC)
// ============================================================================

/// Serializable snapshot of teammate state
#[derive(Debug, Clone, Serialize)]
pub struct TeammateStatusResponse {
    pub name: String,
    pub color: String,
    pub model: String,
    pub role: String,
    pub status: TeammateStatus,
    pub cost: TeammateCost,
    pub spawned_at: String,
    pub last_activity_at: String,
    /// Conversation ID for this teammate's persisted chat history
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
}

/// Serializable snapshot of team state
#[derive(Debug, Clone, Serialize)]
pub struct TeamStatusResponse {
    pub name: String,
    pub context_id: String,
    pub context_type: String,
    pub lead_name: Option<String>,
    pub teammates: Vec<TeammateStatusResponse>,
    pub phase: TeamPhase,
    pub created_at: String,
    pub message_count: usize,
}

/// Serializable team message for IPC
#[derive(Debug, Clone, Serialize)]
pub struct TeamMessageResponse {
    pub id: String,
    pub sender: String,
    pub recipient: Option<String>,
    pub content: String,
    pub message_type: TeamMessageType,
    pub timestamp: String,
}

/// Serializable cost response
#[derive(Debug, Clone, Serialize)]
pub struct TeammateCostResponse {
    pub teammate_name: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub estimated_usd: f64,
}

// ============================================================================
// TeamStateTracker
// ============================================================================

/// Decision result for a plan approval (sent through the watch channel)
#[derive(Debug, Clone, Serialize)]
pub struct PlanDecision {
    pub approved: bool,
    pub team_name: Option<String>,
    pub teammates_spawned: Vec<PlanDecisionTeammate>,
    pub message: String,
}

/// Spawned teammate info included in a plan decision
#[derive(Debug, Clone, Serialize)]
pub struct PlanDecisionTeammate {
    pub name: String,
    pub role: String,
    pub model: String,
    pub color: String,
}

/// A validated team plan pending user approval
#[derive(Debug, Clone)]
pub struct PendingTeamPlan {
    pub plan_id: String,
    pub context_type: String,
    pub context_id: String,
    pub process: String,
    pub teammates: Vec<PendingTeammate>,
    pub created_at: DateTime<Utc>,
    /// Team name from the lead agent's TeamCreate call.
    /// Used by approve_team_plan to ensure teammates join the correct team registry.
    pub team_name: String,
    /// Lead agent's Claude Code session ID.
    /// When present, used as parent-session-id for teammate spawns.
    pub lead_session_id: Option<String>,
}

/// A teammate in a pending plan (carries full spawn data)
#[derive(Debug, Clone)]
pub struct PendingTeammate {
    pub role: String,
    pub prompt: String,
    pub tools: Vec<String>,
    pub mcp_tools: Vec<String>,
    pub model: String,
    pub preset: Option<String>,
}

/// Thread-safe tracker for active agent teams
#[derive(Clone)]
pub struct TeamStateTracker {
    teams: Arc<RwLock<HashMap<String, TeamState>>>,
    pending_plans: Arc<RwLock<HashMap<String, PendingTeamPlan>>>,
    /// Watch channels for blocking plan approval — plan_id → (sender, created_at)
    plan_channels: Arc<RwLock<HashMap<String, (watch::Sender<Option<PlanDecision>>, std::time::Instant)>>>,
}

impl std::fmt::Debug for TeamStateTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamStateTracker")
            .field("teams", &self.teams)
            .field("pending_plans", &self.pending_plans)
            .field("plan_channels_count", &"<dynamic>")
            .finish()
    }
}

impl Default for TeamStateTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TeamStateTracker {
    pub fn new() -> Self {
        Self {
            teams: Arc::new(RwLock::new(HashMap::new())),
            pending_plans: Arc::new(RwLock::new(HashMap::new())),
            plan_channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store a validated team plan pending user approval.
    ///
    /// Safety invariants enforced on every write:
    /// 1. Single-plan-per-context: cancels any existing pending plan for the same `context_id`.
    /// 2. Plan GC: removes stale plans older than 30 minutes.
    /// 3. Channel GC: removes watch channels older than 20 minutes and channels belonging
    ///    to plans that were just cancelled (piggyback sweep, avoids a separate GC task).
    pub async fn store_pending_plan(&self, plan: PendingTeamPlan) {
        // Phase 1: update pending plans, collect IDs of old plans for the same context.
        let superseded_ids: Vec<String> = {
            let mut plans = self.pending_plans.write().await;
            // Cleanup stale plans (older than 30 minutes)
            let stale_cutoff = Utc::now() - chrono::Duration::minutes(30);
            plans.retain(|_id, p| p.created_at > stale_cutoff);
            // Single-plan-per-context: collect and remove old plans for this context_id.
            let superseded: Vec<String> = plans
                .values()
                .filter(|p| p.context_id == plan.context_id && p.plan_id != plan.plan_id)
                .map(|p| p.plan_id.clone())
                .collect();
            for id in &superseded {
                plans.remove(id);
            }
            plans.insert(plan.plan_id.clone(), plan);
            superseded
        };

        // Phase 2: sweep plan_channels — remove GC'd entries and cancelled-plan channels.
        {
            let mut channels = self.plan_channels.write().await;
            let now = std::time::Instant::now();
            channels.retain(|id, (_tx, created_at)| {
                if superseded_ids.contains(id) {
                    return false; // cancel superseded plan's channel
                }
                now.duration_since(*created_at) < std::time::Duration::from_secs(20 * 60)
            });
        }
    }

    /// Take a pending plan by ID (removes it from the store)
    pub async fn take_pending_plan(&self, plan_id: &str) -> Option<PendingTeamPlan> {
        let mut plans = self.pending_plans.write().await;
        plans.remove(plan_id)
    }

    /// Register a watch channel for plan approval (returns receiver for long-polling)
    pub async fn register_plan_channel(
        &self,
        plan_id: &str,
    ) -> watch::Receiver<Option<PlanDecision>> {
        let (tx, rx) = watch::channel(None);
        let mut channels = self.plan_channels.write().await;
        channels.insert(plan_id.to_string(), (tx, std::time::Instant::now()));
        rx
    }

    /// Check whether a plan channel exists (used for approve-at-timeout guard).
    pub async fn plan_channel_exists(&self, plan_id: &str) -> bool {
        let channels = self.plan_channels.read().await;
        channels.contains_key(plan_id)
    }

    /// Subscribe to a plan channel for long-polling (returns None if channel not found).
    pub async fn subscribe_plan_channel(
        &self,
        plan_id: &str,
    ) -> Option<watch::Receiver<Option<PlanDecision>>> {
        let channels = self.plan_channels.read().await;
        channels.get(plan_id).map(|(tx, _)| tx.subscribe())
    }

    /// Signal plan approval/rejection through the watch channel
    pub async fn resolve_plan(&self, plan_id: &str, decision: PlanDecision) -> bool {
        let channels = self.plan_channels.read().await;
        if let Some((tx, _)) = channels.get(plan_id) {
            let _ = tx.send(Some(decision));
            true
        } else {
            false
        }
    }

    /// Remove a plan channel (cleanup after long-poll completes)
    pub async fn remove_plan_channel(&self, plan_id: &str) {
        let mut channels = self.plan_channels.write().await;
        channels.remove(plan_id);
    }

    /// Get the pending plan for a context_id (for frontend reconciliation).
    pub async fn get_pending_plan_for_context(&self, context_id: &str) -> Option<PendingTeamPlan> {
        let plans = self.pending_plans.read().await;
        plans.values().find(|p| p.context_id == context_id).cloned()
    }

    /// Create a new team
    pub async fn create_team(
        &self,
        name: &str,
        context_id: &str,
        context_type: &str,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        if teams.contains_key(name) {
            return Err(TeamTrackerError::TeamAlreadyExists(name.to_string()));
        }
        teams.insert(
            name.to_string(),
            TeamState {
                name: name.to_string(),
                context_id: context_id.to_string(),
                context_type: context_type.to_string(),
                lead_name: None,
                teammates: HashMap::new(),
                messages: Vec::new(),
                created_at: Utc::now(),
                phase: TeamPhase::Forming,
            },
        );
        Ok(())
    }

    /// Add a teammate to a team
    pub async fn add_teammate(
        &self,
        team_name: &str,
        name: &str,
        color: &str,
        model: &str,
        role: &str,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;

        if team.teammates.contains_key(name) {
            return Err(TeamTrackerError::TeammateAlreadyExists(name.to_string()));
        }

        let now = Utc::now();
        team.teammates.insert(
            name.to_string(),
            TeammateState {
                name: name.to_string(),
                color: color.to_string(),
                model: model.to_string(),
                role: role.to_string(),
                status: TeammateStatus::Spawning,
                cost: TeammateCost::default(),
                handle: None,
                spawned_at: now,
                last_activity_at: now,
                conversation_id: None,
            },
        );

        // First teammate added becomes the lead
        if team.lead_name.is_none() {
            team.lead_name = Some(name.to_string());
        }

        // Move to Active phase when first teammate is added
        if team.phase == TeamPhase::Forming {
            team.phase = TeamPhase::Active;
        }

        Ok(())
    }

    /// Set the teammate handle (process + stream task)
    pub async fn set_teammate_handle(
        &self,
        team_name: &str,
        teammate_name: &str,
        handle: TeammateHandle,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get_mut(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;
        teammate.handle = Some(handle);
        Ok(())
    }

    /// Update teammate status
    pub async fn update_teammate_status(
        &self,
        team_name: &str,
        teammate_name: &str,
        status: TeammateStatus,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get_mut(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;
        teammate.status = status;
        teammate.last_activity_at = Utc::now();
        Ok(())
    }

    /// Set the conversation_id for a teammate (called by stream processor after creating conversation)
    pub async fn set_teammate_conversation_id(
        &self,
        team_name: &str,
        teammate_name: &str,
        conversation_id: String,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get_mut(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;
        teammate.conversation_id = Some(conversation_id);
        Ok(())
    }

    /// Update teammate cost
    pub async fn update_teammate_cost(
        &self,
        team_name: &str,
        teammate_name: &str,
        cost: TeammateCost,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get_mut(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;
        teammate.cost = cost;
        Ok(())
    }

    /// Get team status (serializable snapshot)
    pub async fn get_team_status(
        &self,
        team_name: &str,
    ) -> Result<TeamStatusResponse, TeamTrackerError> {
        let teams = self.teams.read().await;
        let team = teams
            .get(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;

        Ok(TeamStatusResponse {
            name: team.name.clone(),
            context_id: team.context_id.clone(),
            context_type: team.context_type.clone(),
            lead_name: team.lead_name.clone(),
            teammates: team
                .teammates
                .values()
                .map(|t| TeammateStatusResponse {
                    name: t.name.clone(),
                    color: t.color.clone(),
                    model: t.model.clone(),
                    role: t.role.clone(),
                    status: t.status,
                    cost: t.cost.clone(),
                    spawned_at: t.spawned_at.to_rfc3339(),
                    last_activity_at: t.last_activity_at.to_rfc3339(),
                    conversation_id: t.conversation_id.clone(),
                })
                .collect(),
            phase: team.phase,
            created_at: team.created_at.to_rfc3339(),
            message_count: team.messages.len(),
        })
    }

    /// Get teammate cost
    pub async fn get_teammate_cost(
        &self,
        team_name: &str,
        teammate_name: &str,
    ) -> Result<TeammateCostResponse, TeamTrackerError> {
        let teams = self.teams.read().await;
        let team = teams
            .get(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;

        Ok(TeammateCostResponse {
            teammate_name: teammate.name.clone(),
            input_tokens: teammate.cost.input_tokens,
            output_tokens: teammate.cost.output_tokens,
            cache_creation_tokens: teammate.cost.cache_creation_tokens,
            cache_read_tokens: teammate.cost.cache_read_tokens,
            estimated_usd: teammate.cost.estimated_usd,
        })
    }

    /// Send a user message to the team
    pub async fn send_user_message(
        &self,
        team_name: &str,
        content: &str,
    ) -> Result<TeamMessage, TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;

        let message = TeamMessage {
            id: uuid::Uuid::new_v4().to_string(),
            team_name: team_name.to_string(),
            sender: "user".to_string(),
            recipient: None,
            content: content.to_string(),
            message_type: TeamMessageType::UserMessage,
            timestamp: Utc::now(),
        };
        team.messages.push(message.clone());
        Ok(message)
    }

    /// Add a teammate message
    pub async fn add_teammate_message(
        &self,
        team_name: &str,
        sender: &str,
        recipient: Option<&str>,
        content: &str,
        message_type: TeamMessageType,
    ) -> Result<TeamMessage, TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;

        let message = TeamMessage {
            id: uuid::Uuid::new_v4().to_string(),
            team_name: team_name.to_string(),
            sender: sender.to_string(),
            recipient: recipient.map(|r| r.to_string()),
            content: content.to_string(),
            message_type,
            timestamp: Utc::now(),
        };
        team.messages.push(message.clone());
        Ok(message)
    }

    /// Get team messages (serializable)
    pub async fn get_team_messages(
        &self,
        team_name: &str,
        limit: Option<usize>,
    ) -> Result<Vec<TeamMessageResponse>, TeamTrackerError> {
        let teams = self.teams.read().await;
        let team = teams
            .get(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;

        let messages: Vec<TeamMessageResponse> = team
            .messages
            .iter()
            .rev()
            .take(limit.unwrap_or(usize::MAX))
            .map(|m| TeamMessageResponse {
                id: m.id.clone(),
                sender: m.sender.clone(),
                recipient: m.recipient.clone(),
                content: m.content.clone(),
                message_type: m.message_type.clone(),
                timestamp: m.timestamp.to_rfc3339(),
            })
            .collect();

        Ok(messages)
    }

    /// Stop a specific teammate
    pub async fn stop_teammate(
        &self,
        team_name: &str,
        teammate_name: &str,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get_mut(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;

        // Signal kill + abort stream task
        if let Some(ref mut handle) = teammate.handle {
            if let Some(kill_tx) = handle.kill_tx.take() {
                let _ = kill_tx.send(());
            }
            if let Some(task) = handle.stream_task.take() {
                task.abort();
            }
        }
        teammate.status = TeammateStatus::Shutdown;
        teammate.last_activity_at = Utc::now();
        teammate.handle = None;
        Ok(())
    }

    /// Remove a teammate from a team
    pub async fn remove_teammate(
        &self,
        team_name: &str,
        teammate_name: &str,
    ) -> Result<(), TeamTrackerError> {
        // Stop first
        self.stop_teammate(team_name, teammate_name).await?;

        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        team.teammates.remove(teammate_name);
        Ok(())
    }

    /// Stop all teammates in a team
    pub async fn stop_team(&self, team_name: &str) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;

        for teammate in team.teammates.values_mut() {
            if let Some(ref mut handle) = teammate.handle {
                if let Some(kill_tx) = handle.kill_tx.take() {
                    let _ = kill_tx.send(());
                }
                if let Some(task) = handle.stream_task.take() {
                    task.abort();
                }
            }
            teammate.status = TeammateStatus::Shutdown;
            teammate.last_activity_at = Utc::now();
            teammate.handle = None;
        }
        team.phase = TeamPhase::Winding;
        Ok(())
    }

    /// Disband a team (stop all + remove)
    pub async fn disband_team(&self, team_name: &str) -> Result<(), TeamTrackerError> {
        // Stop all teammates first (needs separate write lock acquisition)
        self.stop_team(team_name).await?;

        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        team.phase = TeamPhase::Disbanded;
        Ok(())
    }

    /// List all active team names
    pub async fn list_teams(&self) -> Vec<String> {
        let teams = self.teams.read().await;
        teams.keys().cloned().collect()
    }

    /// Check if a team exists
    pub async fn team_exists(&self, team_name: &str) -> bool {
        let teams = self.teams.read().await;
        teams.contains_key(team_name)
    }

    /// Find an active (non-disbanded) team by its context_id.
    /// Returns the team name if found.
    pub async fn find_team_by_context_id(&self, context_id: &str) -> Option<String> {
        let teams = self.teams.read().await;
        teams
            .values()
            .find(|t| t.context_id == context_id && t.phase != TeamPhase::Disbanded)
            .map(|t| t.name.clone())
    }

    /// Send a message to a teammate's stdin (interactive mode).
    ///
    /// Returns an error if the teammate doesn't exist, has no handle,
    /// or is not in interactive mode.
    pub async fn send_stdin_message(
        &self,
        team_name: &str,
        teammate_name: &str,
        message: &str,
    ) -> Result<(), TeamTrackerError> {
        let mut teams = self.teams.write().await;
        let team = teams
            .get_mut(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        let teammate = team
            .teammates
            .get_mut(teammate_name)
            .ok_or_else(|| TeamTrackerError::TeammateNotFound(teammate_name.to_string()))?;

        if let Some(ref mut handle) = teammate.handle {
            handle
                .write_message(message)
                .await
                .map_err(|e| TeamTrackerError::StdinWriteFailed(e.to_string()))?;
            teammate.last_activity_at = Utc::now();
            Ok(())
        } else {
            Err(TeamTrackerError::TeammateNotFound(format!(
                "{} (no process handle)",
                teammate_name
            )))
        }
    }

    /// Get the teammate count for a team
    pub async fn get_teammate_count(&self, team_name: &str) -> Result<usize, TeamTrackerError> {
        let teams = self.teams.read().await;
        let team = teams
            .get(team_name)
            .ok_or_else(|| TeamTrackerError::TeamNotFound(team_name.to_string()))?;
        Ok(team.teammates.len())
    }
}

// ============================================================================
// Errors
// ============================================================================

#[derive(Debug, Clone)]
pub enum TeamTrackerError {
    TeamNotFound(String),
    TeamAlreadyExists(String),
    TeammateNotFound(String),
    TeammateAlreadyExists(String),
    StdinWriteFailed(String),
    MaxTeammatesExceeded { max: u8, current: usize },
}

impl std::fmt::Display for TeamTrackerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TeamNotFound(name) => write!(f, "Team not found: {}", name),
            Self::TeamAlreadyExists(name) => write!(f, "Team already exists: {}", name),
            Self::TeammateNotFound(name) => write!(f, "Teammate not found: {}", name),
            Self::TeammateAlreadyExists(name) => write!(f, "Teammate already exists: {}", name),
            Self::StdinWriteFailed(msg) => write!(f, "Stdin write failed: {}", msg),
            Self::MaxTeammatesExceeded { max, current } => {
                write!(f, "Max teammates exceeded: {current} >= {max}")
            }
        }
    }
}

impl std::error::Error for TeamTrackerError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[path = "team_state_tracker_tests.rs"]
mod tests;
