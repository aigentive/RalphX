use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::agents::{
    AgentHarnessKind, EffectiveMcpServerPolicy, McpLaunchPolicy, McpOverrideState,
    McpPolicyOverride, McpPolicySource, McpServerKey, NativeMcpServerSnapshot, NativeMcpState,
};
use crate::domain::repositories::McpPolicyRepository;
use crate::error::{AppError, AppResult};

use super::mcp_policy_config::{load_mcp_policy_file, McpPolicyConfigSnapshot};

#[derive(Clone)]
pub struct McpPolicyService {
    repo: Arc<dyn McpPolicyRepository>,
    global_policy_path: PathBuf,
}

impl McpPolicyService {
    pub fn new(repo: Arc<dyn McpPolicyRepository>, global_policy_path: PathBuf) -> Self {
        Self {
            repo,
            global_policy_path,
        }
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

        let global_yaml = self.load_global_yaml()?;
        let project_yaml = match project_root {
            Some(root) => {
                load_mcp_policy_file(root, &root.join(".ralphx").join("mcp.yaml"), project_id)?
            }
            None => McpPolicyConfigSnapshot::default(),
        };
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
        let policy_root = self.global_policy_path.parent().ok_or_else(|| {
            AppError::Infrastructure("Global MCP policy has no owned root".to_string())
        })?;
        let provider_home = if policy_root
            .file_name()
            .is_some_and(|name| name == ".ralphx")
        {
            policy_root.parent().ok_or_else(|| {
                AppError::Infrastructure("Global MCP policy has no provider home".to_string())
            })?
        } else {
            policy_root
        };
        crate::infrastructure::agents::ensure_no_reserved_native_mcp_collision_at(
            provider,
            provider_home,
            project_root,
        )
        .map_err(|error| {
            AppError::Infrastructure(format!("MCP launch preflight failed: {error}"))
        })?;
        let global_yaml = self.load_global_yaml()?;
        let project_yaml = match project_root {
            Some(root) => {
                load_mcp_policy_file(root, &root.join(".ralphx").join("mcp.yaml"), project_id)?
            }
            None => McpPolicyConfigSnapshot::default(),
        };
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

    fn load_global_yaml(&self) -> AppResult<McpPolicyConfigSnapshot> {
        let root = self.global_policy_path.parent().ok_or_else(|| {
            AppError::Infrastructure("Global MCP policy has no owned root".to_string())
        })?;
        load_mcp_policy_file(root, &self.global_policy_path, None)
    }

    pub async fn set_server_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        state: McpOverrideState,
    ) -> AppResult<McpPolicyOverride> {
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

fn repository_error(error: impl std::fmt::Display) -> AppError {
    AppError::Infrastructure(format!("MCP policy repository failed: {error}"))
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
