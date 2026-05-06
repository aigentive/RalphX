use std::sync::Arc;

use super::agent_lane_resolution::resolve_agent_spawn_settings;
use crate::domain::agents::{AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort};
use crate::domain::entities::ChatContextType;
use crate::domain::repositories::AgentLaneSettingsRepository;
use crate::infrastructure::memory::MemoryAgentLaneSettingsRepository;

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
    assert_eq!(resolved.approval_policy.as_deref(), Some("on_request"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("workspace_write"));
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
async fn codex_verifier_lane_without_model_or_effort_uses_registry_defaults() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::IdeationVerifier,
            &AgentLaneSettings {
                harness: AgentHarnessKind::Codex,
                model: None,
                effort: None,
                approval_policy: None,
                sandbox_mode: None,
            },
        )
        .await
        .expect("verifier codex lane upsert should succeed");

    let resolved = resolve_agent_spawn_settings(
        "ralphx-plan-verifier",
        None,
        ChatContextType::Ideation,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(resolved.effective_harness, AgentHarnessKind::Codex);
    assert_eq!(resolved.model, "gpt-5.4-mini");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::Medium));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
    assert_eq!(resolved.subagent_model_cap.as_deref(), Some("gpt-5.4-mini"));
}

#[tokio::test]
async fn verifier_and_primary_subagent_caps_use_lane_rows_when_claude_is_selected() {
    let lane_repo: Arc<dyn AgentLaneSettingsRepository> =
        Arc::new(MemoryAgentLaneSettingsRepository::new());

    lane_repo
        .upsert_global(
            AgentLane::IdeationVerifier,
            &claude_lane_settings("opus", Some(LogicalEffort::High), None, None),
        )
        .await
        .expect("verifier lane upsert should succeed");
    lane_repo
        .upsert_global(
            AgentLane::IdeationVerifierSubagent,
            &claude_lane_settings("haiku", None, None, None),
        )
        .await
        .expect("verifier subagent lane upsert should succeed");

    let verifier = resolve_agent_spawn_settings(
        "ralphx-plan-verifier",
        None,
        ChatContextType::Ideation,
        None,
        None,
        None,
        Some(&lane_repo),
    )
    .await;

    assert_eq!(verifier.model, "opus");
    assert_eq!(verifier.logical_effort, Some(LogicalEffort::High));
    assert_eq!(verifier.claude_effort.as_deref(), Some("high"));
    assert_eq!(verifier.subagent_model_cap.as_deref(), Some("haiku"));
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
    assert_eq!(resolved.approval_policy.as_deref(), Some("on-request"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("workspace-write"));
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
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("read-only"));
}
