use std::path::PathBuf;
use std::sync::Arc;

use crate::application::agent_workspace_publish_recovery::{
    recover_stale_agent_workspace_publish_repairs,
    recover_stale_agent_workspace_publish_repairs_for_state,
    recover_stale_agent_workspace_publish_repairs_on_startup,
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state,
    recover_stale_publish_repair_for_workspace,
    recover_stale_publish_repair_for_workspace_and_reload,
    recover_stale_publish_repair_for_workspace_and_reload_with_review_target,
    recover_stale_publish_repair_for_workspace_in_state, recover_stale_transient_publish_statuses,
    STALE_NEEDS_AGENT_CLASSIFICATION, STALE_REPAIR_BLOCKED_SUMMARY, STALE_REPAIR_RECOVERED_STEP,
    STALE_TRANSIENT_CLASSIFICATION, STALE_TRANSIENT_RECOVERED_STEP,
};
use crate::application::agent_workspace_review::{
    AgentWorkspaceReviewPacket, AgentWorkspaceReviewTarget,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus, AgentWorkspaceReviewOutcome,
    AgentWorkspaceReviewTargetScope, ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind,
    ProjectId,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, AgentRunRepository};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryTaskOutcomeRepository,
};

fn conversation_id(suffix: u8) -> ChatConversationId {
    ChatConversationId::from_string(format!("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbb{suffix:02}"))
}

fn project_id() -> ProjectId {
    ProjectId::from_string("project-publish-recovery".to_string())
}

fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/publish-recovery".to_string(),
        "/tmp/ralphx-test-publish-recovery".to_string(),
    );
    workspace.publication_pr_number = Some(684);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/684".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_autofix_enabled = true;
    workspace.pr_auto_merge_current = Some(true);
    workspace.pr_supervision_status = Some("fixing".to_string());
    workspace
}

fn review_target() -> AgentWorkspaceReviewTarget {
    AgentWorkspaceReviewTarget {
        scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        base_ref: "main".to_string(),
        base_sha: Some("base-sha".to_string()),
        head_ref: "ralphx/test/publish-recovery".to_string(),
        head_sha: Some("head-current".to_string()),
        diff_fingerprint: "diff-current".to_string(),
        working_directory: PathBuf::from("/tmp/ralphx-test-publish-recovery"),
        source_pull_request_number: None,
        review_packet: AgentWorkspaceReviewPacket::default(),
    }
}

fn reviewing_monitor(
    conversation_id: ChatConversationId,
    target: &AgentWorkspaceReviewTarget,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

fn stale_passed_monitor(
    conversation_id: ChatConversationId,
    target: &AgentWorkspaceReviewTarget,
) -> AgentWorkspaceReviewMonitor {
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id());
    monitor.status = AgentWorkspaceReviewMonitorStatus::Ready;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Passed;
    monitor.review_outcome = AgentWorkspaceReviewOutcome::Passed;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-stale"));
    monitor.reviewed_target_scope = Some(target.scope);
    monitor.reviewed_head_sha = Some("old-head".to_string());
    monitor.reviewed_diff_fingerprint = Some("old-diff".to_string());
    monitor.current_target_scope = Some(target.scope);
    monitor.current_diff_fingerprint = Some(target.diff_fingerprint.clone());
    monitor.workspace_head_sha = target.head_sha.clone();
    monitor
}

async fn seed_terminal_run(
    agent_run_repo: &dyn AgentRunRepository,
    conversation_id: ChatConversationId,
) {
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed run");
    agent_run_repo
        .fail(&run.id, "agent repair exited")
        .await
        .expect("mark run failed");
}

#[tokio::test]
async fn startup_recovery_wrappers_finish_on_empty_repositories() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());

    recover_stale_agent_workspace_publish_repairs_on_startup(
        workspace_repo as Arc<dyn AgentConversationWorkspaceRepository>,
        agent_run_repo as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTaskOutcomeRepository::new()),
    )
    .await;
    recover_stale_agent_workspace_publish_repairs_on_startup_for_state(&AppState::new_test()).await;
}

#[tokio::test]
async fn state_recovery_recovers_terminal_needs_agent_workspace_and_reloads_it() {
    let state = AppState::new_test();
    let conversation_id = conversation_id(1);
    let workspace = needs_agent_workspace(conversation_id);
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    seed_terminal_run(state.agent_run_repo.as_ref(), conversation_id).await;

    let recovered = recover_stale_agent_workspace_publish_repairs_for_state(&state)
        .await
        .expect("recover stale publish repair");

    assert_eq!(recovered, 1);
    let updated = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("load workspace")
        .expect("workspace exists");
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    let events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events.iter().any(|event| {
        event.step == "stale_repair_recovered"
            && event.classification.as_deref() == Some("stale_needs_agent")
    }));
}

#[tokio::test]
async fn recovery_with_review_target_preserves_current_reviewing_handoff() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(2);
    let workspace = needs_agent_workspace(conversation_id);
    let target = review_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    workspace_repo
        .upsert_workspace_review_monitor(reviewing_monitor(conversation_id, &target))
        .await
        .expect("seed review monitor");
    seed_terminal_run(agent_run_repo.as_ref(), conversation_id).await;

    let (refreshed, recovered) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            Arc::new(MemoryTaskOutcomeRepository::new()),
            workspace,
            Some(&target),
        )
        .await
        .expect("check stale publish repair");

    assert!(!recovered);
    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(
        !events
            .iter()
            .any(|event| event.step == "stale_repair_recovered"),
        "current Workspace Review handoff must not be downgraded as stale"
    );
}

#[tokio::test]
async fn stale_review_handoff_without_matching_target_is_recovered_and_reloaded() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(3);
    let workspace = needs_agent_workspace(conversation_id);
    let target = review_target();
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "pr_autofix_workspace_review",
            "reviewing",
            "PR fix completed; Workspace Review started before publishing resumes.",
            Some("workspace_review_started".to_string()),
        ))
        .await
        .expect("seed pending review event");
    workspace_repo
        .upsert_workspace_review_monitor(stale_passed_monitor(conversation_id, &target))
        .await
        .expect("seed stale passed review monitor");
    seed_terminal_run(agent_run_repo.as_ref(), conversation_id).await;

    let refreshed = recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTaskOutcomeRepository::new()),
        workspace,
    )
    .await
    .expect("recover stale publish repair");

    assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("list events");
    assert!(events
        .iter()
        .any(|event| event.step == "stale_repair_recovered"));
}

#[tokio::test]
async fn batch_recovery_counts_only_recovered_workspaces() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let recoverable_id = conversation_id(4);
    let active_id = conversation_id(5);
    let recoverable = needs_agent_workspace(recoverable_id);
    let active = needs_agent_workspace(active_id);
    workspace_repo
        .create_or_update(recoverable)
        .await
        .expect("seed recoverable workspace");
    workspace_repo
        .create_or_update(active)
        .await
        .expect("seed active workspace");
    seed_terminal_run(agent_run_repo.as_ref(), recoverable_id).await;
    agent_run_repo
        .create(AgentRun::new(active_id))
        .await
        .expect("seed active run");

    let recovered = recover_stale_agent_workspace_publish_repairs(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTaskOutcomeRepository::new()),
    )
    .await
    .expect("recover batch");

    assert_eq!(recovered, 1);
    assert_eq!(
        workspace_repo
            .get_by_conversation_id(&active_id)
            .await
            .expect("load active workspace")
            .expect("active workspace exists")
            .publication_push_status
            .as_deref(),
        Some("needs_agent")
    );
}

#[tokio::test]
async fn recovery_heals_only_an_active_current_repair_to_fixing() {
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let conversation_id = conversation_id(6);
    let mut workspace = needs_agent_workspace(conversation_id);
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("seed workspace");
    agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed active run");
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id,
            "repair_sent",
            "succeeded",
            "Sent failure to workspace repair agent",
            Some("agent_fixable".to_string()),
        ))
        .await
        .expect("seed repair evidence");

    let refreshed = recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
        Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        workspace,
    )
    .await
    .expect("reconcile active repair");

    assert_eq!(
        refreshed.publication_push_status.as_deref(),
        Some("needs_agent")
    );
    assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("fixing"));
}

mod extracted_inline_tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, ChatConversationId,
        IdeationAnalysisBaseRefKind, ProjectId,
    };
    use crate::infrastructure::memory::{
        MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    };

    fn needs_agent_workspace(conversation_id: ChatConversationId) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-1".to_string()),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_status = Some("failed".to_string());
        workspace.publication_push_status = Some("needs_agent".to_string());
        workspace
    }

    async fn create_failed_run(
        agent_run_repo: &MemoryAgentRunRepository,
        conversation_id: ChatConversationId,
    ) {
        let run = agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed run");
        agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("mark run failed");
    }

    #[tokio::test]
    async fn recovers_needs_agent_workspace_when_no_agent_run_is_active() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        let recovered = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await
        .expect("recover stale repair");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));

        let events = workspace_repo
            .list_publication_events(&workspace.conversation_id)
            .await
            .expect("list events");
        assert!(events.iter().any(|event| {
            event.step == STALE_REPAIR_RECOVERED_STEP
                && event.status == "succeeded"
                && event.classification.as_deref() == Some(STALE_NEEDS_AGENT_CLASSIFICATION)
        }));
    }

    #[tokio::test]
    async fn reloads_recovered_workspace_from_app_state() {
        let state = crate::application::AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
        let workspace = needs_agent_workspace(conversation_id);
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let run = state
            .agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed run");
        state
            .agent_run_repo
            .fail(&run.id, "repair agent exited")
            .await
            .expect("mark run failed");

        let refreshed = recover_stale_publish_repair_for_workspace_in_state(&state, workspace)
            .await
            .expect("recover stale repair");

        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn recovers_stale_supervised_autofix_workspace_as_blocked() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace.pr_autofix_enabled = true;
        workspace.pr_auto_merge_current = Some(true);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        let recovered = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await
        .expect("recover stale repair");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&workspace.conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
        assert_eq!(refreshed.pr_supervision_status.as_deref(), Some("blocked"));
        assert_eq!(
            refreshed.pr_supervision_summary.as_deref(),
            Some(STALE_REPAIR_BLOCKED_SUMMARY)
        );
        assert_eq!(refreshed.pr_auto_merge_current, Some(true));
    }

    #[tokio::test]
    async fn startup_helper_recovers_stale_repairs() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("77777777-7777-7777-7777-777777777777");

        recover_stale_agent_workspace_publish_repairs_on_startup(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await;

        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;

        recover_stale_agent_workspace_publish_repairs_on_startup(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
        )
        .await;

        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));
    }

    #[tokio::test]
    async fn keeps_needs_agent_workspace_locked_while_agent_run_is_active() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        agent_run_repo
            .create(AgentRun::new(conversation_id))
            .await
            .expect("seed active run");

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            Arc::clone(&agent_run_repo) as Arc<dyn AgentRunRepository>,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("needs_agent")
        );
        assert!(
            workspace_repo
                .list_publication_events(&conversation_id)
                .await
                .expect("list events")
                .is_empty(),
            "active repairs must not be downgraded"
        );
    }

    #[tokio::test]
    async fn ignores_workspace_that_is_not_waiting_on_agent_repair() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace.publication_push_status = Some("failed".to_string());

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
    }

    #[tokio::test]
    async fn ignores_workspace_without_terminal_repair_run_evidence() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("55555555-5555-5555-5555-555555555555");
        let workspace = needs_agent_workspace(conversation_id);

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
    }

    #[tokio::test]
    async fn recovers_terminal_run_without_completion_timestamp() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("88888888-8888-8888-8888-888888888888");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        let mut run = AgentRun::new(conversation_id);
        run.status = AgentRunStatus::Failed;
        run.completed_at = None;
        agent_run_repo.create(run).await.expect("seed run");

        let recovered =
            recover_stale_publish_repair_for_workspace(workspace_repo, agent_run_repo, workspace)
                .await
                .expect("check repair state");

        assert!(recovered);
    }

    #[tokio::test]
    async fn does_not_recover_a_fresh_claim_from_an_older_terminal_run() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");
        let mut workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;
        workspace.updated_at = chrono::Utc::now() + chrono::Duration::minutes(5);

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(!recovered);
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            current.publication_push_status.as_deref(),
            Some("needs_agent")
        );
    }

    #[tokio::test]
    async fn stale_recovery_snapshot_cannot_overwrite_newer_workspace_state() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
        let conversation_id =
            ChatConversationId::from_string("99999999-9999-9999-9999-999999999999");
        let workspace = needs_agent_workspace(conversation_id);
        workspace_repo
            .create_or_update(workspace.clone())
            .await
            .expect("seed workspace");
        create_failed_run(&agent_run_repo, conversation_id).await;
        workspace_repo
            .update_publication(
                &conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                workspace.publication_pr_status.as_deref(),
                Some("pushed"),
            )
            .await
            .expect("persist newer publication state");
        workspace_repo
            .update_pr_auto_merge_state(
                &conversation_id,
                workspace.pr_auto_merge_current,
                Some("monitoring"),
                Some("Newer state is authoritative"),
            )
            .await
            .expect("persist newer supervision state");

        let recovered = recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            agent_run_repo,
            workspace,
        )
        .await
        .expect("stale recovery should be rejected");

        assert!(!recovered);
        let current = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(current.publication_push_status.as_deref(), Some("pushed"));
        assert_eq!(current.pr_supervision_status.as_deref(), Some("monitoring"));
        assert!(workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .unwrap()
            .iter()
            .all(|event| event.step != STALE_REPAIR_RECOVERED_STEP));
    }

    fn transient_workspace(
        conversation_id: ChatConversationId,
        status: &str,
    ) -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            conversation_id,
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("Project default (main)".to_string()),
            Some("base-1".to_string()),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_push_status = Some(status.to_string());
        workspace
    }

    #[tokio::test]
    async fn recovers_stale_transient_refreshing_workspace() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        let workspace = transient_workspace(conversation_id.clone(), "refreshing");
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        // stale_older_than_secs=0 means any workspace updated at or before now is stale
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            0,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 1);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(refreshed.publication_push_status.as_deref(), Some("failed"));

        let events = workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("list events");
        assert!(events.iter().any(|e| {
            e.step == STALE_TRANSIENT_RECOVERED_STEP
                && e.status == "succeeded"
                && e.classification.as_deref() == Some(STALE_TRANSIENT_CLASSIFICATION)
        }));
    }

    #[tokio::test]
    async fn skips_recent_transient_workspace_within_staleness_window() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
        let conversation_id =
            ChatConversationId::from_string("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        let workspace = transient_workspace(conversation_id.clone(), "checking");
        workspace_repo
            .create_or_update(workspace)
            .await
            .expect("seed workspace");

        // stale_older_than_secs=3600 means only workspaces older than 1 hour are stale;
        // a just-created workspace must not be recovered
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            3600,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 0);
        let refreshed = workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .expect("load workspace")
            .expect("workspace exists");
        assert_eq!(
            refreshed.publication_push_status.as_deref(),
            Some("checking")
        );
    }

    #[tokio::test]
    async fn recovers_all_four_stale_transient_statuses() {
        let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());

        for (id, status) in [
            ("cccccccc-cccc-cccc-cccc-cccccccccc01", "refreshing"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc02", "checking"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc03", "committing"),
            ("cccccccc-cccc-cccc-cccc-cccccccccc04", "describing"),
        ] {
            let conv_id = ChatConversationId::from_string(id.to_string());
            let workspace = transient_workspace(conv_id, status);
            workspace_repo
                .create_or_update(workspace)
                .await
                .expect("seed workspace");
        }

        // stale_older_than_secs=0 catches all freshly-seeded transient workspaces
        let recovered = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo) as Arc<dyn AgentConversationWorkspaceRepository>,
            0,
        )
        .await
        .expect("recover transient statuses");

        assert_eq!(recovered, 4);
    }
}
