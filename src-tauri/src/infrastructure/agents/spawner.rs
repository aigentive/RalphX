// AgenticClientSpawner
// Bridges the state machine's AgentSpawner trait to the AgenticClient

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use tauri::{AppHandle, Emitter, Wry};

use crate::application::agent_lane_resolution::resolve_agent_spawn_settings;
use crate::application::chat_service::uses_execution_slot;
use crate::application::harness_runtime_registry::resolve_harness_plugin_dir;
use crate::commands::execution_commands::context_matches_running_status_for_gc;
use crate::commands::ExecutionState;
use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentHarnessKind, AgentRole, AgenticClient, ClientType,
    LogicalEffort, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{ChatContextType, IdeationSessionId, TaskId};
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, ExecutionSettingsRepository,
    IdeationSessionRepository, ProjectRepository, TaskRepository,
};
use crate::domain::services::RunningAgentRegistry;
use crate::domain::state_machine::AgentSpawner;
use crate::domain::supervisor::{ErrorInfo, SupervisorEvent, ToolCallInfo};
use crate::infrastructure::supervisor::EventBus;

/// Bridge between the state machine's AgentSpawner and the AgenticClient
///
/// This allows the state machine to spawn agents without knowing
/// the implementation details of the agentic client.
pub struct AgenticClientSpawner {
    /// The underlying agentic client
    default_client: Arc<dyn AgenticClient>,
    /// The harness represented by the default client
    default_harness: AgentHarnessKind,
    /// Harness-specific clients for multi-harness execution lanes
    harness_clients: HashMap<AgentHarnessKind, Arc<dyn AgenticClient>>,
    /// Working directory for spawned agents (fallback when task/project lookup fails)
    working_directory: PathBuf,
    /// Task repository for per-task CWD resolution
    task_repo: Option<Arc<dyn TaskRepository>>,
    /// Project repository for per-task CWD resolution
    project_repo: Option<Arc<dyn ProjectRepository>>,
    /// Execution settings repo for project-aware spawn gating
    execution_settings_repo: Option<Arc<dyn ExecutionSettingsRepository>>,
    /// Provider-neutral lane settings for harness selection
    agent_lane_settings_repo: Option<Arc<dyn AgentLaneSettingsRepository>>,
    /// Provider enablement/default settings for onboarding gating
    agent_provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    /// Ideation session repo for ideation-aware project slot counting
    ideation_session_repo: Option<Arc<dyn IdeationSessionRepository>>,
    /// Running registry for project-aware slot counting
    running_agent_registry: Option<Arc<dyn RunningAgentRegistry>>,
    /// Event bus for supervisor events (optional)
    event_bus: Option<Arc<EventBus>>,
    /// Tracks active agent handles by task_id for wait/stop operations
    handles: Arc<Mutex<HashMap<String, AgentHandle>>>,
    /// Execution state for spawn gating (optional)
    execution_state: Option<Arc<ExecutionState>>,
    /// Tauri app handle for emitting events to frontend (optional)
    app_handle: Option<AppHandle<Wry>>,
}

impl AgenticClientSpawner {
    /// Create a new spawner with the given client
    pub fn new(client: Arc<dyn AgenticClient>) -> Self {
        // Working directory should be project root (parent of src-tauri), not src-tauri itself
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let working_directory = cwd.parent().map(|p| p.to_path_buf()).unwrap_or(cwd);

        Self {
            default_client: client,
            default_harness: DEFAULT_AGENT_HARNESS,
            harness_clients: HashMap::new(),
            working_directory,
            task_repo: None,
            project_repo: None,
            execution_settings_repo: None,
            agent_lane_settings_repo: None,
            agent_provider_settings_repo: None,
            ideation_session_repo: None,
            running_agent_registry: None,
            event_bus: None,
            handles: Arc::new(Mutex::new(HashMap::new())),
            execution_state: None,
            app_handle: None,
        }
    }

    pub fn with_default_harness(mut self, harness: AgentHarnessKind) -> Self {
        self.default_harness = harness;
        self
    }

    /// Create with a specific working directory
    pub fn with_working_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.working_directory = path.into();
        self
    }

    /// Attach task and project repos for per-task CWD resolution
    pub fn with_repos(
        mut self,
        task_repo: Arc<dyn TaskRepository>,
        project_repo: Arc<dyn ProjectRepository>,
    ) -> Self {
        self.task_repo = Some(task_repo);
        self.project_repo = Some(project_repo);
        self
    }

    /// Attach runtime allocation context for project-aware spawn gating.
    pub fn with_runtime_admission_context(
        mut self,
        execution_settings_repo: Arc<dyn ExecutionSettingsRepository>,
        agent_lane_settings_repo: Arc<dyn AgentLaneSettingsRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
    ) -> Self {
        self.execution_settings_repo = Some(execution_settings_repo);
        self.agent_lane_settings_repo = Some(agent_lane_settings_repo);
        self.ideation_session_repo = Some(ideation_session_repo);
        self.running_agent_registry = Some(running_agent_registry);
        self
    }

    /// Attach an event bus for supervisor events
    pub fn with_event_bus(mut self, event_bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Attach execution state for spawn gating
    pub fn with_execution_state(mut self, state: Arc<ExecutionState>) -> Self {
        self.execution_state = Some(state);
        self
    }

    /// Attach Tauri app handle for event emission
    pub fn with_app_handle(mut self, handle: AppHandle<Wry>) -> Self {
        self.app_handle = Some(handle);
        self
    }

    /// Override the client used for a specific harness.
    pub fn with_harness_client(
        mut self,
        harness: AgentHarnessKind,
        client: Arc<dyn AgenticClient>,
    ) -> Self {
        self.harness_clients.insert(harness, client);
        self
    }

    pub fn with_harness_clients<I>(mut self, clients: I) -> Self
    where
        I: IntoIterator<Item = (AgentHarnessKind, Arc<dyn AgenticClient>)>,
    {
        self.harness_clients.extend(clients);
        self
    }

    pub fn with_agent_provider_settings_repo(
        mut self,
        repo: Arc<dyn AgentProviderSettingsRepository>,
    ) -> Self {
        self.agent_provider_settings_repo = Some(repo);
        self
    }

    /// Map agent type string to AgentRole
    fn role_from_string(agent_type: &str) -> AgentRole {
        match agent_type {
            "worker" | "ralphx-worker" | "ralphx-execution-worker" => AgentRole::Worker,
            "coder" | "ralphx-coder" | "ralphx-execution-coder" => AgentRole::Worker,
            "qa-prep" => AgentRole::QaPrep,
            "qa-refiner" => AgentRole::QaRefiner,
            "qa-tester" => AgentRole::QaTester,
            "reviewer" | "ralphx-reviewer" | "ralphx-execution-reviewer" => AgentRole::Reviewer,
            "merger" | "ralphx-merger" | "ralphx-execution-merger" => AgentRole::Reviewer,
            other => AgentRole::Custom(other.to_string()),
        }
    }

    /// Emit a TaskStart event if event bus is configured
    fn emit_task_start(&self, task_id: &str, agent_type: &str) {
        if let Some(ref event_bus) = self.event_bus {
            let event = SupervisorEvent::task_start(task_id, agent_type);
            // Ignore if no subscribers - that's okay
            let _ = event_bus.publish(event);
        }
    }

    /// Emit a ToolCall event if event bus is configured
    pub fn emit_tool_call(&self, task_id: &str, info: ToolCallInfo) {
        if let Some(ref event_bus) = self.event_bus {
            let event = SupervisorEvent::tool_call(task_id, info);
            let _ = event_bus.publish(event);
        }
    }

    /// Emit an Error event if event bus is configured
    pub fn emit_error(&self, task_id: &str, info: ErrorInfo) {
        if let Some(ref event_bus) = self.event_bus {
            let event = SupervisorEvent::error(task_id, info);
            let _ = event_bus.publish(event);
        }
    }

    /// Get a reference to the event bus if configured
    pub fn event_bus(&self) -> Option<&Arc<EventBus>> {
        self.event_bus.as_ref()
    }

    /// Resolve the project ID for a given task.
    async fn resolve_project_id(&self, task_id: &str) -> Option<String> {
        if let Some(task_repo) = &self.task_repo {
            let task_id_typed = TaskId(task_id.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id_typed).await {
                return Some(task.project_id.as_str().to_string());
            }
        }
        None
    }

    async fn resolve_task_status(&self, task_id: &str) -> Option<String> {
        let task_repo = self.task_repo.as_ref()?;
        let task_id_typed = TaskId(task_id.to_string());
        let task = task_repo.get_by_id(&task_id_typed).await.ok()??;
        Some(task.internal_status.as_str().to_string())
    }

    async fn project_has_execution_capacity(
        &self,
        task_id: &str,
        execution_state: &ExecutionState,
    ) -> Option<bool> {
        let (Some(task_repo), Some(execution_settings_repo), Some(running_agent_registry)) = (
            &self.task_repo,
            &self.execution_settings_repo,
            &self.running_agent_registry,
        ) else {
            return None;
        };

        let task_id_typed = TaskId(task_id.to_string());
        let Ok(Some(task)) = task_repo.get_by_id(&task_id_typed).await else {
            return None;
        };

        let Ok(settings) = execution_settings_repo
            .get_settings(Some(&task.project_id))
            .await
        else {
            return None;
        };

        let registry_entries = running_agent_registry.list_all().await;
        let mut running_project_total = 0u32;

        for (key, info) in registry_entries {
            if info.pid == 0 {
                continue;
            }

            if key.context_type == "ideation" || key.context_type == "session" {
                let Some(ideation_session_repo) = &self.ideation_session_repo else {
                    continue;
                };
                let session_id = IdeationSessionId::from_string(key.context_id.clone());
                let Ok(Some(session)) = ideation_session_repo.get_by_id(&session_id).await else {
                    continue;
                };

                if session.project_id != task.project_id {
                    continue;
                }

                let slot_key = format!("{}/{}", key.context_type, key.context_id);
                if execution_state.is_interactive_idle(&slot_key) {
                    continue;
                }

                running_project_total += 1;
                continue;
            }

            let Ok(context_type) = key.context_type.parse::<ChatContextType>() else {
                continue;
            };
            if !uses_execution_slot(context_type) {
                continue;
            }

            let related_task_id = TaskId::from_string(key.context_id);
            let Ok(Some(related_task)) = task_repo.get_by_id(&related_task_id).await else {
                continue;
            };

            if related_task.project_id != task.project_id
                || !context_matches_running_status_for_gc(
                    context_type,
                    related_task.internal_status,
                )
            {
                continue;
            }

            running_project_total += 1;
        }

        Some(
            execution_state
                .can_start_execution_context(running_project_total, settings.max_concurrent_tasks),
        )
    }

    /// Resolve the working directory for a given task.
    /// Uses task's worktree_path, falls back to spawner default.
    async fn resolve_working_directory(&self, task_id: &str) -> PathBuf {
        if let (Some(task_repo), Some(project_repo)) = (&self.task_repo, &self.project_repo) {
            let task_id_typed = TaskId(task_id.to_string());
            if let Ok(Some(task)) = task_repo.get_by_id(&task_id_typed).await {
                let project_id = &task.project_id;
                if let Ok(Some(_project)) = project_repo.get_by_id(project_id).await {
                    return task
                        .worktree_path
                        .as_ref()
                        .map(|p| PathBuf::from(p))
                        .unwrap_or_else(|| {
                            tracing::error!(
                                task_id = %task.id.0,
                                "Safety net: worktree_path is None — \
                                 refusing to use project directory (main branch). \
                                 Falling back to spawner default."
                            );
                            self.working_directory.clone()
                        });
                }
            }
        }
        // Fallback to the spawner's default working directory
        self.working_directory.clone()
    }

    fn process_name(agent_type: &str) -> Option<&'static str> {
        match agent_type {
            "worker" | "coder" | "ralphx-execution-worker" | "ralphx-execution-coder" => Some("execution"),
            "reviewer" | "ralphx-execution-reviewer" => Some("review"),
            "merger" | "ralphx-execution-merger" => Some("merge"),
            "qa-prep" => Some("qa_prep"),
            "qa-refiner" => Some("qa_refine"),
            "qa-tester" => Some("qa_test"),
            _ => None,
        }
    }

    fn context_type_for_agent(agent_type: &str) -> Option<ChatContextType> {
        match agent_type {
            "worker" | "coder" | "ralphx-execution-worker" | "ralphx-execution-coder" | "qa-prep" => {
                Some(ChatContextType::TaskExecution)
            }
            "qa-refiner" | "qa-tester" => Some(ChatContextType::TaskExecution),
            "reviewer" | "ralphx-execution-reviewer" => Some(ChatContextType::Review),
            "merger" | "ralphx-execution-merger" => Some(ChatContextType::Merge),
            _ => None,
        }
    }

    fn resolve_process_agent_name(agent_type: &str) -> Option<String> {
        let process = Self::process_name(agent_type)?;
        crate::infrastructure::agents::claude::resolve_process_agent(
            crate::infrastructure::agents::claude::process_mapping(),
            process,
            "default",
        )
        .map(|name| crate::infrastructure::agents::claude::qualify_agent_name(&name))
    }

    async fn resolve_spawn_harness(
        &self,
        agent_type: &str,
        task_id: &str,
        project_id: Option<&str>,
    ) -> Result<
        (
        AgentHarnessKind,
        Option<String>,
        Option<LogicalEffort>,
        Option<String>,
        Option<String>,
        ),
        String,
    > {
        let Some(context_type) = Self::context_type_for_agent(agent_type) else {
            if let Some(provider_repo) = self.agent_provider_settings_repo.as_ref() {
                crate::application::ensure_provider_spawn_enabled(
                    provider_repo,
                    self.default_harness,
                    "state-machine agent spawn",
                )
                .await?;
            }
            return Ok((self.default_harness, None, None, None, None));
        };
        let Some(agent_name) = Self::resolve_process_agent_name(agent_type) else {
            if let Some(provider_repo) = self.agent_provider_settings_repo.as_ref() {
                crate::application::ensure_provider_spawn_enabled(
                    provider_repo,
                    self.default_harness,
                    "state-machine agent spawn",
                )
                .await?;
            }
            return Ok((self.default_harness, None, None, None, None));
        };
        let entity_status = self.resolve_task_status(task_id).await;

        let resolved = resolve_agent_spawn_settings(
            &agent_name,
            project_id,
            context_type,
            entity_status.as_deref(),
            None,
            None,
            self.agent_lane_settings_repo.as_ref(),
        )
        .await;

        let harness = resolved.effective_harness;
        if let Some(provider_repo) = self.agent_provider_settings_repo.as_ref() {
            crate::application::ensure_provider_spawn_enabled(
                provider_repo,
                harness,
                "state-machine agent spawn",
            )
            .await?;
        }
        let client = self.resolve_client_for_harness(harness).ok_or_else(|| {
            format!("Configured execution harness {} is not registered in the runtime", harness)
        })?;
        let harness_available = client.is_available().await.unwrap_or(false);
        if !harness_available {
            return Err(format!(
                "Configured execution harness {} is unavailable for {}",
                harness, agent_type
            ));
        }

        Ok((
            harness,
            Some(resolved.model),
            resolved.logical_effort,
            resolved.approval_policy,
            resolved.sandbox_mode,
        ))
    }

    fn build_agent_config(
        &self,
        harness: AgentHarnessKind,
        client_type: ClientType,
        role: AgentRole,
        agent_type: &str,
        task_id: &str,
        working_dir: PathBuf,
        project_id: Option<String>,
        model: Option<String>,
        logical_effort: Option<LogicalEffort>,
        approval_policy: Option<String>,
        sandbox_mode: Option<String>,
    ) -> AgentConfig {
        let mut env = std::collections::HashMap::new();
        if let Some(pid) = project_id {
            env.insert("RALPHX_PROJECT_ID".to_string(), pid.clone());
        }
        env.insert("RALPHX_TASK_ID".to_string(), task_id.to_string());

        let mut config = AgentConfig {
            role,
            prompt: format!("Execute task {}", task_id),
            working_directory: working_dir.clone(),
            plugin_dir: None,
            agent: None,
            model,
            harness: Some(harness),
            logical_effort,
            approval_policy,
            sandbox_mode,
            max_tokens: None,
            timeout_secs: None,
            env,
        };

        if matches!(client_type, ClientType::ClaudeCode | ClientType::Codex) {
            let plugin_dir = resolve_harness_plugin_dir(harness, &working_dir);
            config.plugin_dir = Some(plugin_dir);
            config.agent = Self::resolve_process_agent_name(agent_type).or_else(|| {
                Some(
                    crate::infrastructure::agents::claude::agent_names::spawner_agent_name(
                        agent_type,
                    )
                    .to_string(),
                )
            });
        }

        config
    }

    fn resolve_client_for_harness(&self, harness: AgentHarnessKind) -> Option<Arc<dyn AgenticClient>> {
        if harness == self.default_harness {
            Some(Arc::clone(&self.default_client))
        } else {
            self.harness_clients.get(&harness).cloned()
        }
    }

    fn resolve_client_for_handle(&self, handle: &AgentHandle) -> Option<Arc<dyn AgenticClient>> {
        AgentHarnessKind::try_from(handle.client_type.clone())
            .ok()
            .and_then(|harness| self.resolve_client_for_harness(harness))
    }

    fn rollback_spawn_failure(&self) {
        if let Some(ref exec) = self.execution_state {
            exec.decrement_running();
            if let Some(ref handle) = self.app_handle {
                exec.emit_status_changed(handle, "task_spawn_failed");
            }
        }
    }
}

#[async_trait]
impl AgentSpawner for AgenticClientSpawner {
    async fn spawn(&self, agent_type: &str, task_id: &str) {
        // B5: Check if this agent type is already running for this task
        let handle_key = format!("{}/{}", task_id, agent_type);
        {
            let handles = self.handles.lock().await;
            if handles.contains_key(&handle_key) {
                warn!(
                    task_id = task_id,
                    agent_type = agent_type,
                    "Spawn skipped: agent already running for this task"
                );
                return;
            }
        }

        // Check execution state before spawning
        if let Some(ref exec) = self.execution_state {
            let global_allowed = exec.can_start_any_execution_context();
            let project_allowed = self
                .project_has_execution_capacity(task_id, exec)
                .await
                .unwrap_or(true);
            if !global_allowed || !project_allowed {
                let reason = if exec.is_paused() {
                    "execution_paused"
                } else if exec.is_provider_blocked() {
                    "provider_rate_limited"
                } else if !project_allowed {
                    "project_max_concurrent_reached"
                } else {
                    "max_concurrent_reached"
                };
                info!(
                    task_id = task_id,
                    agent_type = agent_type,
                    is_paused = exec.is_paused(),
                    running_count = exec.running_count(),
                    max_concurrent = exec.max_concurrent(),
                    reason = reason,
                    "Spawn blocked: execution paused or at max concurrent"
                );
                // Emit event for UI visibility
                if let Some(ref handle) = self.app_handle {
                    let _ = handle.emit(
                        "execution:spawn_blocked",
                        serde_json::json!({
                            "task_id": task_id,
                            "agent_type": agent_type,
                            "reason": reason,
                            "running_count": exec.running_count(),
                            "max_concurrent": exec.max_concurrent(),
                        }),
                    );
                }
                return;
            }
            // Increment running count before spawning
            exec.increment_running();
            // Emit status_changed event to frontend for real-time UI update
            if let Some(ref handle) = self.app_handle {
                exec.emit_status_changed(handle, "task_started");
            }
        }

        // Emit TaskStart event before spawning
        self.emit_task_start(task_id, agent_type);

        let role = Self::role_from_string(agent_type);

        // Resolve working directory per-task (worktree-aware)
        let working_dir = self.resolve_working_directory(task_id).await;

        // Resolve project ID for RALPHX_PROJECT_ID env var
        let project_id = self.resolve_project_id(task_id).await;
        let (harness, model, logical_effort, approval_policy, sandbox_mode) = match self
            .resolve_spawn_harness(agent_type, task_id, project_id.as_deref())
            .await
        {
            Ok(values) => values,
            Err(error) => {
                self.rollback_spawn_failure();
                self.emit_error(task_id, ErrorInfo::new(error, "resolve_spawn_harness"));
                return;
            }
        };
        let Some(client) = self.resolve_client_for_harness(harness) else {
            self.rollback_spawn_failure();
            self.emit_error(
                task_id,
                ErrorInfo::new(
                    format!("Configured execution harness {} is not registered in the runtime", harness),
                    "resolve_client_for_harness",
                ),
            );
            return;
        };
        let client_type = client.capabilities().client_type.clone();
        let config = self.build_agent_config(
            harness,
            client_type,
            role,
            agent_type,
            task_id,
            working_dir,
            project_id,
            model,
            logical_effort,
            approval_policy,
            sandbox_mode,
        );

        // Spawn and handle errors
        match client.spawn_agent(config).await {
            Ok(handle) => {
                // Store handle for wait/stop operations
                let mut handles = self.handles.lock().await;
                handles.insert(handle_key, handle);
            }
            Err(e) => {
                self.rollback_spawn_failure();
                // Emit error event
                self.emit_error(task_id, ErrorInfo::new(e.to_string(), "spawn_agent"));
            }
        }
    }

    async fn spawn_background(&self, agent_type: &str, task_id: &str) {
        // For background spawning, we just spawn without waiting
        self.spawn(agent_type, task_id).await;
    }

    async fn wait_for(&self, agent_type: &str, task_id: &str) {
        // Remove handle when done waiting (agent has completed)
        let handle_key = format!("{}/{}", task_id, agent_type);
        let handle = {
            let mut handles = self.handles.lock().await;
            handles.remove(&handle_key)
        };

        if let Some(handle) = handle {
            // Wait for agent to complete
            let Some(client) = self.resolve_client_for_handle(&handle) else {
                self.emit_error(
                    task_id,
                    ErrorInfo::new(
                        format!("No client registered for handle {:?}", handle.client_type),
                        "resolve_client_for_handle",
                    ),
                );
                return;
            };
            if let Err(e) = client.wait_for_completion(&handle).await {
                self.emit_error(task_id, ErrorInfo::new(e.to_string(), "wait_for"));
            }
        }
    }

    async fn stop(&self, agent_type: &str, task_id: &str) {
        let handle_key = format!("{}/{}", task_id, agent_type);
        let handle = {
            let mut handles = self.handles.lock().await;
            handles.remove(&handle_key)
        };

        if let Some(handle) = handle {
            let Some(client) = self.resolve_client_for_handle(&handle) else {
                self.emit_error(
                    task_id,
                    ErrorInfo::new(
                        format!("No client registered for handle {:?}", handle.client_type),
                        "resolve_client_for_handle",
                    ),
                );
                return;
            };
            if let Err(e) = client.stop_agent(&handle).await {
                self.emit_error(task_id, ErrorInfo::new(e.to_string(), "stop_agent"));
            }
        }
    }
}

#[cfg(test)]
#[path = "spawner_tests.rs"]
mod tests;
