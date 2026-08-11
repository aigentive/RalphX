use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::application::agent_runtime_context::{
    compose_agent_runtime_context, AgentRuntimeContextDeps, AgentRuntimeContextScope,
};
use crate::application::AgentTaskService;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentTaskCreate, AgentTaskScope,
    ChatContextType, ChatConversationId, DelegatedSession, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::domain::repositories::DelegatedSessionRepository;
use crate::infrastructure::memory::{MemoryAgentTaskRepository, MemoryDelegatedSessionRepository};

fn empty_deps() -> AgentRuntimeContextDeps {
    AgentRuntimeContextDeps::new(
        Arc::new(MemoryDelegatedSessionRepository::new()),
        Arc::new(MemoryAgentTaskRepository::new()),
    )
}

#[tokio::test]
async fn active_delegations_include_recoverable_job_identity() {
    let conversation_id = ChatConversationId::from_string("conversation-delegations");
    let project_id = ProjectId::from_string("project-delegations".to_string());
    let delegated_repo = Arc::new(MemoryDelegatedSessionRepository::new());
    let mut session = DelegatedSession::new(
        project_id.clone(),
        "project",
        project_id.as_str(),
        "ralphx-general-worker<&",
        AgentHarnessKind::Codex,
    );
    session.caller_conversation_id = Some(conversation_id.as_str().to_string());
    session.job_id = Some("job-<&".to_string());
    delegated_repo
        .create(session)
        .await
        .expect("delegate should persist");
    let deps =
        AgentRuntimeContextDeps::new(delegated_repo, Arc::new(MemoryAgentTaskRepository::new()));
    let scope = AgentRuntimeContextScope {
        conversation_id: &conversation_id,
        context_type: ChatContextType::Project,
        context_id: project_id.as_str(),
        project_id: Some(project_id.as_str()),
        workspace: None,
        working_directory: Path::new("/tmp/runtime-context-delegations"),
        entity_status: None,
    };

    let rendered = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("active delegation should render");

    assert!(rendered.contains("<active_delegations>"));
    assert!(rendered.contains("job_id=\"job-&lt;&amp;\""));
    assert!(rendered.contains("agent=\"ralphx-general-worker&lt;&amp;\""));
    assert!(rendered.contains("delegate_wait or delegate_cancel"));
}

#[tokio::test]
async fn task_ledger_lists_unresolved_tasks_in_actionable_order() {
    let conversation_id = ChatConversationId::from_string("conversation-ledger");
    let project_id = ProjectId::from_string("project-ledger".to_string());
    let task_repo = Arc::new(MemoryAgentTaskRepository::new());
    let task_service = AgentTaskService::new(task_repo.clone());
    let mut task_scope = AgentTaskScope::new("conversation", conversation_id.as_str());
    task_scope.project_id = Some(project_id.clone());
    task_service
        .create_task(
            &task_scope,
            AgentTaskCreate {
                title: "Inspect <state>".to_string(),
                details: "details".to_string(),
                active_label: None,
                owner_agent: Some("ralphx-general-worker".to_string()),
                metadata: None,
                blocked_by: Vec::new(),
                blocks: Vec::new(),
            },
        )
        .await
        .expect("task should persist");
    task_service
        .create_task(
            &task_scope,
            AgentTaskCreate {
                title: "Already active".to_string(),
                details: "details".to_string(),
                active_label: None,
                owner_agent: None,
                metadata: None,
                blocked_by: Vec::new(),
                blocks: Vec::new(),
            },
        )
        .await
        .expect("second task should persist");
    task_service
        .claim_task(&task_scope, "2", Some("ralphx-general-worker".to_string()))
        .await
        .expect("claim should succeed")
        .expect("second task should exist");
    let deps =
        AgentRuntimeContextDeps::new(Arc::new(MemoryDelegatedSessionRepository::new()), task_repo);
    let scope = AgentRuntimeContextScope {
        conversation_id: &conversation_id,
        context_type: ChatContextType::Project,
        context_id: project_id.as_str(),
        project_id: Some(project_id.as_str()),
        workspace: None,
        working_directory: Path::new("/tmp/runtime-context-ledger"),
        entity_status: None,
    };

    let rendered = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("open ledger should render");

    assert!(rendered.contains("<task_ledger>"));
    assert!(rendered.contains("task_ref=\"1\""));
    assert!(rendered.contains("title=\"Inspect &lt;state&gt;\""));
    assert!(rendered.contains("state=\"open\""));
    let open_index = rendered
        .find("task_ref=\"1\"")
        .expect("open task should render");
    let active_index = rendered
        .find("task_ref=\"2\"")
        .expect("active task should render");
    assert!(open_index < active_index);
    assert!(rendered.contains("assignee=\"ralphx-general-worker\""));
}

#[tokio::test]
async fn delegation_and_ledger_caps_bound_large_runtime_state() {
    let conversation_id = ChatConversationId::from_string("conversation-runtime-caps");
    let project_id = ProjectId::from_string("project-runtime-caps".to_string());
    let delegated_repo = Arc::new(MemoryDelegatedSessionRepository::new());
    for index in 0..100 {
        let mut session = DelegatedSession::new(
            project_id.clone(),
            "project",
            project_id.as_str(),
            format!("delegate-{index}"),
            AgentHarnessKind::Codex,
        );
        session.caller_conversation_id = Some(conversation_id.as_str().to_string());
        session.job_id = Some(format!("job-{index}"));
        delegated_repo
            .create(session)
            .await
            .expect("delegate should persist");
    }

    let task_repo = Arc::new(MemoryAgentTaskRepository::new());
    let task_service = AgentTaskService::new(task_repo.clone());
    let mut task_scope = AgentTaskScope::new("conversation", conversation_id.as_str());
    task_scope.project_id = Some(project_id.clone());
    for index in 0..500 {
        task_service
            .create_task(
                &task_scope,
                AgentTaskCreate {
                    title: format!("Task {index}"),
                    details: format!("Bounded runtime-state task {index}"),
                    active_label: None,
                    owner_agent: None,
                    metadata: None,
                    blocked_by: Vec::new(),
                    blocks: Vec::new(),
                },
            )
            .await
            .expect("task should persist");
    }

    let deps = AgentRuntimeContextDeps::new(delegated_repo, task_repo);
    let rendered = compose_agent_runtime_context(
        &AgentRuntimeContextScope {
            conversation_id: &conversation_id,
            context_type: ChatContextType::Project,
            context_id: project_id.as_str(),
            project_id: Some(project_id.as_str()),
            workspace: None,
            working_directory: Path::new("/tmp/runtime-context-caps"),
            entity_status: None,
        },
        &deps,
    )
    .await
    .expect("bounded runtime state should render");

    assert_eq!(rendered.matches("<delegate ").count(), 20);
    assert_eq!(rendered.matches("<task ").count(), 50);
    assert!(rendered.len() < 20_000);
}

#[tokio::test]
async fn empty_runtime_state_omits_the_envelope() {
    let conversation_id = ChatConversationId::from_string("conversation-empty");
    let scope = AgentRuntimeContextScope {
        conversation_id: &conversation_id,
        context_type: ChatContextType::Standalone,
        context_id: "standalone",
        project_id: None,
        workspace: None,
        working_directory: Path::new("/tmp/runtime-context-empty"),
        entity_status: None,
    };

    let rendered = compose_agent_runtime_context(&scope, &empty_deps()).await;

    assert_eq!(rendered, None);
}

#[tokio::test]
async fn exhausted_budget_is_explicit_instead_of_reading_as_empty_state() {
    let conversation_id = ChatConversationId::new();
    let deps = empty_deps().with_budget(Duration::ZERO);
    let scope = AgentRuntimeContextScope {
        conversation_id: &conversation_id,
        context_type: ChatContextType::Standalone,
        context_id: "standalone",
        project_id: None,
        workspace: None,
        working_directory: Path::new("/tmp/runtime-context-budget"),
        entity_status: None,
    };

    let rendered = compose_agent_runtime_context(&scope, &deps)
        .await
        .expect("budget exhaustion should be explicit");

    assert!(
        rendered.contains("<active_delegations state=\"unavailable\" reason=\"budget_exceeded\"/>")
    );
    assert!(rendered.contains("<task_ledger state=\"unavailable\" reason=\"budget_exceeded\"/>"));
}

#[tokio::test]
async fn workspace_and_task_runtime_blocks_share_one_ordered_envelope() {
    let conversation_id = ChatConversationId::from_string("conversation-<&");
    let project_id = ProjectId::from_string("project-<&".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project_id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::LocalBranch,
        "main".to_string(),
        None,
        None,
        "ralphx/runtime-context".to_string(),
        "/tmp/runtime-context".to_string(),
    );
    workspace.base_ref = "main<&".to_string();
    let scope = AgentRuntimeContextScope {
        conversation_id: &conversation_id,
        context_type: ChatContextType::TaskExecution,
        context_id: "task-<&",
        project_id: Some(project_id.as_str()),
        workspace: Some(&workspace),
        working_directory: Path::new("/tmp/runtime-context<&"),
        entity_status: Some("executing"),
    };

    let rendered = compose_agent_runtime_context(&scope, &empty_deps())
        .await
        .expect("workspace and task state should render");

    assert!(rendered.starts_with("<agent_runtime_state>"));
    assert!(rendered.ends_with("</agent_runtime_state>"));
    let workspace_index = rendered
        .find("<agent_workspace_context>")
        .expect("workspace block should be present");
    let task_index = rendered
        .find("<task_runtime_context>")
        .expect("task runtime block should be present");
    assert!(workspace_index < task_index);
    assert!(rendered.contains("base_ref>main&lt;&amp;</base_ref>"));
    assert!(rendered.contains("task-&lt;&amp;"));
    assert_eq!(rendered.matches("<agent_runtime_state>").count(), 1);
}
