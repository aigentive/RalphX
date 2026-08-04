mod active_delegations;
pub(crate) mod branch_status;
mod linked_plan;
mod task_ledger;
mod task_runtime;
mod team;
mod workspace;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::domain::entities::{
    AgentConversationWorkspace, AgentTaskScope, ChatContextType, ChatConversationId,
};
use crate::domain::repositories::{
    AgentTaskListOptions, AgentTaskRepository, DelegatedSessionRepository, TeamRepository,
};

use active_delegations::{
    render_active_delegations, render_active_delegations_unavailable,
};
use branch_status::{render_branch_status, BranchStatusCache};
pub(crate) use linked_plan::{
    linked_plan_snapshot_resolver_from_app_state, LinkedPlanSnapshotResolver,
};
use linked_plan::{
    render_linked_plan_identity, render_linked_plan_unavailable,
};
use task_ledger::{render_task_ledger, render_task_ledger_unavailable};
pub(crate) use task_runtime::{
    build_task_runtime_context_prompt, task_runtime_state_for_context,
};
use team::{render_team_state, render_team_state_unavailable};
pub(crate) use workspace::format_agent_workspace_source_pull_request_prompt_context;

pub(crate) struct AgentRuntimeContextScope<'a> {
    pub conversation_id: &'a ChatConversationId,
    pub context_type: ChatContextType,
    pub context_id: &'a str,
    pub project_id: Option<&'a str>,
    pub workspace: Option<&'a AgentConversationWorkspace>,
    pub working_directory: &'a Path,
    pub entity_status: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeContextBlock {
    Workspace,
    TaskRuntime,
    ActiveDelegations,
    TaskLedger,
    BranchStatus,
    LinkedPlan,
    TeamState,
}

impl RuntimeContextBlock {
    pub(crate) const ALL: [Self; 7] = [
        Self::Workspace,
        Self::TaskRuntime,
        Self::ActiveDelegations,
        Self::TaskLedger,
        Self::BranchStatus,
        Self::LinkedPlan,
        Self::TeamState,
    ];

    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Workspace => "agent_workspace_context",
            Self::TaskRuntime => "task_runtime_context",
            Self::ActiveDelegations => "active_delegations",
            Self::TaskLedger => "task_ledger",
            Self::BranchStatus => "branch_status",
            Self::LinkedPlan => "linked_plan",
            Self::TeamState => "team_state",
        }
    }
}

#[derive(Clone)]
pub(crate) struct AgentRuntimeContextDeps {
    delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
    agent_task_repo: Arc<dyn AgentTaskRepository>,
    linked_plan_snapshot_resolver: Option<Arc<dyn LinkedPlanSnapshotResolver>>,
    team_repo: Option<Arc<dyn TeamRepository>>,
    branch_status_cache: Option<BranchStatusCache>,
    budget: Duration,
}

impl AgentRuntimeContextDeps {
    pub(crate) fn new(
        delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
        agent_task_repo: Arc<dyn AgentTaskRepository>,
    ) -> Self {
        Self {
            delegated_session_repo,
            agent_task_repo,
            linked_plan_snapshot_resolver: None,
            team_repo: None,
            branch_status_cache: None,
            budget: Duration::from_millis(
                crate::infrastructure::agents::claude::git_runtime_config()
                    .agent_runtime_context_budget_ms,
            ),
        }
    }

    pub(crate) fn with_linked_plan_snapshot_resolver(
        mut self,
        resolver: Arc<dyn LinkedPlanSnapshotResolver>,
    ) -> Self {
        self.linked_plan_snapshot_resolver = Some(resolver);
        self
    }

    pub(crate) fn with_team_repo(mut self, team_repo: Arc<dyn TeamRepository>) -> Self {
        self.team_repo = Some(team_repo);
        self
    }

    pub(crate) fn with_branch_status_cache(mut self, cache: BranchStatusCache) -> Self {
        self.branch_status_cache = Some(cache);
        self
    }

    pub(crate) fn schedule_branch_status_refresh_if_due(
        &self,
        workspace_path: &Path,
        base_ref: Option<&str>,
    ) {
        let Some(cache) = self.branch_status_cache.as_ref() else {
            return;
        };
        cache.schedule_refresh_if_due(
            workspace_path.to_path_buf(),
            base_ref.map(str::to_string),
            chrono::Duration::seconds(
                crate::infrastructure::agents::claude::git_runtime_config()
                    .agent_runtime_branch_status_refresh_secs as i64,
            ),
        );
    }

    #[cfg(test)]
    pub(crate) fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }
}

pub(crate) async fn compose_agent_runtime_context(
    scope: &AgentRuntimeContextScope<'_>,
    deps: &AgentRuntimeContextDeps,
) -> Option<String> {
    let started_at = Instant::now();
    let mut blocks = Vec::new();

    for block in RuntimeContextBlock::ALL {
        let rendered = match block {
            RuntimeContextBlock::Workspace => scope
                .workspace
                .and_then(format_agent_workspace_source_pull_request_prompt_context),
            RuntimeContextBlock::TaskRuntime => build_task_runtime_context_prompt(
                scope.context_type,
                scope.context_id,
                scope.entity_status,
                scope.project_id,
                scope.working_directory,
            )
            .unwrap_or_else(|error| {
                tracing::warn!(error, "task runtime context unavailable");
                Some(format!(
                    "<task_runtime_context state=\"unavailable\" reason=\"{}\"/>",
                    crate::application::chat_service::escape_attr(&error)
                ))
            }),
            RuntimeContextBlock::ActiveDelegations => {
                let remaining = deps.budget.saturating_sub(started_at.elapsed());
                if remaining.is_zero() {
                    Some(render_active_delegations_unavailable("budget_exceeded"))
                } else {
                    let caller_conversation_id = scope.conversation_id.as_str();
                    match tokio::time::timeout(
                        remaining,
                        deps.delegated_session_repo
                            .list_active_by_caller_conversation(&caller_conversation_id),
                    )
                    .await
                    {
                        Ok(Ok(sessions)) => render_active_delegations(sessions),
                        Ok(Err(error)) => {
                            tracing::warn!(error = %error, "active delegation context unavailable");
                            Some(render_active_delegations_unavailable("repository_error"))
                        }
                        Err(_) => {
                            tracing::warn!("active delegation context exceeded compose budget");
                            Some(render_active_delegations_unavailable("budget_exceeded"))
                        }
                    }
                }
            }
            RuntimeContextBlock::TaskLedger => {
                let remaining = deps.budget.saturating_sub(started_at.elapsed());
                if remaining.is_zero() {
                    Some(render_task_ledger_unavailable("budget_exceeded"))
                } else {
                    let mut task_scope =
                        AgentTaskScope::new("conversation", scope.conversation_id.as_str());
                    task_scope.project_id = scope
                        .project_id
                        .map(|id| crate::domain::entities::ProjectId::from_string(id.to_string()));
                    match tokio::time::timeout(
                        remaining,
                        deps.agent_task_repo.list_tasks(
                            &task_scope,
                            AgentTaskListOptions {
                                include_done: false,
                            },
                        ),
                    )
                    .await
                    {
                        Ok(Ok(tasks)) => render_task_ledger(tasks),
                        Ok(Err(error)) => {
                            tracing::warn!(error = %error, "task ledger context unavailable");
                            Some(render_task_ledger_unavailable("repository_error"))
                        }
                        Err(_) => {
                            tracing::warn!("task ledger context exceeded compose budget");
                            Some(render_task_ledger_unavailable("budget_exceeded"))
                        }
                    }
                }
            }
            RuntimeContextBlock::LinkedPlan => {
                match scope.workspace {
                    Some(workspace) if workspace.linked_ideation_session_id.is_some() => {
                        let remaining = deps.budget.saturating_sub(started_at.elapsed());
                        if remaining.is_zero() {
                            Some(render_linked_plan_unavailable("budget_exceeded"))
                        } else {
                            match deps.linked_plan_snapshot_resolver.as_ref() {
                                Some(resolver) => {
                                    match tokio::time::timeout(
                                        remaining,
                                        resolver.resolve(workspace),
                                    )
                                    .await
                                    {
                                        Ok(Ok(Some(snapshot))) => {
                                            Some(render_linked_plan_identity(&snapshot))
                                        }
                                        Ok(Ok(None)) => None,
                                        Ok(Err(error)) => {
                                            tracing::warn!(
                                                error = %error,
                                                "linked plan runtime context unavailable"
                                            );
                                            Some(render_linked_plan_unavailable("resolution_error"))
                                        }
                                        Err(_) => {
                                            tracing::warn!(
                                                "linked plan runtime context exceeded compose budget"
                                            );
                                            Some(render_linked_plan_unavailable("budget_exceeded"))
                                        }
                                    }
                                }
                                None => {
                                    Some(render_linked_plan_unavailable("resolver_unavailable"))
                                }
                            }
                        }
                    }
                    _ => None,
                }
            }
            RuntimeContextBlock::TeamState => {
                let remaining = deps.budget.saturating_sub(started_at.elapsed());
                if remaining.is_zero() {
                    Some(render_team_state_unavailable("budget_exceeded"))
                } else if let Some(team_repo) = deps.team_repo.as_ref() {
                    match tokio::time::timeout(
                        remaining,
                        team_repo.get_open_session_for_conversation(scope.conversation_id),
                    )
                    .await
                    {
                        Ok(Ok(Some(session))) => {
                            let remaining = deps.budget.saturating_sub(started_at.elapsed());
                            if remaining.is_zero() {
                                Some(render_team_state_unavailable("budget_exceeded"))
                            } else {
                                match tokio::time::timeout(
                                    remaining,
                                    team_repo.list_members(&session.id),
                                )
                                .await
                                {
                                    Ok(Ok(members)) => Some(render_team_state(&session, members)),
                                    Ok(Err(error)) => {
                                        tracing::warn!(error = %error, "team runtime context unavailable");
                                        Some(render_team_state_unavailable("repository_error"))
                                    }
                                    Err(_) => {
                                        tracing::warn!("team runtime context exceeded compose budget");
                                        Some(render_team_state_unavailable("budget_exceeded"))
                                    }
                                }
                            }
                        }
                        Ok(Ok(None)) => None,
                        Ok(Err(error)) => {
                            tracing::warn!(error = %error, "team runtime context unavailable");
                            Some(render_team_state_unavailable("repository_error"))
                        }
                        Err(_) => {
                            tracing::warn!("team runtime context exceeded compose budget");
                            Some(render_team_state_unavailable("budget_exceeded"))
                        }
                    }
                } else {
                    None
                }
            }
            RuntimeContextBlock::BranchStatus => scope.workspace.map(|_| {
                deps.branch_status_cache.as_ref().map_or_else(
                    || {
                        render_branch_status(
                            &BranchStatusCache::default(),
                            scope.working_directory,
                            chrono::Utc::now(),
                            chrono::Duration::seconds(
                                crate::infrastructure::agents::claude::git_runtime_config()
                                    .agent_runtime_branch_status_stale_secs
                                    as i64,
                            ),
                        )
                    },
                    |cache| {
                        render_branch_status(
                            cache,
                            scope.working_directory,
                            chrono::Utc::now(),
                            chrono::Duration::seconds(
                                crate::infrastructure::agents::claude::git_runtime_config()
                                    .agent_runtime_branch_status_stale_secs
                                    as i64,
                            ),
                        )
                    },
                )
            }),
        };
        if let Some(rendered) = rendered {
            tracing::trace!(block = block.tag(), "composed agent runtime context block");
            blocks.push(rendered);
        }
    }

    if blocks.is_empty() {
        None
    } else {
        Some(format!(
            "<agent_runtime_state>\n{}\n</agent_runtime_state>",
            blocks.join("\n")
        ))
    }
}
