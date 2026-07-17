use std::sync::Arc;

use super::agent_lane_resolution::{
    resolve_agent_spawn_settings, resolve_manual_role_spawn_settings, routing_role_for_chat_launch,
    routing_role_for_delegated_launch, routing_role_for_spawner_agent,
};
use super::manual_role_default_service::ManualRoleDefaultService;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort, ManualRoleDefault,
    ManualServiceTier, RoutingRole,
};
use crate::domain::entities::{AgentConversationWorkspaceMode, ChatContextType};
use crate::domain::repositories::{
    AgentLaneSettingsRepository, AgentProviderSettingsRepository, ManualRoleDefaultRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentLaneSettingsRepository, MemoryAgentProviderSettingsRepository,
    MemoryManualRoleDefaultRepository, MemoryPersonaRepository,
};

fn claude_lane_settings(
    model: &str,
    effort: Option<LogicalEffort>,
    approval_policy: Option<&str>,
    sandbox_mode: Option<&str>,
) -> AgentLaneSettings {
    AgentLaneSettings {
        harness: AgentHarnessKind::Claude,
        model: Some(model.to_string()),
        effort,
        approval_policy: approval_policy.map(str::to_string),
        sandbox_mode: sandbox_mode.map(str::to_string),
    }
}

fn codex_lane_settings(
    model: &str,
    effort: Option<LogicalEffort>,
    approval_policy: Option<&str>,
    sandbox_mode: Option<&str>,
) -> AgentLaneSettings {
    AgentLaneSettings {
        harness: AgentHarnessKind::Codex,
        model: Some(model.to_string()),
        effort,
        approval_policy: approval_policy.map(str::to_string),
        sandbox_mode: sandbox_mode.map(str::to_string),
    }
}

#[tokio::test]
async fn lane_row_with_claude_harness_overrides_model_and_effort() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_for_project(
            "proj-1",
            AgentLane::IdeationPrimary,
            &claude_lane_settings(
                "opus",
                Some(LogicalEffort::XHigh),
                Some("on_request"),
                Some("workspace_write"),
            ),
        )
        .await
        .expect("lane upsert should succeed");
    lane_repo
        .upsert_for_project(
            "proj-1",
            AgentLane::IdeationSubagent,
            &claude_lane_settings("haiku", None, None, None),
        )
        .await
        .expect("subagent lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "ralphx-ideation",
        Some("proj-1"),
        ChatContextType::Ideation,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, Some(AgentHarnessKind::Claude));
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Claude);
    assert_eq!(resolved.configured_model.as_deref(), Some("opus"));
    assert_eq!(
        resolved.configured_logical_effort,
        Some(LogicalEffort::XHigh)
    );
    assert_eq!(
        resolved.configured_approval_policy.as_deref(),
        Some("on_request")
    );
    assert_eq!(
        resolved.configured_sandbox_mode.as_deref(),
        Some("workspace_write")
    );
    assert_eq!(resolved.model, "opus");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(resolved.claude_effort.as_deref(), Some("xhigh"));
    assert_eq!(resolved.approval_policy.as_deref(), Some("on_request"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("workspace_write"));
    assert_eq!(
        resolved.configured_subagent_model_cap.as_deref(),
        Some("haiku")
    );
    assert_eq!(resolved.subagent_model_cap.as_deref(), Some("haiku"));
}

#[tokio::test]
async fn codex_lane_selection_uses_codex_lane_settings() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::IdeationPrimary,
            &codex_lane_settings(
                "gpt-5.4",
                Some(LogicalEffort::XHigh),
                Some("on_request"),
                Some("workspace_write"),
            ),
        )
        .await
        .expect("codex lane upsert should succeed");
    lane_repo
        .upsert_global(
            AgentLane::IdeationSubagent,
            &codex_lane_settings(
                "gpt-5.4-mini",
                Some(LogicalEffort::Medium),
                Some("never"),
                Some("danger_full_access"),
            ),
        )
        .await
        .expect("codex subagent lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "ralphx-ideation",
        None,
        ChatContextType::Ideation,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.configured_model.as_deref(), Some("gpt-5.4"));
    assert_eq!(
        resolved.configured_logical_effort,
        Some(LogicalEffort::XHigh)
    );
    assert_eq!(
        resolved.configured_approval_policy.as_deref(),
        Some("on_request")
    );
    assert_eq!(
        resolved.configured_sandbox_mode.as_deref(),
        Some("workspace_write")
    );
    assert_eq!(resolved.model, "gpt-5.4");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(resolved.claude_effort.as_deref(), Some("xhigh"));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(
        resolved.configured_subagent_model_cap.as_deref(),
        Some("gpt-5.4-mini")
    );
    assert_eq!(resolved.subagent_model_cap.as_deref(), Some("gpt-5.4-mini"));
}

#[tokio::test]
async fn codex_primary_lane_without_model_or_effort_uses_registry_defaults() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::IdeationPrimary,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Codex,
                model: None,
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .expect("codex lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "ralphx-ideation",
        None,
        ChatContextType::Ideation,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.model, "gpt-5.5");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(resolved.subagent_model_cap.as_deref(), Some("gpt-5.4-mini"));
}

#[tokio::test]
async fn execution_worker_lane_can_resolve_codex_settings() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::ExecutionWorker,
            &codex_lane_settings(
                "gpt-5.4",
                Some(LogicalEffort::High),
                Some("on-request"),
                Some("workspace-write"),
            ),
        )
        .await
        .expect("execution worker lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "worker",
        None,
        ChatContextType::TaskExecution,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.model, "gpt-5.4");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::High));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(resolved.subagent_model_cap, None);
}

#[tokio::test]
async fn execution_worker_harness_override_ignores_mismatched_lane_harness_settings() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::ExecutionWorker,
            &claude_lane_settings(
                "opus",
                Some(LogicalEffort::High),
                Some("on_request"),
                Some("workspace_write"),
            ),
        )
        .await
        .expect("execution worker lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "worker",
        None,
        ChatContextType::TaskExecution,
        None,
        Some(AgentHarnessKind::Codex),
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, None);
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.configured_model, None);
    assert_eq!(resolved.configured_logical_effort, None);
    assert_eq!(resolved.configured_approval_policy, None);
    assert_eq!(resolved.configured_sandbox_mode, None);
    assert_eq!(resolved.model, "gpt-5.5");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn execution_worker_codex_without_model_uses_generic_codex_defaults() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::ExecutionWorker,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Codex,
                model: None,
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .expect("execution worker codex lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "worker",
        None,
        ChatContextType::TaskExecution,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.model, "gpt-5.5");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn project_chat_codex_override_without_model_gets_codex_defaults() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    let resolved = resolve_agent_spawn_settings(
        "ralphx-chat-project",
        Some("proj-1"),
        ChatContextType::Project,
        None,
        Some(AgentHarnessKind::Codex),
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, None);
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.configured_approval_policy, None);
    assert_eq!(resolved.configured_sandbox_mode, None);
    assert_eq!(resolved.model, "gpt-5.5");
    assert_eq!(resolved.logical_effort, None);
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[tokio::test]
async fn project_chat_claude_override_without_model_gets_claude_default() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    let resolved = resolve_agent_spawn_settings(
        "ralphx-chat-project",
        Some("proj-1"),
        ChatContextType::Project,
        None,
        Some(AgentHarnessKind::Claude),
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, None);
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Claude);
    assert_eq!(resolved.model, "sonnet");
    assert_eq!(resolved.logical_effort, None);
    assert_eq!(resolved.approval_policy, None);
    assert_eq!(resolved.sandbox_mode, None);
}

#[tokio::test]
async fn reexecuting_task_execution_uses_reexecutor_lane_settings() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::ExecutionReexecutor,
            &codex_lane_settings(
                "gpt-5.4-mini",
                Some(LogicalEffort::Medium),
                Some("never"),
                Some("read-only"),
            ),
        )
        .await
        .expect("execution reexecutor lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "worker",
        None,
        ChatContextType::TaskExecution,
        Some("re_executing"),
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.configured_harness, Some(AgentHarnessKind::Codex));
    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.model, "gpt-5.4-mini");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::Medium));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[test]
fn chat_launch_inventory_maps_every_context_without_an_unnamed_fallback() {
    let cases = [
        (
            "ralphx-general-explorer",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Chat),
            false,
            RoutingRole::WorkspaceChat,
        ),
        (
            "ralphx-general-worker",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Edit),
            false,
            RoutingRole::WorkspaceEdit,
        ),
        (
            "ralphx-ideation",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Plan),
            false,
            RoutingRole::WorkspacePlan,
        ),
        (
            "ralphx-task-manager",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Tasks),
            false,
            RoutingRole::UtilityLightweight,
        ),
        (
            "ralphx-chat-project",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Autopilot),
            false,
            RoutingRole::WorkspaceIdeation,
        ),
        (
            "ralphx-chat-project",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Ideation),
            false,
            RoutingRole::WorkspaceIdeation,
        ),
        (
            "ralphx-pr-reviewer",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::ReviewPr),
            false,
            RoutingRole::WorkspaceReviewPr,
        ),
        (
            "ralphx-automation-setup",
            ChatContextType::Project,
            None,
            Some(AgentConversationWorkspaceMode::Automation),
            false,
            RoutingRole::WorkspaceAutomation,
        ),
        (
            "ralphx-ideation",
            ChatContextType::Ideation,
            None,
            None,
            false,
            RoutingRole::IdeationPrimary,
        ),
        (
            "ralphx-ideation",
            ChatContextType::Ideation,
            None,
            None,
            true,
            RoutingRole::IdeationVerifier,
        ),
        (
            "ralphx-chat-project",
            ChatContextType::Delegation,
            None,
            None,
            false,
            RoutingRole::DelegatedSubagent,
        ),
        (
            "ralphx-chat-task",
            ChatContextType::Task,
            None,
            None,
            false,
            RoutingRole::UtilityLightweight,
        ),
        (
            "ralphx-execution-worker",
            ChatContextType::TaskExecution,
            None,
            None,
            false,
            RoutingRole::ExecutionWorker,
        ),
        (
            "ralphx-execution-worker",
            ChatContextType::TaskExecution,
            Some("re_executing"),
            None,
            false,
            RoutingRole::ExecutionReexecutor,
        ),
        (
            "ralphx-execution-reviewer",
            ChatContextType::Review,
            None,
            None,
            false,
            RoutingRole::ExecutionReviewer,
        ),
        (
            "ralphx-execution-merger",
            ChatContextType::Merge,
            None,
            None,
            false,
            RoutingRole::ExecutionMerger,
        ),
        (
            "ralphx-execution-branch-updater",
            ChatContextType::BranchUpdate,
            None,
            None,
            false,
            RoutingRole::WorkspaceRepair,
        ),
    ];

    for (agent, context, status, mode, verification, expected) in cases {
        assert_eq!(
            routing_role_for_chat_launch(agent, context, status, mode, verification),
            expected,
            "unexpected role for {agent} in {context}"
        );
    }
}

#[test]
fn canonical_specialist_launches_override_generic_project_context() {
    let cases = [
        (
            "ralphx-automation-plan-judge",
            RoutingRole::AutomationPlanJudge,
        ),
        (
            "ralphx-automation-judge",
            RoutingRole::AutomationResultJudge,
        ),
        ("ralphx-workspace-reviewer", RoutingRole::WorkspaceReviewer),
        (
            "ralphx-agent-workspace-repair",
            RoutingRole::WorkspaceRepair,
        ),
        (
            "ralphx-agent-workspace-pr-fixer",
            RoutingRole::WorkspacePrFixer,
        ),
        (
            "ralphx-utility-pr-describer",
            RoutingRole::UtilityPrDescriber,
        ),
        (
            "ralphx-project-analyzer",
            RoutingRole::UtilityProjectAnalyzer,
        ),
        ("ralphx-memory-capture", RoutingRole::MemoryCapture),
        ("ralphx-memory-maintainer", RoutingRole::MemoryMaintainer),
    ];

    for (agent, expected) in cases {
        assert_eq!(
            routing_role_for_chat_launch(agent, ChatContextType::Project, None, None, false,),
            expected,
            "unexpected specialist role for {agent}"
        );
    }
}

#[test]
fn delegated_launches_preserve_ideation_parent_roles() {
    assert_eq!(
        routing_role_for_delegated_launch(
            "ralphx-ideation-specialist-backend",
            ChatContextType::Ideation,
            false,
        ),
        RoutingRole::IdeationSubagent
    );
    assert_eq!(
        routing_role_for_delegated_launch(
            "ralphx-general-explorer",
            ChatContextType::Ideation,
            true,
        ),
        RoutingRole::IdeationVerifierSubagent
    );
    assert_eq!(
        routing_role_for_delegated_launch(
            "ralphx-research-deep-researcher",
            ChatContextType::Project,
            false,
        ),
        RoutingRole::DelegatedSubagent
    );
}

#[test]
fn workspace_repair_agent_uses_merge_repair_role_inside_merge_context() {
    assert_eq!(
        routing_role_for_chat_launch(
            "ralphx-agent-workspace-repair",
            ChatContextType::Merge,
            None,
            None,
            false,
        ),
        RoutingRole::WorkspaceMergeRepair
    );
    assert_eq!(
        routing_role_for_chat_launch(
            "ralphx-agent-workspace-repair",
            ChatContextType::BranchUpdate,
            None,
            None,
            false,
        ),
        RoutingRole::WorkspaceRepair
    );
}

#[tokio::test]
async fn manual_role_default_preserves_exact_standard_speed_in_spawn_settings() {
    let config_root = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
    let manual_repo = Arc::new(MemoryManualRoleDefaultRepository::new());
    manual_repo
        .upsert_for_project(
            "project-manual",
            RoutingRole::WorkspaceEdit,
            &ManualRoleDefault {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-5.6-test".to_string()),
                effort: Some(LogicalEffort::High),
                service_tier: ManualServiceTier::Standard,
                coordination_mode: None,
                persona_id: None,
                approval_policy: Some("on-request".to_string()),
                sandbox_mode: Some("workspace-write".to_string()),
            },
        )
        .await
        .unwrap();
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(AgentHarnessKind::Claude),
    );
    let service = ManualRoleDefaultService::new(
        manual_repo,
        lane_repo,
        provider_repo,
        Arc::new(MemoryPersonaRepository::new()),
        Arc::new(crate::application::agent_capability_gate::AgentCapabilityGate::default()),
        true,
        config_root.path().join("router.yaml"),
    );

    let resolved = resolve_manual_role_spawn_settings(
        "ralphx-general-worker",
        Some("project-manual"),
        None,
        RoutingRole::WorkspaceEdit,
        None,
        None,
        &service,
    )
    .await
    .unwrap();

    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.configured_model.as_deref(), Some("gpt-5.6-test"));
    assert_eq!(resolved.model, "gpt-5.6-test");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::High));
    assert_eq!(resolved.service_tier.as_deref(), Some("standard"));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
}

#[test]
fn state_machine_spawner_inventory_maps_specialized_execution_roles() {
    let cases = [
        ("worker", None, RoutingRole::ExecutionWorker),
        (
            "worker",
            Some("re_executing"),
            RoutingRole::ExecutionReexecutor,
        ),
        ("coder", None, RoutingRole::DelegatedSubagent),
        ("qa-prep", None, RoutingRole::ExecutionQaPrep),
        ("qa-refiner", None, RoutingRole::ExecutionQaRefiner),
        ("qa-tester", None, RoutingRole::ExecutionQaTester),
        ("reviewer", None, RoutingRole::ExecutionReviewer),
        ("merger", None, RoutingRole::ExecutionMerger),
        ("branch-updater", None, RoutingRole::WorkspaceRepair),
    ];

    for (agent_type, status, expected) in cases {
        assert_eq!(
            routing_role_for_spawner_agent(agent_type, status),
            Some(expected),
            "unexpected spawner role for {agent_type}"
        );
    }
    assert_eq!(routing_role_for_spawner_agent("custom-agent", None), None);
}
