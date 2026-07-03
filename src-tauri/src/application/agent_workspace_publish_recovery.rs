use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, TaskOutcomeRepository,
};
use crate::domain::services::AgentWorkspaceOutcomeAdapter;
use crate::error::AppResult;

const STALE_REPAIR_RECOVERED_STEP: &str = "stale_repair_recovered";
const STALE_NEEDS_AGENT_CLASSIFICATION: &str = "stale_needs_agent";
const STALE_PR_AUTOFIX_SUMMARY: &str =
    "Recovered stale PR autofix state; no active fixer run is running.";
const STALE_TRANSIENT_RECOVERED_STEP: &str = "stale_transient_recovered";
const STALE_TRANSIENT_CLASSIFICATION: &str = "stale_transient_status";
pub const STALE_TRANSIENT_STATUS_STALE_SECS: u64 = 300;

pub async fn recover_stale_agent_workspace_publish_repairs_on_startup(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
) {
    match recover_stale_agent_workspace_publish_repairs(
        workspace_repo,
        agent_run_repo,
        task_outcome_repo,
    )
    .await
    {
        Ok(count) if count > 0 => {
            tracing::info!(
                count,
                "Recovered stale agent workspace publish repair states on startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Failed to recover stale agent workspace publish repair states on startup"
            );
        }
    }
}

pub async fn recover_stale_agent_workspace_publish_repairs(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
) -> AppResult<u32> {
    let workspaces = workspace_repo.list_active_needs_agent_workspaces().await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        if recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo),
            Arc::clone(&agent_run_repo),
            Arc::clone(&task_outcome_repo),
            workspace,
        )
        .await?
        {
            recovered += 1;
        }
    }

    Ok(recovered)
}

pub async fn recover_stale_publish_repair_for_workspace_in_state(
    state: &crate::application::AppState,
    workspace: AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    recover_stale_publish_repair_for_workspace_and_reload(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.task_outcome_repo),
        workspace,
    )
    .await
}

pub async fn recover_stale_publish_repair_for_workspace_and_reload(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    let conversation_id = workspace.conversation_id;
    let recovered = recover_stale_publish_repair_for_workspace(
        Arc::clone(&workspace_repo),
        agent_run_repo,
        task_outcome_repo,
        workspace.clone(),
    )
    .await?;
    if recovered {
        return Ok(workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await?
            .unwrap_or(workspace));
    }

    Ok(workspace)
}

pub async fn recover_stale_publish_repair_for_workspace(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Ok(false);
    }

    if agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
        .is_some()
    {
        return Ok(false);
    }

    let Some(latest_run) = agent_run_repo
        .get_latest_for_conversation(&workspace.conversation_id)
        .await?
    else {
        return Ok(false);
    };

    if !latest_run.status.is_terminal() {
        return Ok(false);
    }

    workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some("failed"),
        )
        .await?;
    if workspace.pr_autofix_enabled {
        workspace_repo
            .update_pr_auto_merge_state(
                &workspace.conversation_id,
                workspace.pr_auto_merge_current,
                Some("blocked"),
                Some(STALE_PR_AUTOFIX_SUMMARY),
            )
            .await?;
    }
    let summary =
        "Recovered stale publish repair state; no active workspace agent repair is running.";
    let event = AgentConversationWorkspacePublicationEvent::new(
        workspace.conversation_id.clone(),
        STALE_REPAIR_RECOVERED_STEP,
        "succeeded",
        summary,
        Some(STALE_NEEDS_AGENT_CLASSIFICATION.to_string()),
    );
    workspace_repo
        .append_publication_event(event.clone())
        .await?;
    let adapter = AgentWorkspaceOutcomeAdapter::new(task_outcome_repo);
    if let Err(error) = adapter
        .record_stale_publish_repair(&workspace, Some(&event), summary)
        .await
    {
        tracing::warn!(
            conversation_id = workspace.conversation_id.as_str(),
            error = %error,
            "Failed to record stale direct agent workspace publish repair outcome"
        );
    }

    tracing::info!(
        conversation_id = latest_run.conversation_id.as_str(),
        agent_run_id = %latest_run.id,
        agent_run_status = %latest_run.status,
        "Recovered stale agent workspace publish repair state"
    );

    Ok(true)
}

pub async fn recover_stale_transient_publish_statuses(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    stale_older_than_secs: u64,
) -> AppResult<u32> {
    let workspaces = workspace_repo
        .list_active_transient_publish_status_workspaces(stale_older_than_secs)
        .await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        let stuck_status = workspace
            .publication_push_status
            .clone()
            .unwrap_or_default();
        workspace_repo
            .update_publication(
                &workspace.conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                workspace.publication_pr_status.as_deref(),
                Some("failed"),
            )
            .await?;
        workspace_repo
            .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                workspace.conversation_id.clone(),
                STALE_TRANSIENT_RECOVERED_STEP,
                "succeeded",
                format!(
                    "Recovered stale transient publish status '{stuck_status}'; no agent is actively progressing it."
                ),
                Some(STALE_TRANSIENT_CLASSIFICATION.to_string()),
            ))
            .await?;
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            stuck_status = %stuck_status,
            "Recovered stale transient publish status workspace"
        );
        recovered += 1;
    }

    Ok(recovered)
}

pub async fn run_periodic_workspace_publish_recovery(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;

        if let Err(err) = recover_stale_agent_workspace_publish_repairs(
            Arc::clone(&workspace_repo),
            Arc::clone(&agent_run_repo),
            Arc::clone(&task_outcome_repo),
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to recover stale needs_agent workspace repairs"
            );
        }

        if let Err(err) = recover_stale_transient_publish_statuses(
            Arc::clone(&workspace_repo),
            STALE_TRANSIENT_STATUS_STALE_SECS,
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to recover stale transient publish statuses"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, AgentRun, AgentRunStatus, ChatConversationId,
        IdeationAnalysisBaseRefKind, ProjectId,
    };
    use crate::infrastructure::memory::{
        MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
        MemoryTaskOutcomeRepository,
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

    fn outcome_repo() -> Arc<MemoryTaskOutcomeRepository> {
        Arc::new(MemoryTaskOutcomeRepository::new())
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
            outcome_repo(),
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
            outcome_repo(),
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
            Some(STALE_PR_AUTOFIX_SUMMARY)
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
            outcome_repo(),
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
            outcome_repo(),
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
            outcome_repo(),
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
            workspace_repo,
            agent_run_repo,
            outcome_repo(),
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
            workspace_repo,
            agent_run_repo,
            outcome_repo(),
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

        let recovered = recover_stale_publish_repair_for_workspace(
            workspace_repo,
            agent_run_repo,
            outcome_repo(),
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(recovered);
    }

    #[tokio::test]
    async fn recovers_terminal_run_that_finished_before_workspace_update() {
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
            workspace_repo,
            agent_run_repo,
            outcome_repo(),
            workspace,
        )
        .await
        .expect("check repair state");

        assert!(recovered);
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
