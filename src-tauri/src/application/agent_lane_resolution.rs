use std::path::Path;
use std::sync::Arc;

use crate::domain::agents::{
    default_approval_policy_for_harness, default_sandbox_mode_for_harness,
    generic_harness_lane_defaults, generic_harness_role_defaults, AgentHarnessKind, AgentLane,
    AgentLaneSettings, LogicalEffort, ManualRoleDefault, ManualRoleRuntimeOverride,
    ManualServiceTier, RoutingRole, RoutingRoleFamily, StoredAgentLaneSettings,
    DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{AgentConversationWorkspaceMode, ChatContextType, RuntimeSource};
use crate::domain::repositories::AgentLaneSettingsRepository;
use crate::error::AppResult;
use crate::infrastructure::agents::claude::{canonical_short_agent_name, resolve_model};

use super::manual_role_default_service::{ManualDefaultSource, ManualRoleDefaultService};

/// Map a provider-backed chat launch to the backend-owned semantic routing role.
///
/// Canonical agent identity handles specialist launches while typed context,
/// workspace mode, and ideation purpose disambiguate agents reused by more than
/// one workflow. Model or frontend input never supplies this value.
pub fn routing_role_for_chat_launch(
    agent_name: &str,
    context_type: ChatContextType,
    entity_status: Option<&str>,
    workspace_mode: Option<AgentConversationWorkspaceMode>,
    ideation_verification: bool,
) -> RoutingRole {
    let agent_name = canonical_short_agent_name(agent_name);
    let specialist = match agent_name {
        "ralphx-automation-plan-judge" => Some(RoutingRole::AutomationPlanJudge),
        "ralphx-automation-judge" | "ralphx-automation-decomposition-verifier" => {
            Some(RoutingRole::AutomationResultJudge)
        }
        "ralphx-workspace-reviewer" | "ralphx-review-chat" | "ralphx-review-history" => {
            Some(RoutingRole::WorkspaceReviewer)
        }
        "ralphx-agent-workspace-repair" if context_type == ChatContextType::Merge => {
            Some(RoutingRole::WorkspaceMergeRepair)
        }
        "ralphx-agent-workspace-repair" | "ralphx-execution-branch-updater" => {
            Some(RoutingRole::WorkspaceRepair)
        }
        "ralphx-agent-workspace-pr-fixer" => Some(RoutingRole::WorkspacePrFixer),
        "ralphx-execution-coder" | "ralphx-research-deep-researcher" => {
            Some(RoutingRole::DelegatedSubagent)
        }
        "ralphx-qa-prep" => Some(RoutingRole::ExecutionQaPrep),
        "qa-refiner" => Some(RoutingRole::ExecutionQaRefiner),
        "qa-tester" | "ralphx-qa-executor" => Some(RoutingRole::ExecutionQaTester),
        "ralphx-utility-pr-describer" => Some(RoutingRole::UtilityPrDescriber),
        "ralphx-project-analyzer" => Some(RoutingRole::UtilityProjectAnalyzer),
        "ralphx-memory-capture" => Some(RoutingRole::MemoryCapture),
        "ralphx-memory-maintainer" => Some(RoutingRole::MemoryMaintainer),
        "ralphx-utility-session-namer"
        | "ralphx-utility-plan-complexity"
        | "ralphx-persona-extractor" => Some(RoutingRole::UtilityLightweight),
        _ => None,
    };
    if let Some(role) = specialist {
        return role;
    }

    match context_type {
        ChatContextType::Project => match workspace_mode {
            Some(AgentConversationWorkspaceMode::Chat) => RoutingRole::WorkspaceChat,
            Some(AgentConversationWorkspaceMode::Edit) => RoutingRole::WorkspaceEdit,
            Some(AgentConversationWorkspaceMode::Plan) => RoutingRole::WorkspacePlan,
            Some(AgentConversationWorkspaceMode::Tasks) => RoutingRole::UtilityLightweight,
            Some(AgentConversationWorkspaceMode::Autopilot) => RoutingRole::WorkspaceIdeation,
            Some(AgentConversationWorkspaceMode::Ideation) => RoutingRole::WorkspaceIdeation,
            Some(AgentConversationWorkspaceMode::ReviewPr) => RoutingRole::WorkspaceReviewPr,
            Some(AgentConversationWorkspaceMode::Automation) => RoutingRole::WorkspaceAutomation,
            Some(AgentConversationWorkspaceMode::PersonaBuilder) => RoutingRole::UtilityLightweight,
            None => match agent_name {
                "ralphx-general-worker" => RoutingRole::WorkspaceEdit,
                "ralphx-ideation" | "ralphx-ideation-readonly" => RoutingRole::WorkspacePlan,
                "ralphx-pr-reviewer" => RoutingRole::WorkspaceReviewPr,
                "ralphx-automation-setup" => RoutingRole::WorkspaceAutomation,
                _ => RoutingRole::WorkspaceChat,
            },
        },
        ChatContextType::Standalone => match workspace_mode {
            Some(AgentConversationWorkspaceMode::PersonaBuilder) => RoutingRole::UtilityLightweight,
            _ => RoutingRole::WorkspaceChat,
        },
        ChatContextType::Ideation => {
            if ideation_verification {
                RoutingRole::IdeationVerifier
            } else {
                RoutingRole::IdeationPrimary
            }
        }
        ChatContextType::Delegation => RoutingRole::DelegatedSubagent,
        ChatContextType::Task => RoutingRole::UtilityLightweight,
        ChatContextType::TaskExecution => {
            if matches!(entity_status, Some("re_executing")) {
                RoutingRole::ExecutionReexecutor
            } else {
                RoutingRole::ExecutionWorker
            }
        }
        ChatContextType::Review => RoutingRole::ExecutionReviewer,
        ChatContextType::Merge => RoutingRole::ExecutionMerger,
        ChatContextType::BranchUpdate => RoutingRole::WorkspaceRepair,
    }
}

/// Map a delegated chat launch using its backend-owned parent context.
pub fn routing_role_for_delegated_launch(
    agent_name: &str,
    parent_context_type: ChatContextType,
    ideation_verification: bool,
) -> RoutingRole {
    if parent_context_type == ChatContextType::Ideation {
        if ideation_verification {
            RoutingRole::IdeationVerifierSubagent
        } else {
            RoutingRole::IdeationSubagent
        }
    } else {
        routing_role_for_chat_launch(agent_name, ChatContextType::Delegation, None, None, false)
    }
}

/// Map state-machine spawner identifiers to semantic roles.
pub fn routing_role_for_spawner_agent(
    agent_type: &str,
    entity_status: Option<&str>,
) -> Option<RoutingRole> {
    match agent_type {
        "worker" | "ralphx-execution-worker" => {
            if matches!(entity_status, Some("re_executing")) {
                Some(RoutingRole::ExecutionReexecutor)
            } else {
                Some(RoutingRole::ExecutionWorker)
            }
        }
        "coder" | "ralphx-execution-coder" => Some(RoutingRole::DelegatedSubagent),
        "qa-prep" => Some(RoutingRole::ExecutionQaPrep),
        "qa-refiner" => Some(RoutingRole::ExecutionQaRefiner),
        "qa-tester" => Some(RoutingRole::ExecutionQaTester),
        "reviewer" | "ralphx-execution-reviewer" => Some(RoutingRole::ExecutionReviewer),
        "merger" | "ralphx-execution-merger" => Some(RoutingRole::ExecutionMerger),
        "branch-updater" | "ralphx-execution-branch-updater" => Some(RoutingRole::WorkspaceRepair),
        _ => None,
    }
}

#[async_trait::async_trait]
impl crate::infrastructure::agents::spawner::StateMachineRoleResolver for ManualRoleDefaultService {
    async fn resolve_state_machine_role(
        &self,
        agent_name: &str,
        agent_type: &str,
        entity_status: Option<&str>,
        project_id: Option<&str>,
        project_root: Option<&Path>,
    ) -> Result<Option<crate::infrastructure::agents::spawner::StateMachineRoleSettings>, String>
    {
        let Some(role) = routing_role_for_spawner_agent(agent_type, entity_status) else {
            return Ok(None);
        };
        let resolved = resolve_manual_role_spawn_settings(
            agent_name,
            project_id,
            project_root,
            role,
            None,
            None,
            None,
            self,
        )
        .await
        .map_err(|error| format!("Failed to resolve manual default for {role}: {error}"))?;
        Ok(Some(
            crate::infrastructure::agents::spawner::StateMachineRoleSettings {
                harness: resolved.effective_harness,
                model: resolved.model,
                logical_effort: resolved.logical_effort,
                approval_policy: resolved.approval_policy,
                sandbox_mode: resolved.sandbox_mode,
                service_tier: resolved.service_tier,
            },
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[doc(hidden)]
pub struct ResolvedAgentSpawnSettings {
    pub configured_harness: Option<AgentHarnessKind>,
    pub effective_harness: AgentHarnessKind,
    pub configured_model: Option<String>,
    pub configured_logical_effort: Option<LogicalEffort>,
    pub configured_approval_policy: Option<String>,
    pub configured_sandbox_mode: Option<String>,
    pub configured_service_tier: Option<String>,
    pub model: String,
    pub logical_effort: Option<LogicalEffort>,
    pub claude_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub service_tier: Option<String>,
    pub configured_subagent_model_cap: Option<String>,
    pub subagent_model_cap: Option<String>,
    pub runtime_source: RuntimeSource,
}

/// Integration-test seam (doc-hidden): suites resolve spawn settings to feed
/// the launch-plan test helper.
#[doc(hidden)]
pub async fn resolve_agent_spawn_settings(
    agent_name: &str,
    project_id: Option<&str>,
    context_type: ChatContextType,
    entity_status: Option<&str>,
    harness_override: Option<AgentHarnessKind>,
    model_override: Option<&str>,
    agent_lane_settings_repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
) -> ResolvedAgentSpawnSettings {
    let primary_lane = lane_for_context(agent_name, context_type, entity_status);
    let subagent_lane = subagent_lane_for_context(agent_name, context_type);

    if primary_lane.is_none() {
        let effective_harness = harness_override.unwrap_or(DEFAULT_AGENT_HARNESS);
        let non_lane_defaults = non_lane_harness_defaults(effective_harness);
        return ResolvedAgentSpawnSettings {
            configured_harness: None,
            effective_harness,
            configured_model: None,
            configured_logical_effort: None,
            configured_approval_policy: None,
            configured_sandbox_mode: None,
            configured_service_tier: None,
            model: model_override
                .map(str::to_string)
                .or_else(|| {
                    non_lane_defaults
                        .as_ref()
                        .and_then(|settings| settings.model.clone())
                })
                .unwrap_or_else(|| {
                    if effective_harness == AgentHarnessKind::Claude {
                        resolve_model(Some(agent_name))
                    } else {
                        crate::domain::agents::default_model_for_provider(effective_harness)
                            .to_string()
                    }
                }),
            logical_effort: None,
            claude_effort: None,
            approval_policy: default_approval_policy_for_harness(effective_harness)
                .map(str::to_string)
                .or_else(|| {
                    non_lane_defaults
                        .as_ref()
                        .and_then(|settings| settings.approval_policy.clone())
                }),
            sandbox_mode: default_sandbox_mode_for_harness(effective_harness)
                .map(str::to_string)
                .or_else(|| {
                    non_lane_defaults
                        .as_ref()
                        .and_then(|settings| settings.sandbox_mode.clone())
                }),
            service_tier: None,
            configured_subagent_model_cap: None,
            subagent_model_cap: None,
            runtime_source: RuntimeSource::HarnessFallback,
        };
    }

    let (primary_project_row, primary_global_row) =
        load_lane_rows(agent_lane_settings_repo, project_id, primary_lane).await;
    let configured_harness =
        lane_harness(primary_project_row.as_ref(), primary_global_row.as_ref());
    let effective_harness = harness_override
        .or(configured_harness)
        .unwrap_or(DEFAULT_AGENT_HARNESS);
    let settings_match_effective_harness = configured_harness
        .map(|configured| configured == effective_harness)
        .unwrap_or(true);
    let configured_primary_settings = settings_match_effective_harness
        .then(|| lane_settings_value(primary_project_row.as_ref(), primary_global_row.as_ref()))
        .flatten();
    let configured_harness =
        configured_harness.filter(|configured| *configured == effective_harness);
    let harness_primary_defaults =
        primary_lane.and_then(|lane| nondefault_harness_lane_settings(lane, effective_harness));

    let model = if let Some(model_override) = model_override {
        model_override.to_string()
    } else if let Some(model) = configured_primary_settings
        .as_ref()
        .and_then(|settings| settings.model.clone())
    {
        model
    } else if let Some(model) = harness_primary_defaults
        .as_ref()
        .and_then(|settings| settings.model.clone())
    {
        model
    } else {
        resolve_model(Some(agent_name))
    };

    let logical_effort = if primary_lane.is_some() {
        if let Some(effort) = configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.effort)
        {
            Some(effort)
        } else if let Some(defaults) = harness_primary_defaults.as_ref() {
            defaults.effort
        } else {
            None
        }
    } else {
        None
    };

    let (configured_subagent_model_cap, subagent_model_cap) =
        if let Some(subagent_lane) = subagent_lane {
            let (subagent_project_row, subagent_global_row) =
                load_lane_rows(agent_lane_settings_repo, project_id, Some(subagent_lane)).await;
            let subagent_harness =
                lane_harness(subagent_project_row.as_ref(), subagent_global_row.as_ref());
            let configured_subagent_model_cap = subagent_harness
                .map(|configured| configured == effective_harness)
                .unwrap_or(true)
                .then(|| {
                    lane_settings_value(subagent_project_row.as_ref(), subagent_global_row.as_ref())
                        .and_then(|settings| settings.model)
                })
                .flatten();

            let subagent_model_cap = if let Some(model) = configured_subagent_model_cap.clone() {
                model
            } else if let Some(model) =
                nondefault_harness_lane_settings(subagent_lane, effective_harness)
                    .and_then(|settings| settings.model)
            {
                model
            } else {
                "haiku".to_string()
            };

            (configured_subagent_model_cap, Some(subagent_model_cap))
        } else {
            (None, None)
        };

    ResolvedAgentSpawnSettings {
        configured_harness,
        effective_harness,
        configured_model: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.model.clone()),
        configured_logical_effort: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.effort),
        configured_approval_policy: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.approval_policy.clone()),
        configured_sandbox_mode: configured_primary_settings
            .as_ref()
            .and_then(|settings| settings.sandbox_mode.clone()),
        configured_service_tier: None,
        model,
        logical_effort,
        claude_effort: logical_effort.map(|effort| effort.to_legacy_claude_effort().to_string()),
        approval_policy: default_approval_policy_for_harness(effective_harness)
            .map(str::to_string)
            .or_else(|| {
                configured_primary_settings
                    .as_ref()
                    .and_then(|settings| settings.approval_policy.clone())
            })
            .or_else(|| {
                harness_primary_defaults
                    .as_ref()
                    .and_then(|settings| settings.approval_policy.clone())
            }),
        sandbox_mode: default_sandbox_mode_for_harness(effective_harness)
            .map(str::to_string)
            .or_else(|| {
                configured_primary_settings
                    .as_ref()
                    .and_then(|settings| settings.sandbox_mode.clone())
            })
            .or_else(|| {
                harness_primary_defaults
                    .as_ref()
                    .and_then(|settings| settings.sandbox_mode.clone())
            }),
        service_tier: None,
        configured_subagent_model_cap,
        subagent_model_cap,
        runtime_source: if primary_project_row.is_some() {
            RuntimeSource::ProjectDefault
        } else if primary_global_row.is_some() {
            RuntimeSource::RoleDefault
        } else {
            RuntimeSource::HarnessFallback
        },
    }
}

/// Resolve one exact semantic role default for a production provider launch.
///
/// Unlike the legacy lane compatibility function, repository and config errors
/// are returned to the caller so an explicit broken default cannot degrade into
/// a guessed provider launch.
#[allow(clippy::too_many_arguments)]
pub async fn resolve_manual_role_spawn_settings(
    agent_name: &str,
    project_id: Option<&str>,
    project_root: Option<&Path>,
    role: RoutingRole,
    runtime_override: Option<&ManualRoleRuntimeOverride>,
    harness_override: Option<AgentHarnessKind>,
    model_override: Option<&str>,
    service: &ManualRoleDefaultService,
) -> AppResult<ResolvedAgentSpawnSettings> {
    if runtime_override.is_some() && (harness_override.is_some() || model_override.is_some()) {
        return Err(crate::error::AppError::Validation(
            "A complete role runtime override cannot be mixed with legacy provider or model overrides"
                .to_string(),
        ));
    }
    let resolved = if runtime_override.is_some() {
        service
            .resolve_for_explicit_runtime(project_id, project_root, role)
            .await?
    } else {
        service.resolve(project_id, project_root, role).await?
    };
    let selected_runtime = if let Some(runtime_override) = runtime_override {
        Some(ManualRoleDefault {
            harness: runtime_override.harness,
            model: runtime_override.model.clone(),
            effort: runtime_override.effort,
            service_tier: runtime_override.service_tier,
            coordination_mode: runtime_override.coordination_mode,
            persona_id: runtime_override.persona_id.clone(),
            approval_policy: resolved.value.approval_policy.clone(),
            sandbox_mode: resolved.value.sandbox_mode.clone(),
        })
    } else {
        None
    };
    let effective_harness = selected_runtime
        .as_ref()
        .map(|value| value.harness)
        .or(harness_override)
        .unwrap_or(resolved.value.harness);
    let selected_provider = if selected_runtime.is_some() {
        Some(
            service
                .resolve_enabled_provider_settings(effective_harness, "manual role runtime")
                .await?,
        )
    } else {
        None
    };
    let settings_match_effective_harness = resolved.value.harness == effective_harness;
    let utility_legacy_harness_only = resolved.source == ManualDefaultSource::LegacyLane
        && role.metadata().family == RoutingRoleFamily::Utility;
    let configured = settings_match_effective_harness
        .then_some(&resolved.value)
        .filter(|_| {
            resolved.source != ManualDefaultSource::ProviderDefault && !utility_legacy_harness_only
        });
    let selected = settings_match_effective_harness
        .then_some(&resolved.value)
        .filter(|_| !utility_legacy_harness_only);
    let model_and_effort = if role == RoutingRole::DelegatedSubagent {
        selected
    } else {
        configured
    };
    let harness_defaults = manual_role_harness_defaults(role, effective_harness);

    let model = selected_runtime
        .as_ref()
        .and_then(|value| value.model.clone())
        .or_else(|| {
            selected_provider
                .as_ref()
                .and_then(|provider| provider.model.clone())
        })
        .or_else(|| model_override.map(str::to_string))
        .or_else(|| {
            selected_runtime
                .is_none()
                .then(|| model_and_effort.and_then(|value| value.model.clone()))
                .flatten()
        })
        .or_else(|| {
            harness_defaults
                .as_ref()
                .and_then(|settings| settings.model.clone())
        })
        .unwrap_or_else(|| resolve_model(Some(agent_name)));
    if selected_runtime.is_some() {
        validate_model_harness_compatibility(effective_harness, &model)
            .map_err(crate::error::AppError::Validation)?;
    }
    let logical_effort = selected_runtime
        .as_ref()
        .and_then(|value| value.effort)
        .or_else(|| {
            selected_provider
                .as_ref()
                .and_then(|provider| provider.effort)
        })
        .or_else(|| {
            selected_runtime
                .is_none()
                .then(|| model_and_effort.and_then(|value| value.effort))
                .flatten()
        })
        .or_else(|| {
            harness_defaults
                .as_ref()
                .and_then(|settings| settings.effort)
        });
    let service_tier = match selected_runtime.as_ref() {
        Some(value) => match value.service_tier {
            ManualServiceTier::ProviderDefault => selected_provider
                .as_ref()
                .and_then(|provider| provider.service_tier.as_deref())
                .and_then(manual_provider_service_tier),
            tier => manual_service_tier(tier),
        },
        None => selected.and_then(|value| manual_service_tier(value.service_tier)),
    };
    if let Some(selected_runtime) = selected_runtime.as_ref() {
        let complete_runtime = ManualRoleDefault {
            harness: effective_harness,
            model: Some(model.clone()),
            effort: logical_effort,
            service_tier: manual_service_tier_from_resolved(service_tier.as_deref()),
            coordination_mode: selected_runtime.coordination_mode,
            persona_id: selected_runtime.persona_id.clone(),
            approval_policy: selected_runtime.approval_policy.clone(),
            sandbox_mode: selected_runtime.sandbox_mode.clone(),
        };
        service
            .validate_explicit_value(role, &complete_runtime)
            .await?;
        validate_model_harness_compatibility(effective_harness, &model)
            .map_err(crate::error::AppError::Validation)?;
    }
    let (configured_subagent_model_cap, subagent_model_cap) =
        resolve_manual_subagent_model(project_id, project_root, role, effective_harness, service)
            .await?;

    Ok(ResolvedAgentSpawnSettings {
        configured_harness: configured.map(|value| value.harness),
        effective_harness,
        configured_model: configured.and_then(|value| value.model.clone()),
        configured_logical_effort: configured.and_then(|value| value.effort),
        configured_approval_policy: configured.and_then(|value| value.approval_policy.clone()),
        configured_sandbox_mode: configured.and_then(|value| value.sandbox_mode.clone()),
        configured_service_tier: configured
            .and_then(|value| manual_service_tier(value.service_tier)),
        model,
        logical_effort,
        claude_effort: logical_effort.map(|effort| effort.to_legacy_claude_effort().to_string()),
        approval_policy: default_approval_policy_for_harness(effective_harness)
            .map(str::to_string)
            .or_else(|| selected.and_then(|value| value.approval_policy.clone()))
            .or_else(|| {
                harness_defaults
                    .as_ref()
                    .and_then(|settings| settings.approval_policy.clone())
            }),
        sandbox_mode: default_sandbox_mode_for_harness(effective_harness)
            .map(str::to_string)
            .or_else(|| selected.and_then(|value| value.sandbox_mode.clone()))
            .or_else(|| {
                harness_defaults
                    .as_ref()
                    .and_then(|settings| settings.sandbox_mode.clone())
            }),
        service_tier,
        configured_subagent_model_cap,
        subagent_model_cap,
        runtime_source: if runtime_override.is_some() {
            RuntimeSource::ConversationOverride
        } else {
            runtime_source_for_manual_default(resolved.source)
        },
    })
}

fn runtime_source_for_manual_default(source: ManualDefaultSource) -> RuntimeSource {
    match source {
        ManualDefaultSource::ProviderDefault => RuntimeSource::HarnessFallback,
        ManualDefaultSource::ProjectUi
        | ManualDefaultSource::ProjectYaml
        | ManualDefaultSource::GlobalUi
        | ManualDefaultSource::GlobalYaml
        | ManualDefaultSource::LegacyLane
        | ManualDefaultSource::LegacyWorkspaceReview => RuntimeSource::RoleDefault,
    }
}

fn manual_role_harness_defaults(
    role: RoutingRole,
    harness: AgentHarnessKind,
) -> Option<AgentLaneSettings> {
    if role == RoutingRole::WorkspacePlan || role.metadata().family == RoutingRoleFamily::Utility {
        return Some(generic_harness_role_defaults(harness, role));
    }

    let lane = role.legacy_lane().or(match role {
        RoutingRole::ExecutionQaPrep
        | RoutingRole::ExecutionQaRefiner
        | RoutingRole::ExecutionQaTester => Some(AgentLane::ExecutionWorker),
        _ => None,
    });
    if let Some(lane) = lane {
        return nondefault_harness_lane_settings(lane, harness);
    }

    (harness != DEFAULT_AGENT_HARNESS).then(|| generic_harness_role_defaults(harness, role))
}

/// Reject a model only when the built-in catalog positively assigns it to a
/// different harness. Unknown/custom aliases remain eligible for provider-side
/// validation.
#[doc(hidden)]
pub fn validate_model_harness_compatibility(
    harness: AgentHarnessKind,
    model: &str,
) -> Result<(), String> {
    let owners = crate::domain::agents::built_in_agent_models()
        .into_iter()
        .filter(|definition| definition.model_id == model)
        .map(|definition| definition.provider)
        .collect::<std::collections::HashSet<_>>();
    if owners.is_empty() || owners.contains(&harness) {
        return Ok(());
    }

    let owner = owners
        .iter()
        .next()
        .copied()
        .expect("non-empty model owner set");
    Err(format!(
        "Model '{model}' belongs to the {owner} harness and cannot launch with {harness}"
    ))
}

async fn resolve_manual_subagent_model(
    project_id: Option<&str>,
    project_root: Option<&Path>,
    role: RoutingRole,
    primary_harness: AgentHarnessKind,
    service: &ManualRoleDefaultService,
) -> AppResult<(Option<String>, Option<String>)> {
    let subagent_role = match role {
        RoutingRole::IdeationPrimary => RoutingRole::IdeationSubagent,
        RoutingRole::IdeationVerifier => RoutingRole::IdeationVerifierSubagent,
        _ => return Ok((None, None)),
    };
    let resolved = service
        .resolve(project_id, project_root, subagent_role)
        .await?;
    let compatible = resolved.value.harness == primary_harness;
    let configured = (compatible && resolved.source != ManualDefaultSource::ProviderDefault)
        .then(|| resolved.value.model.clone())
        .flatten();
    let effective = if compatible && resolved.source != ManualDefaultSource::ProviderDefault {
        resolved.value.model
    } else {
        nondefault_harness_lane_settings(
            subagent_role
                .legacy_lane()
                .expect("ideation subagent roles have legacy compatibility lanes"),
            primary_harness,
        )
        .and_then(|settings| settings.model)
        .or_else(|| Some("haiku".to_string()))
    };
    Ok((configured, effective))
}

fn manual_service_tier(tier: ManualServiceTier) -> Option<String> {
    match tier {
        ManualServiceTier::ProviderDefault => None,
        ManualServiceTier::Standard => Some("standard".to_string()),
        ManualServiceTier::Fast => Some("fast".to_string()),
    }
}

fn manual_provider_service_tier(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn manual_service_tier_from_resolved(service_tier: Option<&str>) -> ManualServiceTier {
    match service_tier {
        Some(value) if value.eq_ignore_ascii_case("fast") => ManualServiceTier::Fast,
        Some(_) => ManualServiceTier::Standard,
        None => ManualServiceTier::ProviderDefault,
    }
}

fn ideation_lane_for_agent(agent_name: &str) -> Option<AgentLane> {
    let normalized = canonical_short_agent_name(agent_name);
    match normalized {
        "ralphx-ideation" | "ralphx-ideation-readonly" => Some(AgentLane::IdeationPrimary),
        _ => None,
    }
}

fn execution_lane_for_context(
    context_type: ChatContextType,
    entity_status: Option<&str>,
) -> Option<AgentLane> {
    match context_type {
        ChatContextType::TaskExecution => {
            if matches!(entity_status, Some("re_executing")) {
                Some(AgentLane::ExecutionReexecutor)
            } else {
                Some(AgentLane::ExecutionWorker)
            }
        }
        ChatContextType::Review => Some(AgentLane::ExecutionReviewer),
        ChatContextType::Merge => Some(AgentLane::ExecutionMerger),
        ChatContextType::BranchUpdate => Some(AgentLane::ExecutionBranchUpdater),
        ChatContextType::Ideation
        | ChatContextType::Delegation
        | ChatContextType::Task
        | ChatContextType::Project
        | ChatContextType::Standalone => None,
    }
}

fn lane_for_context(
    agent_name: &str,
    context_type: ChatContextType,
    entity_status: Option<&str>,
) -> Option<AgentLane> {
    match context_type {
        ChatContextType::Ideation => ideation_lane_for_agent(agent_name),
        _ => execution_lane_for_context(context_type, entity_status),
    }
}

fn ideation_subagent_lane_for_agent(agent_name: &str) -> Option<AgentLane> {
    ideation_lane_for_agent(agent_name).map(|lane| match lane {
        AgentLane::IdeationVerifier => AgentLane::IdeationVerifierSubagent,
        AgentLane::IdeationPrimary => AgentLane::IdeationSubagent,
        _ => unreachable!("ideation lane mapper returned a non-ideation lane"),
    })
}

fn subagent_lane_for_context(agent_name: &str, context_type: ChatContextType) -> Option<AgentLane> {
    match context_type {
        ChatContextType::Ideation => ideation_subagent_lane_for_agent(agent_name),
        _ => None,
    }
}

async fn load_lane_rows(
    repo: Option<&Arc<dyn AgentLaneSettingsRepository>>,
    project_id: Option<&str>,
    lane: Option<AgentLane>,
) -> (
    Option<StoredAgentLaneSettings>,
    Option<StoredAgentLaneSettings>,
) {
    let Some(repo) = repo else {
        return (None, None);
    };
    let Some(lane) = lane else {
        return (None, None);
    };

    let project_row = if let Some(project_id) = project_id {
        repo.get_for_project(project_id, lane)
            .await
            .inspect_err(|error| {
                tracing::warn!(
                    %project_id,
                    lane = %lane,
                    %error,
                    "Failed to fetch project-scoped agent lane settings"
                );
            })
            .ok()
            .flatten()
    } else {
        None
    };

    let global_row = repo
        .get_global(lane)
        .await
        .inspect_err(|error| {
            tracing::warn!(
                lane = %lane,
                %error,
                "Failed to fetch global agent lane settings"
            );
        })
        .ok()
        .flatten();

    (project_row, global_row)
}

fn lane_settings_value(
    project_row: Option<&StoredAgentLaneSettings>,
    global_row: Option<&StoredAgentLaneSettings>,
) -> Option<AgentLaneSettings> {
    project_row
        .map(|row| row.settings.clone())
        .or_else(|| global_row.map(|row| row.settings.clone()))
}

fn lane_harness(
    project_row: Option<&StoredAgentLaneSettings>,
    global_row: Option<&StoredAgentLaneSettings>,
) -> Option<AgentHarnessKind> {
    project_row
        .map(|row| row.settings.harness)
        .or_else(|| global_row.map(|row| row.settings.harness))
}

fn nondefault_harness_lane_settings(
    lane: AgentLane,
    harness: AgentHarnessKind,
) -> Option<AgentLaneSettings> {
    if harness == DEFAULT_AGENT_HARNESS {
        return None;
    }

    Some(generic_harness_lane_defaults(harness, lane))
}

fn non_lane_harness_defaults(harness: AgentHarnessKind) -> Option<AgentLaneSettings> {
    nondefault_harness_lane_settings(AgentLane::IdeationPrimary, harness)
}
