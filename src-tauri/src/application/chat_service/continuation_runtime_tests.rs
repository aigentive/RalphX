use std::sync::Arc;

use super::continuation_runtime::{
    compare_live_run_model_identity, compare_model_identity_fields, resolve_for_conversation,
    ContinuationRuntime, ModelIdentityComparison, RuntimeOverridePresence,
};
use super::SendMessageOptions;
use crate::application::agent_lane_resolution::ResolvedAgentSpawnSettings;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort, ProviderSessionRef};
use crate::domain::entities::{AgentRun, AgentRunStatus, ChatConversation, RuntimeSource};
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
        runtime_source: RuntimeSource::HarnessFallback,
        extra_allowed_mcp_tools: Vec::new(),
    }
}

#[test]
fn runtime_source_for_send_uses_typed_launch_provenance_before_materialized_fields() {
    let resolved = base_codex_settings();
    assert_eq!(
        super::runtime_source_for_send(
            &SendMessageOptions {
                model_override: Some("gpt-5.6-sol".to_string()),
                ..Default::default()
            },
            &resolved,
        ),
        RuntimeSource::ComposerSelection
    );
    assert_eq!(
        super::runtime_source_for_send(
            &SendMessageOptions {
                manual_role_runtime_override: Some(
                    crate::domain::agents::ManualRoleRuntimeOverride {
                        harness: AgentHarnessKind::Codex,
                        model: Some("gpt-5.6-sol".to_string()),
                        effort: Some(LogicalEffort::High),
                        service_tier: crate::domain::agents::ManualServiceTier::Standard,
                        coordination_mode: None,
                        persona_id: None,
                    },
                ),
                runtime_source_override: Some(RuntimeSource::ComposerSelection),
                ..Default::default()
            },
            &resolved,
        ),
        RuntimeSource::ComposerSelection
    );
    assert_eq!(
        super::runtime_source_for_send(
            &SendMessageOptions {
                model_override: Some("gpt-5.6-sol".to_string()),
                runtime_source_override: Some(RuntimeSource::RoleDefault),
                ..Default::default()
            },
            &resolved,
        ),
        RuntimeSource::RoleDefault
    );
    assert_eq!(
        super::runtime_source_for_send(&SendMessageOptions::default(), &resolved),
        RuntimeSource::HarnessFallback
    );
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

#[test]
fn model_identity_matches_logical_alias_or_exact_effective_id() {
    let runtime = ContinuationRuntime {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "session-1".to_string(),
        logical_model: Some("sonnet".to_string()),
        effective_model_id: Some("claude-sonnet-4-6".to_string()),
        logical_effort: None,
        service_tier: None,
        approval_policy: None,
        sandbox_mode: None,
    };

    assert_eq!(
        runtime.compare_model_identity(" SONNET "),
        ModelIdentityComparison::Same
    );
    assert_eq!(
        runtime.compare_model_identity("claude-sonnet-4-6"),
        ModelIdentityComparison::Same
    );
    assert_eq!(
        runtime.compare_model_identity("claude-sonnet-4-5"),
        ModelIdentityComparison::Changed,
        "different effective versions must not collapse to one Claude family"
    );
    assert_eq!(
        runtime.compare_model_identity("opus"),
        ModelIdentityComparison::Changed
    );
}

#[test]
fn model_identity_is_unknown_without_persisted_model_attribution() {
    let runtime = ContinuationRuntime {
        harness: AgentHarnessKind::Claude,
        provider_session_id: "session-1".to_string(),
        logical_model: None,
        effective_model_id: None,
        logical_effort: None,
        service_tier: None,
        approval_policy: None,
        sandbox_mode: None,
    };

    assert_eq!(
        runtime.compare_model_identity("sonnet"),
        ModelIdentityComparison::Unknown
    );
}

#[test]
fn model_identity_fields_preserve_alias_tolerance_and_unknown_semantics() {
    assert_eq!(
        compare_model_identity_fields(Some(" SONNET "), None, "sonnet"),
        ModelIdentityComparison::Same
    );
    assert_eq!(
        compare_model_identity_fields(
            Some("sonnet"),
            Some("claude-sonnet-4-6"),
            "claude-sonnet-4-6",
        ),
        ModelIdentityComparison::Same
    );
    assert_eq!(
        compare_model_identity_fields(Some("sonnet"), Some("claude-sonnet-4-6"), "opus",),
        ModelIdentityComparison::Changed
    );
    assert_eq!(
        compare_model_identity_fields(None, None, "sonnet"),
        ModelIdentityComparison::Unknown
    );
    assert_eq!(
        compare_model_identity_fields(Some("  "), Some("\t"), "sonnet"),
        ModelIdentityComparison::Unknown
    );
}

#[tokio::test]
async fn live_model_identity_requires_a_running_run() {
    let repository: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = crate::domain::entities::ChatConversationId::new();
    let mut running = AgentRun::new(conversation_id);
    running.logical_model = Some("sonnet".to_string());
    running.effective_model_id = Some("claude-sonnet-4-6".to_string());
    let running_id = running.id;
    repository.create(running).await.unwrap();

    assert_eq!(
        compare_live_run_model_identity(&repository, &running_id, "sonnet")
            .await
            .unwrap(),
        ModelIdentityComparison::Same
    );

    let mut completed = AgentRun::new(conversation_id);
    completed.complete();
    completed.logical_model = Some("sonnet".to_string());
    let completed_id = completed.id;
    repository.create(completed).await.unwrap();

    assert_eq!(
        compare_live_run_model_identity(&repository, &completed_id, "opus")
            .await
            .unwrap(),
        ModelIdentityComparison::Unknown
    );
    assert_eq!(
        compare_live_run_model_identity(
            &repository,
            &crate::domain::entities::AgentRunId::new(),
            "sonnet",
        )
        .await
        .unwrap(),
        ModelIdentityComparison::Unknown
    );
}

fn complete_runtime_override() -> crate::domain::agents::ManualRoleRuntimeOverride {
    crate::domain::agents::ManualRoleRuntimeOverride {
        harness: AgentHarnessKind::Claude,
        model: Some("opus".to_string()),
        effort: Some(LogicalEffort::High),
        service_tier: crate::domain::agents::ManualServiceTier::Standard,
        coordination_mode: None,
        persona_id: None,
    }
}

#[test]
fn manual_runtime_override_wins_over_conversation_derived_harness() {
    let options = SendMessageOptions {
        manual_role_runtime_override: Some(complete_runtime_override()),
        ..Default::default()
    };

    // A harness derived from a (possibly stale) conversation provider session must not reach the
    // legacy-mixing guard — that combination is what rejected the first "Implement Directly" click.
    assert_eq!(
        super::manual_mixing_harness_override(&options, Some(AgentHarnessKind::Claude)),
        None
    );
}

#[test]
fn manual_runtime_override_still_conflicts_with_explicit_client_harness() {
    let options = SendMessageOptions {
        manual_role_runtime_override: Some(complete_runtime_override()),
        harness_override: Some(AgentHarnessKind::Codex),
        ..Default::default()
    };

    assert_eq!(
        super::manual_mixing_harness_override(&options, Some(AgentHarnessKind::Claude)),
        Some(AgentHarnessKind::Codex)
    );
}

#[test]
fn derived_harness_survives_without_a_manual_runtime_override() {
    let options = SendMessageOptions::default();

    assert_eq!(
        super::manual_mixing_harness_override(&options, Some(AgentHarnessKind::Claude)),
        Some(AgentHarnessKind::Claude)
    );
}

#[test]
fn continuation_presence_marks_manual_runtime_override_fields_as_chosen() {
    let presence = super::continuation_override_presence(&SendMessageOptions {
        manual_role_runtime_override: Some(complete_runtime_override()),
        ..Default::default()
    });

    assert_eq!(
        presence,
        RuntimeOverridePresence {
            model: true,
            logical_effort: true,
            service_tier: true,
            approval_policy: false,
            sandbox_mode: false,
        }
    );
}

#[test]
fn continuation_presence_leaves_unset_manual_fields_to_the_prior_runtime() {
    let presence = super::continuation_override_presence(&SendMessageOptions {
        manual_role_runtime_override: Some(crate::domain::agents::ManualRoleRuntimeOverride {
            model: None,
            effort: None,
            ..complete_runtime_override()
        }),
        ..Default::default()
    });

    assert_eq!(
        presence,
        RuntimeOverridePresence {
            model: false,
            logical_effort: false,
            // A complete runtime override always carries a service tier.
            service_tier: true,
            approval_policy: false,
            sandbox_mode: false,
        }
    );
}

#[test]
fn manual_runtime_override_model_survives_continuation_defaults() {
    let mut resolved = base_codex_settings();
    resolved.model = "gpt-5.6-sol".to_string();
    resolved.configured_model = Some("gpt-5.6-sol".to_string());
    let continuation = ContinuationRuntime {
        harness: AgentHarnessKind::Codex,
        provider_session_id: "prior-session".to_string(),
        logical_model: Some("gpt-5.5".to_string()),
        effective_model_id: Some("gpt-5.5".to_string()),
        logical_effort: Some(LogicalEffort::Low),
        service_tier: Some("flex".to_string()),
        approval_policy: Some("never".to_string()),
        sandbox_mode: Some("danger-full-access".to_string()),
    };

    continuation.apply_defaults(
        &mut resolved,
        super::continuation_override_presence(&SendMessageOptions {
            manual_role_runtime_override: Some(crate::domain::agents::ManualRoleRuntimeOverride {
                harness: AgentHarnessKind::Codex,
                model: Some("gpt-5.6-sol".to_string()),
                effort: Some(LogicalEffort::High),
                service_tier: crate::domain::agents::ManualServiceTier::Standard,
                coordination_mode: None,
                persona_id: None,
            }),
            ..Default::default()
        }),
    );

    assert_eq!(resolved.model, "gpt-5.6-sol");
    assert_eq!(resolved.configured_model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(resolved.logical_effort, Some(LogicalEffort::XHigh));
    assert_eq!(resolved.service_tier, None);
    // Approval/sandbox stay continuation-owned.
    assert_eq!(resolved.approval_policy.as_deref(), Some("never"));
}
