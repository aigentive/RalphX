use super::*;
use crate::application::harness_runtime_registry::resolve_default_harness_plugin_dir;
use crate::domain::entities::ChatContextType;
use std::collections::HashMap;
use std::sync::Arc;

/// Context types where context_id is a task ID (worktree resolution applies).
const TASK_CONTEXT_TYPES: &[&str] = &["task_execution", "task", "review", "merge"];

pub(super) async fn ensure_team_mode_supported_for_context(
    state: &HttpServerState,
    context_type: &str,
    context_id: &str,
) -> Result<(), (StatusCode, String)> {
    use crate::domain::entities::chat_conversation::ChatContextType;

    let Ok(context_type_enum) = context_type.parse::<ChatContextType>() else {
        return Ok(());
    };

    if crate::application::team_mode_supported_for_context(
        &state.app_state,
        context_type_enum,
        context_id,
    )
    .await
    {
        return Ok(());
    }

    Err((
        StatusCode::CONFLICT,
        format!(
            "Team mode is not supported when the effective harness for {context_type} resolves to Codex. Codex currently operates in solo mode for this context."
        ),
    ))
}

pub(super) async fn resolve_teammate_working_dir(
    state: &HttpServerState,
    context_type: &str,
    context_id: &str,
) -> Result<PathBuf, String> {
    if let Ok(context_type_enum) = context_type.parse::<ChatContextType>() {
        if matches!(
            context_type_enum,
            ChatContextType::Ideation | ChatContextType::Delegation
        ) {
            return crate::application::chat_service::chat_service_context::resolve_working_directory(
                context_type_enum,
                context_id,
                Arc::clone(&state.app_state.project_repo),
                Arc::clone(&state.app_state.task_repo),
                Arc::clone(&state.app_state.ideation_session_repo),
                Arc::clone(&state.app_state.delegated_session_repo),
                &default_working_dir(),
            )
            .await;
        }
    }

    // Project context: project → project.working_directory
    if context_type == "project" {
        use crate::domain::entities::ProjectId;
        let project_id = ProjectId::from_string(context_id.to_string());
        if let Ok(Some(project)) = state.app_state.project_repo.get_by_id(&project_id).await {
            return Ok(PathBuf::from(&project.working_directory));
        }
        warn!(
            context_type,
            context_id, "Teammate working dir: project lookup failed — using default"
        );
        return Ok(default_working_dir());
    }

    // Task-related contexts: task → worktree_path or project.working_directory
    if !TASK_CONTEXT_TYPES.contains(&context_type) {
        return Ok(default_working_dir());
    }

    let task_id = TaskId(context_id.to_string());

    let task = match state.app_state.task_repo.get_by_id(&task_id).await {
        Ok(Some(task)) => task,
        Ok(None) => {
            warn!(
                context_id = context_id,
                "Teammate working dir: task not found — using default"
            );
            return Ok(default_working_dir());
        }
        Err(e) => {
            warn!(
                context_id = context_id,
                error = %e,
                "Teammate working dir: task lookup failed — using default"
            );
            return Ok(default_working_dir());
        }
    };

    let project = match state
        .app_state
        .project_repo
        .get_by_id(&task.project_id)
        .await
    {
        Ok(Some(project)) => project,
        Ok(None) => {
            warn!(
                project_id = %task.project_id,
                "Teammate working dir: project not found — using default"
            );
            return Ok(default_working_dir());
        }
        Err(e) => {
            warn!(
                project_id = %task.project_id,
                error = %e,
                "Teammate working dir: project lookup failed — using default"
            );
            return Ok(default_working_dir());
        }
    };

    Ok(if let Some(ref wt_path) = task.worktree_path {
        info!(
            task_id = context_id,
            worktree_path = wt_path,
            "Teammate working dir: using task worktree path"
        );
        PathBuf::from(wt_path)
    } else {
        // No worktree — use the project's working directory (repo root)
        PathBuf::from(&project.working_directory)
    })
}

pub(super) fn resolve_teammate_plugin_dir(working_dir: &std::path::Path) -> PathBuf {
    resolve_default_harness_plugin_dir(working_dir)
}

pub(super) async fn load_claude_provider_env_for_teammate_spawn(
    provider_repo: &Arc<dyn crate::domain::repositories::AgentProviderSettingsRepository>,
    spawn_config: TeammateSpawnConfig,
) -> Result<TeammateSpawnConfig, (StatusCode, String)> {
    let provider_env =
        crate::application::provider_env_file::load_provider_custom_env_file_for_harness(
            Some(provider_repo),
            crate::domain::agents::AgentHarnessKind::Claude,
        )
        .await
        .map_err(|error| {
            warn!(error = %error, "Failed to load Claude provider env file for teammate spawn");
            (StatusCode::BAD_REQUEST, error)
        })?;

    Ok(apply_provider_env_to_teammate_spawn_config(
        spawn_config,
        provider_env,
    ))
}

fn apply_provider_env_to_teammate_spawn_config(
    mut spawn_config: TeammateSpawnConfig,
    provider_env: HashMap<String, String>,
) -> TeammateSpawnConfig {
    for (key, value) in provider_env {
        spawn_config = spawn_config.with_env(key, value);
    }
    spawn_config
}

/// Fallback working directory (same as TeammateSpawnConfig::new default).
fn default_working_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Resolve the project ID for a teammate's context, mirroring the lead agent's
/// RALPHX_PROJECT_ID resolution from `chat_service_context::resolve_project_id`.
pub(super) async fn resolve_teammate_project_id(
    state: &HttpServerState,
    context_type: &str,
    context_id: &str,
) -> Option<String> {
    use crate::domain::entities::chat_conversation::ChatContextType;
    let ct = context_type.parse::<ChatContextType>().ok()?;
    crate::application::chat_service::chat_service_context::resolve_project_id(
        ct,
        context_id,
        state.app_state.task_repo.clone(),
        state.app_state.ideation_session_repo.clone(),
        state.app_state.delegated_session_repo.clone(),
    )
    .await
}

/// Color palette for teammate distinction
#[doc(hidden)]
pub const TEAMMATE_COLORS: &[&str] = &["blue", "green", "cyan", "magenta", "yellow"];

/// Find the first active team via TeamService.
/// Returns (team_name, context_id, context_type).
#[doc(hidden)]
pub async fn find_active_team(state: &HttpServerState) -> Result<(String, String, String), String> {
    let teams = state.team_service.list_teams().await;
    for team_name in &teams {
        if let Ok(status) = state.team_service.get_team_status(team_name).await {
            let phase = status.phase;
            if phase == crate::application::team_state_tracker::TeamPhase::Active
                || phase == crate::application::team_state_tracker::TeamPhase::Forming
            {
                return Ok((team_name.clone(), status.context_id, status.context_type));
            }
        }
    }
    Err("No active team found. Create a team before spawning teammates.".to_string())
}

/// Read the lead's Claude Code session ID from the team config file.
///
/// Claude Code's `TeamCreate` tool writes `~/.claude/teams/{name}/config.json`
/// with a `leadSessionId` field. This is the most reliable source when the
/// `RALPHX_LEAD_SESSION_ID` env var wasn't set (first spawn, session_id not yet known).
pub(super) fn resolve_lead_session_from_config(team_name: &str) -> Option<String> {
    let (session_id, config_path) = read_lead_session_config(team_name)?;
    tracing::info!(
        team = %team_name,
        config_path = %config_path.display(),
        "[TEAM_SPAWN] Resolved leadSessionId from Claude Code team config"
    );
    Some(session_id)
}

fn trusted_claude_team_name(team_name: &str) -> Option<&str> {
    let is_safe = !team_name.is_empty()
        && !team_name.contains("..")
        && !team_name.contains('/')
        && !team_name.contains('\\')
        && team_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    is_safe.then_some(team_name)
}

fn read_lead_session_config(team_name: &str) -> Option<(String, PathBuf)> {
    let team_name = trusted_claude_team_name(team_name)?;
    let teams_dir = dirs::home_dir()?.join(".claude").join("teams");
    let canonical_teams_dir = teams_dir.canonicalize().ok()?;
    let canonical_team_dir = canonical_teams_dir.join(team_name).canonicalize().ok()?;
    if !canonical_team_dir.starts_with(&canonical_teams_dir) {
        return None;
    }
    let config_path = canonical_team_dir.join("config.json");
    // codeql[rust/path-injection]
    let content = std::fs::read_to_string(&config_path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&content).ok()?;
    let session_id = config.get("leadSessionId")?.as_str()?.to_string();
    Some((session_id, config_path))
}

/// Generate a unique teammate name, appending a suffix if needed.
#[doc(hidden)]
pub async fn generate_unique_teammate_name(
    state: &HttpServerState,
    team_name: &str,
    role: &str,
) -> String {
    let base_name = role.to_string();
    if let Ok(status) = state.team_service.get_team_status(team_name).await {
        let existing_names: Vec<&str> = status.teammates.iter().map(|t| t.name.as_str()).collect();
        if !existing_names.contains(&base_name.as_str()) {
            return base_name;
        }
        // Find next available suffix
        for i in 2..=99 {
            let candidate = format!("{}-{}", base_name, i);
            if !existing_names.contains(&candidate.as_str()) {
                return candidate;
            }
        }
    }
    base_name
}

/// Assign a color from the palette based on current teammate count.
#[doc(hidden)]
pub async fn assign_teammate_color(state: &HttpServerState, team_name: &str) -> String {
    let count = state
        .team_service
        .get_teammate_count(team_name)
        .await
        .unwrap_or(0);
    TEAMMATE_COLORS[count % TEAMMATE_COLORS.len()].to_string()
}

/// Resolve the MCP agent type for a teammate spawn, preferring the `preset`
/// field when available and falling back to a process-based default.
/// Extracted for unit testability without Axum handler infrastructure.
pub fn resolve_mcp_agent_type(process: &str, preset: Option<&str>) -> String {
    if let Some(p) = preset {
        return p.to_string();
    }
    if process.starts_with("worker") {
        "worker-team-member".to_string()
    } else {
        "ideation-team-member".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings};
    use crate::domain::repositories::AgentProviderSettingsRepository;
    use crate::infrastructure::memory::MemoryAgentProviderSettingsRepository;

    #[test]
    fn apply_provider_env_to_teammate_spawn_config_merges_values() {
        let spawn_config = TeammateSpawnConfig::new("researcher", "team-a", "inspect");
        let provider_env = HashMap::from([(
            "CUSTOM_PROVIDER_TOKEN".to_string(),
            "from-provider-env".to_string(),
        )]);

        let spawn_config = apply_provider_env_to_teammate_spawn_config(spawn_config, provider_env);

        assert_eq!(
            spawn_config
                .env
                .get("CUSTOM_PROVIDER_TOKEN")
                .map(String::as_str),
            Some("from-provider-env")
        );
    }

    #[tokio::test]
    async fn load_claude_provider_env_for_teammate_spawn_reads_repo_settings() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let env_path = temp_dir.path().join("claude.env");
        std::fs::write(
            &env_path,
            "CUSTOM_PROVIDER_TOKEN=from-provider-env\nCLAUDE_MODEL=spoofed\n",
        )
        .expect("write env file");
        let provider_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
        let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
        settings.custom_env_file_enabled = true;
        settings.custom_env_file_path = Some(env_path.to_string_lossy().into_owned());
        provider_repo
            .upsert(&settings)
            .await
            .expect("save settings");
        let provider_repo: Arc<dyn AgentProviderSettingsRepository> = provider_repo;

        let spawn_config = load_claude_provider_env_for_teammate_spawn(
            &provider_repo,
            TeammateSpawnConfig::new("researcher", "team-a", "inspect"),
        )
        .await
        .expect("load provider env");

        assert_eq!(
            spawn_config
                .env
                .get("CUSTOM_PROVIDER_TOKEN")
                .map(String::as_str),
            Some("from-provider-env")
        );
        assert!(!spawn_config.env.contains_key("CLAUDE_MODEL"));
    }
}
