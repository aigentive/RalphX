use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::agents::{
    AgentHarnessKind, EffectiveMcpServerPolicy, McpLaunchPolicy, McpOverrideState,
    McpPolicyOverride, McpPolicySource, McpServerKey, McpSetupPreflightFailure,
    NativeMcpServerSnapshot, NativeMcpState,
};
use crate::domain::repositories::{AgentProviderSettingsRepository, McpPolicyRepository};
use crate::error::{AppError, AppResult};

use super::mcp_policy_config::{load_mcp_policy_file, McpPolicyConfigSnapshot};

#[derive(Clone)]
pub struct McpPolicyService {
    repo: Arc<dyn McpPolicyRepository>,
    global_policy_path: PathBuf,
    provider_settings_repo: Option<Arc<dyn AgentProviderSettingsRepository>>,
    reserved_claude_mcp_cleanup_cli_override: Option<PathBuf>,
}

impl McpPolicyService {
    pub fn new(repo: Arc<dyn McpPolicyRepository>, global_policy_path: PathBuf) -> Self {
        Self {
            repo,
            global_policy_path,
            provider_settings_repo: None,
            reserved_claude_mcp_cleanup_cli_override: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_reserved_claude_mcp_cleanup_cli_for_test(
        mut self,
        cli_path: PathBuf,
    ) -> Self {
        self.reserved_claude_mcp_cleanup_cli_override = Some(cli_path);
        self
    }

    pub fn with_provider_settings_repo(
        mut self,
        provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    ) -> Self {
        self.provider_settings_repo = Some(provider_settings_repo);
        self
    }

    fn default_provider_home(&self) -> AppResult<&Path> {
        let policy_root = self.global_policy_path.parent().ok_or_else(|| {
            AppError::Infrastructure("Global MCP policy has no owned root".to_string())
        })?;
        if policy_root
            .file_name()
            .is_some_and(|name| name == ".ralphx")
        {
            policy_root.parent().ok_or_else(|| {
                AppError::Infrastructure("Global MCP policy has no provider home".to_string())
            })
        } else {
            Ok(policy_root)
        }
    }

    pub(crate) async fn provider_native_config_root(
        &self,
        provider: AgentHarnessKind,
    ) -> AppResult<PathBuf> {
        self.provider_native_context(provider)
            .await
            .map(|(root, _)| root)
    }

    pub(crate) async fn provider_native_context(
        &self,
        provider: AgentHarnessKind,
    ) -> AppResult<(PathBuf, HashMap<String, String>)> {
        let provider_env =
            crate::application::provider_env_file::load_provider_custom_env_file_for_harness(
                self.provider_settings_repo.as_ref(),
                provider,
            )
            .await
            .map_err(AppError::Infrastructure)?;
        let shell_env = self
            .provider_settings_repo
            .is_some()
            .then(crate::infrastructure::login_shell_env::captured)
            .unwrap_or_default();
        let root = resolve_provider_native_config_root(
            provider,
            self.default_provider_home()?,
            &shell_env,
            &provider_env,
        )?;
        let mut effective_env = shell_env.as_ref().clone();
        effective_env.extend(provider_env);
        Ok((root, effective_env))
    }

    pub async fn resolve(
        &self,
        native: NativeMcpServerSnapshot,
        project_id: Option<&str>,
        project_root: Option<&Path>,
    ) -> AppResult<EffectiveMcpServerPolicy> {
        if native.key.is_ralphx_owned() {
            let collision = native.native_scope.as_deref() != Some("ralphx");
            let existing_diagnostic = native.diagnostic.clone();
            return Ok(EffectiveMcpServerPolicy {
                enabled: !collision,
                server_state: McpOverrideState::Enabled,
                server_source: McpPolicySource::RequiredInternal,
                tool_states: BTreeMap::new(),
                tool_sources: BTreeMap::new(),
                disabled_tools: Vec::new(),
                locked: true,
                locked_reason: Some(if collision {
                    format!(
                        "Native provider configuration already defines reserved server ID '{}'",
                        native.key.server_id
                    )
                } else {
                    "Required by RalphX".to_string()
                }),
                native: NativeMcpServerSnapshot {
                    native_state: if collision {
                        crate::domain::agents::NativeMcpState::Unavailable
                    } else {
                        native.native_state
                    },
                    diagnostic: collision.then(|| {
                        "Remove or rename the provider-native server that collides with this reserved RalphX ID. RalphX will not overwrite it."
                            .to_string()
                    }).or(existing_diagnostic),
                    ..native
                },
            });
        }

        let (global_yaml, project_yaml) = self.load_policy_configs(project_id, project_root)?;
        let global_ui = self
            .repo
            .get_global(&native.key)
            .await
            .map_err(repository_error)?;
        let project_ui = match project_id {
            Some(project_id) => self
                .repo
                .get_for_project(project_id, &native.key)
                .await
                .map_err(repository_error)?,
            None => None,
        };

        let key = native.key.clone();
        Ok(resolve_layers(
            native,
            global_yaml.policies.get(&key),
            global_ui.as_ref(),
            None,
            project_ui.as_ref(),
            Some(&project_yaml),
        ))
    }

    pub async fn resolve_launch_policy(
        &self,
        provider: AgentHarnessKind,
        project_id: Option<&str>,
        project_root: Option<&Path>,
    ) -> AppResult<McpLaunchPolicy> {
        let (provider_config_root, provider_env) = self.provider_native_context(provider).await?;
        if provider == AgentHarnessKind::Claude {
            self.reconcile_reserved_claude_registration(&provider_config_root, &provider_env)
                .await?;
        }
        crate::infrastructure::agents::ensure_no_reserved_native_mcp_collision_at(
            provider,
            &provider_config_root,
            project_root,
        )
        .map_err(|error| AppError::Infrastructure(error.safe_message()))?;
        let (global_yaml, project_yaml) = self.load_policy_configs(project_id, project_root)?;
        ensure_valid_policy_config("global", &global_yaml)?;
        ensure_valid_policy_config("project", &project_yaml)?;
        let global_ui = self
            .repo
            .list_global()
            .await
            .map_err(repository_error)?
            .into_iter()
            .map(|policy| (policy.key.clone(), policy))
            .collect::<HashMap<_, _>>();
        let project_ui = match project_id {
            Some(project_id) => self
                .repo
                .list_for_project(project_id)
                .await
                .map_err(repository_error)?
                .into_iter()
                .map(|policy| (policy.key.clone(), policy))
                .collect::<HashMap<_, _>>(),
            None => HashMap::new(),
        };
        let keys = global_yaml
            .policies
            .keys()
            .chain(global_ui.keys())
            .chain(project_yaml.policies.keys())
            .chain(project_ui.keys())
            .filter(|key| key.provider == provider && !key.is_ralphx_owned())
            .cloned()
            .collect::<HashSet<_>>();

        let mut launch = McpLaunchPolicy::default();
        let mut keys = keys.into_iter().collect::<Vec<_>>();
        keys.sort_by(|left, right| left.server_id.cmp(&right.server_id));
        for key in keys {
            let effective = resolve_layers(
                NativeMcpServerSnapshot {
                    key: key.clone(),
                    native_scope: None,
                    native_state: NativeMcpState::Unknown,
                    known_tools: Vec::new(),
                    diagnostic: None,
                },
                global_yaml.policies.get(&key),
                global_ui.get(&key),
                project_yaml.policies.get(&key),
                project_ui.get(&key),
                None,
            );
            if !effective.enabled {
                launch.disabled_servers.push(key.server_id.clone());
            }
            if !effective.disabled_tools.is_empty() {
                launch
                    .disabled_tools
                    .insert(key.server_id, effective.disabled_tools);
            }
        }
        Ok(launch)
    }

    pub(crate) async fn retry_reserved_claude_registration_repair(&self) -> AppResult<bool> {
        let (provider_config_root, provider_env) = self
            .provider_native_context(AgentHarnessKind::Claude)
            .await?;
        self.reconcile_reserved_claude_registration(&provider_config_root, &provider_env)
            .await
    }

    pub(crate) async fn reconcile_reserved_claude_registration_best_effort(
        &self,
    ) -> AppResult<bool> {
        let (provider_config_root, provider_env) = self
            .provider_native_context(AgentHarnessKind::Claude)
            .await?;
        self.reconcile_reserved_claude_registration(&provider_config_root, &provider_env)
            .await
    }

    async fn reconcile_reserved_claude_registration(
        &self,
        provider_config_root: &Path,
        provider_env: &HashMap<String, String>,
    ) -> AppResult<bool> {
        use crate::infrastructure::agents::claude::mcp_catalog::{
            classify_reserved_user_registration, ReservedClaudeUserRegistration,
        };

        let classification = classify_reserved_user_registration(provider_config_root)
            .map_err(|_| reserved_repair_preflight_error())?;
        match classification {
            ReservedClaudeUserRegistration::NotPresent => return Ok(false),
            ReservedClaudeUserRegistration::ReservedUserEntry => {}
        }

        let cli_path = self.resolve_claude_cleanup_cli().await?;
        let started_at = std::time::Instant::now();
        match crate::infrastructure::agents::claude::mcp_registration_repair::remove_reserved_user_registration(
            &cli_path,
            provider_config_root,
            provider_env,
        )
        .await
        {
            Ok(changed) => {
                tracing::info!(
                    provider = "claude",
                    server_id = "ralphx",
                    scope = "user",
                    changed,
                    elapsed_ms = started_at.elapsed().as_millis(),
                    "Reserved RalphX MCP registration reconciliation completed"
                );
                Ok(changed)
            }
            Err(code) => {
                tracing::warn!(
                    provider = "claude",
                    server_id = "ralphx",
                    scope = "user",
                    failure_code = %code,
                    elapsed_ms = started_at.elapsed().as_millis(),
                    "Reserved RalphX MCP registration reconciliation failed"
                );
                Err(reserved_repair_preflight_error())
            }
        }
    }

    async fn resolve_claude_cleanup_cli(&self) -> AppResult<PathBuf> {
        if let Some(path) = self.reserved_claude_mcp_cleanup_cli_override.as_ref() {
            return Ok(path.clone());
        }
        if let Some(repo) = self.provider_settings_repo.as_ref() {
            let settings = repo
                .get(AgentHarnessKind::Claude)
                .await
                .map_err(|_| reserved_repair_preflight_error())?;
            if let Some(settings) = settings {
                if let Some(path) =
                    crate::application::managed_provider_cli::checked_provider_cli_launch_path(
                        &settings,
                        "Claude reserved MCP cleanup",
                    )
                {
                    return path.map_err(|_| reserved_repair_preflight_error());
                }
            }
        }
        tokio::task::spawn_blocking(crate::infrastructure::agents::claude::find_claude_cli)
            .await
            .ok()
            .flatten()
            .ok_or_else(reserved_repair_preflight_error)
    }

    fn load_global_yaml(&self) -> AppResult<McpPolicyConfigSnapshot> {
        let root = self.global_policy_path.parent().ok_or_else(|| {
            AppError::Infrastructure("Global MCP policy has no owned root".to_string())
        })?;
        load_mcp_policy_file(root, &self.global_policy_path, None)
    }

    pub(crate) fn load_policy_configs(
        &self,
        project_id: Option<&str>,
        project_root: Option<&Path>,
    ) -> AppResult<(McpPolicyConfigSnapshot, McpPolicyConfigSnapshot)> {
        let global = self.load_global_yaml()?;
        let project = match project_root {
            Some(root) => {
                load_mcp_policy_file(root, &root.join(".ralphx").join("mcp.yaml"), project_id)?
            }
            None => McpPolicyConfigSnapshot::default(),
        };
        Ok((global, project))
    }

    pub async fn set_server_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        state: McpOverrideState,
    ) -> AppResult<McpPolicyOverride> {
        reject_follow_update(state)?;
        self.repo
            .set_server_state(project_id, key, state)
            .await
            .map_err(repository_error)
    }

    pub async fn set_tool_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
        state: McpOverrideState,
    ) -> AppResult<McpPolicyOverride> {
        reject_follow_update(state)?;
        self.repo
            .set_tool_state(project_id, key, tool_name, state)
            .await
            .map_err(repository_error)
    }

    pub async fn clear_server(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
    ) -> AppResult<bool> {
        self.repo
            .clear_server(project_id, key)
            .await
            .map_err(repository_error)
    }

    pub async fn clear_tool(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
    ) -> AppResult<bool> {
        self.repo
            .clear_tool(project_id, key, tool_name)
            .await
            .map_err(repository_error)
    }
}

fn reserved_repair_preflight_error() -> AppError {
    AppError::Infrastructure(
        McpSetupPreflightFailure::legacy_repair_failed().to_start_error_marker(),
    )
}

fn resolve_provider_native_config_root(
    provider: AgentHarnessKind,
    default_home: &Path,
    shell_env: &HashMap<String, String>,
    provider_env: &HashMap<String, String>,
) -> AppResult<PathBuf> {
    let root = match provider {
        AgentHarnessKind::Claude => default_home.to_path_buf(),
        AgentHarnessKind::Codex => provider_env
            .get("CODEX_HOME")
            .or_else(|| shell_env.get("CODEX_HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| default_home.join(".codex")),
    };
    crate::utils::path_safety::validate_absolute_non_root_path(
        &root,
        &format!("{provider} native MCP config root"),
    )
}

fn repository_error(error: impl std::fmt::Display) -> AppError {
    AppError::Infrastructure(format!("MCP policy repository failed: {error}"))
}

fn reject_follow_update(state: McpOverrideState) -> AppResult<()> {
    if state == McpOverrideState::Follow {
        return Err(AppError::Validation(
            "Follow must use the matching clear MCP policy operation".to_string(),
        ));
    }
    Ok(())
}

fn ensure_valid_policy_config(scope: &str, snapshot: &McpPolicyConfigSnapshot) -> AppResult<()> {
    if snapshot.diagnostics.is_empty() {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "Invalid {scope} MCP policy: {}",
        snapshot.diagnostics.join("; ")
    )))
}

fn policy_for<'a>(
    snapshot: Option<&'a McpPolicyConfigSnapshot>,
    key: &McpServerKey,
) -> Option<&'a McpPolicyOverride> {
    snapshot.and_then(|snapshot| snapshot.policies.get(key))
}

fn resolve_layers(
    native: NativeMcpServerSnapshot,
    global_yaml_direct: Option<&McpPolicyOverride>,
    global_ui: Option<&McpPolicyOverride>,
    project_yaml_direct: Option<&McpPolicyOverride>,
    project_ui: Option<&McpPolicyOverride>,
    project_yaml_snapshot: Option<&McpPolicyConfigSnapshot>,
) -> EffectiveMcpServerPolicy {
    let project_yaml =
        project_yaml_direct.or_else(|| policy_for(project_yaml_snapshot, &native.key));
    let layers = [
        (McpPolicySource::GlobalYaml, global_yaml_direct),
        (McpPolicySource::GlobalUi, global_ui),
        (McpPolicySource::ProjectYaml, project_yaml),
        (McpPolicySource::ProjectUi, project_ui),
    ];
    let mut requested_enabled = true;
    let mut requested_state = McpOverrideState::Follow;
    let mut server_source = McpPolicySource::ProviderNative;
    for (source, policy) in layers {
        let Some(policy) = policy else { continue };
        match policy.server_state {
            McpOverrideState::Follow => {}
            McpOverrideState::Enabled => {
                requested_enabled = true;
                requested_state = McpOverrideState::Enabled;
                server_source = source;
            }
            McpOverrideState::Disabled => {
                requested_enabled = false;
                requested_state = McpOverrideState::Disabled;
                server_source = source;
            }
        }
    }

    let mut tools = native.known_tools.iter().cloned().collect::<BTreeSet<_>>();
    for (_, policy) in layers {
        if let Some(policy) = policy {
            tools.extend(policy.tool_states.keys().cloned());
        }
    }
    let mut tool_states = BTreeMap::new();
    let mut tool_sources = BTreeMap::new();
    let mut disabled_tools = Vec::new();
    for tool in tools {
        let mut state = McpOverrideState::Follow;
        let mut source = McpPolicySource::ProviderNative;
        for (candidate_source, policy) in layers {
            let Some(candidate) = policy.and_then(|row| row.tool_states.get(&tool)) else {
                continue;
            };
            if *candidate != McpOverrideState::Follow {
                state = *candidate;
                source = candidate_source;
            }
        }
        if state == McpOverrideState::Disabled {
            disabled_tools.push(tool.clone());
        }
        tool_states.insert(tool.clone(), state);
        tool_sources.insert(tool, source);
    }

    EffectiveMcpServerPolicy {
        enabled: native.native_state.permits_launch() && requested_enabled,
        server_state: requested_state,
        native,
        server_source,
        tool_states,
        tool_sources,
        disabled_tools,
        locked: false,
        locked_reason: None,
    }
}

#[cfg(test)]
pub(crate) fn resolve_layers_for_test(
    native: NativeMcpServerSnapshot,
    global_yaml: Option<&McpPolicyOverride>,
    global_ui: Option<&McpPolicyOverride>,
    project_yaml: Option<&McpPolicyOverride>,
    project_ui: Option<&McpPolicyOverride>,
) -> EffectiveMcpServerPolicy {
    resolve_layers(
        native,
        global_yaml,
        global_ui,
        project_yaml,
        project_ui,
        None,
    )
}

#[cfg(test)]
pub(crate) fn resolve_provider_native_config_root_for_test(
    provider: AgentHarnessKind,
    default_home: &Path,
    shell_env: &HashMap<String, String>,
    provider_env: &HashMap<String, String>,
) -> AppResult<PathBuf> {
    resolve_provider_native_config_root(provider, default_home, shell_env, provider_env)
}
