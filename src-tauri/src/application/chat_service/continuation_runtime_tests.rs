use std::sync::Arc;

use super::continuation_runtime::{
    resolve_for_conversation, ContinuationRuntime, RuntimeOverridePresence,
};
use crate::application::agent_lane_resolution::ResolvedAgentSpawnSettings;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use crate::domain::entities::{AgentRun, AgentRunStatus, ChatConversation};
use crate::domain::repositories::AgentRunRepository;
use crate::infrastructure::memory::MemoryAgentRunRepository;

fn base_codex_settings() -> ResolvedAgentSpawnSettings {
    ResolvedAgentSpawnSettings {
        configured_harness: None,
        effective_harness: AgentHarnessKind::Codex,
        configured_model: None,
        configured_logical_effort: None,
        configured_approval_policy: None,
        configured_sandbox_mode: None,
        configured_service_tier: None,
        model: "gpt-5.5".to_string(),
        logical_effort: Some(LogicalEffort::XHigh),
        claude_effort: Some("xhigh".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
        service_tier: None,
        configured_subagent_model_cap: None,
        subagent_model_cap: None,
    }
}

#[tokio::test]
async fn conversation_runtime_uses_matching_completed_session_not_newer_failure() {
    let repository: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let mut conversation = ChatConversation::new_project(crate::domain::entities::ProjectId::new());
    conversation.set_provider_session_ref(ProviderSessionRef {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "thread-1".to_string(),
    });
    let mut successful = AgentRun::new(conversation.id);
    successful.status = AgentRunStatus::Completed;
    successful.started_at = chrono::Utc::now() - chrono::Duration::minutes(2);
    successful.harness = Some(AgentHarnessKind::Codex);
    successful.provider_session_id = Some("thread-1".to_string());
    successful.logical_model = Some("gpt-5.6-sol".to_string());
    successful.effective_model_id = Some("gpt-5.6-sol".to_string());
    successful.logical_effort = Some(LogicalEffort::High);
    successful.service_tier = Some("fast".to_string());
    successful.approval_policy = Some("never".to_string());
    successful.sandbox_mode = Some("danger-full-access".to_string());
    repository.create(successful).await.unwrap();

    let mut failed = AgentRun::new(conversation.id);
    failed.status = AgentRunStatus::Failed;
    failed.started_at = chrono::Utc::now();
    failed.harness = Some(AgentHarnessKind::Codex);
    failed.effective_model_id = Some("gpt-5.5".to_string());
    repository.create(failed).await.unwrap();

    let runtime = resolve_for_conversation(&repository, &conversation)
        .await
        .unwrap()
        .expect("matching successful provider runtime");

    assert_eq!(runtime.effective_model(), Some("gpt-5.6-sol"));
    assert_eq!(runtime.logical_effort, Some(LogicalEffort::High));
    assert_eq!(runtime.service_tier.as_deref(), Some("fast"));
}

#[test]
fn continuation_defaults_apply_without_overwriting_explicit_fields() {
    let runtime = ContinuationRuntime {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "thread-1".to_string(),
        logical_model: Some("gpt-5.6-sol".to_string()),
        effective_model_id: Some("gpt-5.6-sol".to_string()),
        logical_effort: Some(LogicalEffort::High),
        service_tier: Some("fast".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    };
    let mut resolved = base_codex_settings();
    resolved.model = "gpt-5.4-mini".to_string();
    resolved.logical_effort = Some(LogicalEffort::Low);

    runtime.apply_defaults(
        &mut resolved,
        RuntimeOverridePresence {
            model: true,
            logical_effort: true,
            ..Default::default()
        },
    );

    assert_eq!(resolved.model, "gpt-5.4-mini");
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::Low));
    assert_eq!(resolved.service_tier.as_deref(), Some("fast"));
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
    assert_eq!(resolved.sandbox_mode.as_deref(), Some("danger-full-access"));
}
