use super::ideation_commands_append::*;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ArtifactId, ChatConversation,
    ChatConversationId, ChatMessage, ExecutionPlan, ExecutionPlanId, IdeationAnalysisBaseRefKind,
    IdeationSession, IdeationSessionId, IdeationSessionStatus, InternalStatus, MessageRole,
    PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use crate::error::AppError;

struct AcceptedPlanFixture {
    state: AppState,
    project_id: ProjectId,
    session_id: IdeationSessionId,
    execution_plan_id: String,
    plan_branch_id: String,
    merge_task_id: TaskId,
}

async fn accepted_plan_fixture(merge_status: InternalStatus) -> AcceptedPlanFixture {
    let state = AppState::new_sqlite_for_apply_test();
    let mut project = Project::new("RalphX".to_string(), "/tmp/ralphx-append-test".to_string());
    project.base_branch = Some("main".to_string());
    let project = state.project_repo.create(project).await.unwrap();

    let mut session =
        IdeationSession::new_with_title(project.id.clone(), "Accepted append target".to_string());
    session.status = IdeationSessionStatus::Accepted;
    session.plan_artifact_id = Some(ArtifactId::from_string("plan-artifact-append-test"));
    let session = state.ideation_session_repo.create(session).await.unwrap();

    let execution_plan = state
        .execution_plan_repo
        .create(ExecutionPlan::new(session.id.clone()))
        .await
        .unwrap();

    let mut branch = PlanBranch::new(
        session.plan_artifact_id.clone().unwrap(),
        session.id.clone(),
        project.id.clone(),
        "ralphx/ralphx/plan-append-test".to_string(),
        "main".to_string(),
    );
    branch.status = PlanBranchStatus::Active;
    branch.execution_plan_id = Some(execution_plan.id.clone());
    let branch = state.plan_branch_repo.create(branch).await.unwrap();

    let mut merge_task = Task::new_with_category(
        project.id.clone(),
        "Merge plan into main".to_string(),
        TaskCategory::PlanMerge,
    );
    merge_task.internal_status = merge_status;
    merge_task.plan_artifact_id = session.plan_artifact_id.clone();
    merge_task.ideation_session_id = Some(session.id.clone());
    merge_task.execution_plan_id = Some(execution_plan.id.clone());
    merge_task.blocked_reason = match merge_status {
        InternalStatus::Blocked => Some("Waiting for all plan tasks to complete".to_string()),
        _ => None,
    };
    let merge_task = state.task_repo.create(merge_task).await.unwrap();
    state
        .plan_branch_repo
        .set_merge_task_id(&branch.id, &merge_task.id)
        .await
        .unwrap();

    AcceptedPlanFixture {
        state,
        project_id: project.id,
        session_id: session.id,
        execution_plan_id: execution_plan.id.as_str().to_string(),
        plan_branch_id: branch.id.as_str().to_string(),
        merge_task_id: merge_task.id,
    }
}

async fn create_plan_task(
    fixture: &AcceptedPlanFixture,
    title: &str,
    status: InternalStatus,
) -> Task {
    let mut task = Task::new(fixture.project_id.clone(), title.to_string());
    task.internal_status = status;
    task.ideation_session_id = Some(fixture.session_id.clone());
    task.execution_plan_id = Some(ExecutionPlanId::from_string(
        fixture.execution_plan_id.clone(),
    ));
    fixture.state.task_repo.create(task).await.unwrap()
}

async fn attach_tasks_conversation(
    fixture: &AcceptedPlanFixture,
    publication_pr_status: Option<&str>,
) -> (String, String) {
    attach_tasks_conversation_for_session(
        fixture,
        fixture.session_id.clone(),
        publication_pr_status,
    )
    .await
}

async fn attach_tasks_conversation_for_session(
    fixture: &AcceptedPlanFixture,
    task_pipeline_session_id: IdeationSessionId,
    publication_pr_status: Option<&str>,
) -> (String, String) {
    let mut conversation = ChatConversation::new_project(fixture.project_id.clone());
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Tasks));
    let conversation = fixture
        .state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut workspace = AgentConversationWorkspace::new(
        conversation.id.clone(),
        fixture.project_id.clone(),
        AgentConversationWorkspaceMode::Tasks,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        None,
        "ralphx/test/tasks-follow-up".to_string(),
        "/tmp/ralphx-tasks-follow-up".to_string(),
    );
    workspace.task_pipeline_session_id = Some(task_pipeline_session_id);
    workspace.publication_pr_status = publication_pr_status.map(str::to_string);
    fixture
        .state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let mut message = ChatMessage::user_in_project(
        fixture.project_id.clone(),
        "Please address this follow-up on the same PR",
    );
    message.conversation_id = Some(conversation.id.clone());
    let message = fixture
        .state
        .chat_message_repo
        .create(message)
        .await
        .unwrap();
    (
        conversation.id.as_str().to_string(),
        message.id.as_str().to_string(),
    )
}

fn tasks_follow_up_input(
    fixture: &AcceptedPlanFixture,
    source_conversation_id: Option<String>,
    source_message_id: Option<String>,
) -> AppendIdeationPlanTaskInput {
    AppendIdeationPlanTaskInput {
        project_id: Some(fixture.project_id.as_str().to_string()),
        session_id: fixture.session_id.as_str().to_string(),
        title: "Address requested Tasks follow-up".to_string(),
        description: None,
        steps: vec![],
        acceptance_criteria: vec!["Follow-up is covered".to_string()],
        depends_on_task_ids: vec![],
        priority: None,
        source_conversation_id,
        source_message_id,
    }
}

async fn append_follow_up(
    fixture: &AcceptedPlanFixture,
    title: &str,
    depends_on_task_ids: Vec<String>,
) -> AppendIdeationPlanTaskResult {
    append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: title.to_string(),
            description: Some("Append follow-up work.".to_string()),
            steps: vec!["Implement the follow-up".to_string()],
            acceptance_criteria: vec!["Follow-up is complete".to_string()],
            depends_on_task_ids,
            priority: None,
            source_conversation_id: None,
            source_message_id: None,
        },
    )
    .await
    .unwrap()
}

async fn appended_task(fixture: &AcceptedPlanFixture, task_id: &str) -> Task {
    fixture
        .state
        .task_repo
        .get_by_id(&TaskId::from_string(task_id.to_string()))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn append_infers_leaf_blockers_before_merge_when_no_explicit_dependencies() {
    let fixture = accepted_plan_fixture(InternalStatus::Ready).await;
    let foundation =
        create_plan_task(&fixture, "Foundation plan task", InternalStatus::Merged).await;
    let leaf = create_plan_task(&fixture, "Leaf plan task", InternalStatus::Merged).await;
    fixture
        .state
        .task_dependency_repo
        .add_dependency(&leaf.id, &foundation.id)
        .await
        .unwrap();
    fixture
        .state
        .task_dependency_repo
        .add_dependency(&fixture.merge_task_id, &foundation.id)
        .await
        .unwrap();
    fixture
        .state
        .task_dependency_repo
        .add_dependency(&fixture.merge_task_id, &leaf.id)
        .await
        .unwrap();

    let result = append_follow_up(&fixture, "Append after leaf", vec![]).await;

    assert_eq!(result.task_status, "ready");
    assert_eq!(result.dependencies_created, 2);
    let appended = appended_task(&fixture, &result.task_id).await;
    let appended_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&appended.id)
        .await
        .unwrap();
    assert_eq!(appended_blockers.len(), 1);
    assert!(appended_blockers.contains(&leaf.id));
    assert!(!appended_blockers.contains(&foundation.id));

    let merge_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&fixture.merge_task_id)
        .await
        .unwrap();
    assert!(merge_blockers.contains(&appended.id));
}

#[tokio::test]
async fn append_inferred_active_leaf_blocks_appended_task() {
    let fixture = accepted_plan_fixture(InternalStatus::Ready).await;
    let active_leaf =
        create_plan_task(&fixture, "Active leaf task", InternalStatus::Executing).await;

    let result = append_follow_up(&fixture, "Append behind active leaf", vec![]).await;

    assert_eq!(result.task_status, "blocked");
    assert!(!result.any_ready_tasks);
    let appended = appended_task(&fixture, &result.task_id).await;
    assert_eq!(appended.internal_status, InternalStatus::Blocked);
    assert!(appended
        .blocked_reason
        .as_deref()
        .unwrap()
        .contains("Active leaf task"));
    let appended_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&appended.id)
        .await
        .unwrap();
    assert_eq!(appended_blockers, vec![active_leaf.id]);
}

#[tokio::test]
async fn append_explicit_dependencies_are_validated_and_respected() {
    let fixture = accepted_plan_fixture(InternalStatus::Ready).await;
    let inferred_leaf = create_plan_task(&fixture, "Inferred leaf", InternalStatus::Merged).await;
    let explicit_blocker =
        create_plan_task(&fixture, "Explicit blocker", InternalStatus::Merged).await;

    let result = append_follow_up(
        &fixture,
        "Append with explicit blocker",
        vec![explicit_blocker.id.as_str().to_string()],
    )
    .await;

    assert_eq!(result.task_status, "ready");
    assert_eq!(result.dependencies_created, 2);
    let appended = appended_task(&fixture, &result.task_id).await;
    let appended_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&appended.id)
        .await
        .unwrap();
    assert_eq!(appended_blockers.len(), 1);
    assert!(appended_blockers.contains(&explicit_blocker.id));
    assert!(!appended_blockers.contains(&inferred_leaf.id));

    let merge_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&fixture.merge_task_id)
        .await
        .unwrap();
    assert!(merge_blockers.contains(&appended.id));
}

#[tokio::test]
async fn append_rejects_invalid_explicit_dependency_without_partial_insert() {
    let fixture = accepted_plan_fixture(InternalStatus::Ready).await;

    let error = append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: "Invalid dependency append".to_string(),
            description: None,
            steps: vec![],
            acceptance_criteria: vec![],
            depends_on_task_ids: vec![fixture.merge_task_id.as_str().to_string()],
            priority: None,
            source_conversation_id: None,
            source_message_id: None,
        },
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("Cannot use the plan merge task as an appended task blocker"));
    let tasks = fixture
        .state
        .task_repo
        .get_by_project(&fixture.project_id)
        .await
        .unwrap();
    assert!(!tasks
        .iter()
        .any(|task| task.title == "Invalid dependency append"));
}

#[tokio::test]
async fn append_creates_linked_task_steps_and_merge_dependency() {
    let fixture = accepted_plan_fixture(InternalStatus::Ready).await;

    let result = append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: "Polish publish CTA".to_string(),
            description: Some("Tune the managed publish action copy.".to_string()),
            steps: vec![
                "Find the publish panel action".to_string(),
                "Adjust the CTA treatment".to_string(),
            ],
            acceptance_criteria: vec!["CTA copy is clear".to_string()],
            depends_on_task_ids: vec![],
            priority: Some(8),
            source_conversation_id: Some("conversation-1".to_string()),
            source_message_id: Some("message-1".to_string()),
        },
    )
    .await
    .unwrap();

    assert_eq!(result.project_id, fixture.project_id.as_str());
    assert_eq!(result.session_id, fixture.session_id.as_str());
    assert_eq!(result.execution_plan_id, fixture.execution_plan_id);
    assert_eq!(result.plan_branch_id, fixture.plan_branch_id);
    assert_eq!(result.merge_task_id, fixture.merge_task_id.as_str());
    assert_eq!(result.task_status, "ready");
    assert_eq!(result.dependencies_created, 1);
    assert!(result.any_ready_tasks);

    let appended_task = fixture
        .state
        .task_repo
        .get_by_id(&TaskId::from_string(result.task_id.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(appended_task.category, TaskCategory::Regular);
    assert_eq!(appended_task.internal_status, InternalStatus::Ready);
    assert_eq!(
        appended_task.ideation_session_id,
        Some(fixture.session_id.clone())
    );
    assert_eq!(
        appended_task.execution_plan_id.as_ref().unwrap().as_str(),
        fixture.execution_plan_id
    );
    assert!(appended_task.source_proposal_id.is_none());
    assert!(appended_task
        .metadata
        .as_deref()
        .unwrap()
        .contains("ideation_plan_append"));
    assert!(appended_task
        .metadata
        .as_deref()
        .unwrap()
        .contains("CTA copy is clear"));

    let steps = fixture
        .state
        .task_step_repo
        .get_by_task(&appended_task.id)
        .await
        .unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].title, "Find the publish panel action");
    assert_eq!(steps[0].created_by, "ideation_plan_append");

    let merge_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&fixture.merge_task_id)
        .await
        .unwrap();
    assert!(merge_blockers.contains(&appended_task.id));

    let merge_task = fixture
        .state
        .task_repo
        .get_by_id(&fixture.merge_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merge_task.internal_status, InternalStatus::Blocked);
    assert!(merge_task
        .blocked_reason
        .as_deref()
        .unwrap()
        .contains("Polish publish CTA"));
}

#[tokio::test]
async fn append_allows_waiting_on_pr_plan_and_blocks_merge_again() {
    let fixture = accepted_plan_fixture(InternalStatus::WaitingOnPr).await;
    let plan_branch_id = PlanBranchId::from_string(fixture.plan_branch_id.clone());
    fixture
        .state
        .plan_branch_repo
        .update_last_polled_at(&plan_branch_id, chrono::Utc::now())
        .await
        .unwrap();

    let result = append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: "Apply requested PR adjustment".to_string(),
            description: Some("Handle a follow-up while the plan PR is still open.".to_string()),
            steps: vec!["Make the requested adjustment".to_string()],
            acceptance_criteria: vec!["The existing PR includes the follow-up".to_string()],
            depends_on_task_ids: vec![],
            priority: None,
            source_conversation_id: None,
            source_message_id: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.task_status, "ready");
    assert!(result.any_ready_tasks);

    let appended_task = fixture
        .state
        .task_repo
        .get_by_id(&TaskId::from_string(result.task_id))
        .await
        .unwrap()
        .unwrap();
    let merge_blockers = fixture
        .state
        .task_dependency_repo
        .get_blockers(&fixture.merge_task_id)
        .await
        .unwrap();
    assert!(merge_blockers.contains(&appended_task.id));

    let merge_task = fixture
        .state
        .task_repo
        .get_by_id(&fixture.merge_task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merge_task.internal_status, InternalStatus::Blocked);
    assert!(merge_task
        .blocked_reason
        .as_deref()
        .unwrap()
        .contains("Apply requested PR adjustment"));

    let branch = fixture
        .state
        .plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !branch.pr_polling_active,
        "appending to a waiting-on-PR plan must stop the stale PR poller"
    );
}

#[tokio::test]
async fn tasks_follow_up_requires_owning_user_message_and_rejects_replay() {
    let fixture = accepted_plan_fixture(InternalStatus::Blocked).await;
    let (conversation_id, message_id) = attach_tasks_conversation(&fixture, Some("open")).await;
    let input = AppendIdeationPlanTaskInput {
        project_id: Some(fixture.project_id.as_str().to_string()),
        session_id: fixture.session_id.as_str().to_string(),
        title: "Address requested PR follow-up".to_string(),
        description: None,
        steps: vec![],
        acceptance_criteria: vec!["Follow-up is covered".to_string()],
        depends_on_task_ids: vec![],
        priority: None,
        source_conversation_id: Some(conversation_id),
        source_message_id: Some(message_id),
    };

    append_ideation_plan_task_core(&fixture.state, input.clone())
        .await
        .unwrap();
    let count_after_first = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();
    let replay_error = append_ideation_plan_task_core(&fixture.state, input)
        .await
        .unwrap_err();
    let count_after_replay = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();

    assert!(matches!(replay_error, AppError::Conflict(_)));
    assert_eq!(count_after_replay, count_after_first);
}

#[tokio::test]
async fn tasks_owned_pipeline_rejects_append_without_source_identity() {
    let fixture = accepted_plan_fixture(InternalStatus::Blocked).await;
    attach_tasks_conversation(&fixture, Some("open")).await;
    let task_count_before = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();

    let error = append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: "Bypass explicit Tasks consent".to_string(),
            description: None,
            steps: vec![],
            acceptance_criteria: vec![],
            depends_on_task_ids: vec![],
            priority: None,
            source_conversation_id: None,
            source_message_id: None,
        },
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("require an explicit source user message"));
    assert_eq!(
        fixture
            .state
            .task_repo
            .get_by_ideation_session(&fixture.session_id)
            .await
            .unwrap()
            .len(),
        task_count_before,
        "missing Tasks consent must not create a follow-up task",
    );
}

#[tokio::test]
async fn tasks_owned_pipeline_rejects_incomplete_or_foreign_source_identity() {
    let fixture = accepted_plan_fixture(InternalStatus::Blocked).await;
    let (conversation_id, message_id) = attach_tasks_conversation(&fixture, Some("open")).await;
    let task_count_before = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();

    let missing_message_id = append_ideation_plan_task_core(
        &fixture.state,
        tasks_follow_up_input(&fixture, Some(conversation_id.clone()), None),
    )
    .await
    .unwrap_err();
    assert!(missing_message_id
        .to_string()
        .contains("require an explicit source user message"));

    let foreign_conversation = append_ideation_plan_task_core(
        &fixture.state,
        tasks_follow_up_input(
            &fixture,
            Some("different-conversation".to_string()),
            Some(message_id.clone()),
        ),
    )
    .await
    .unwrap_err();
    assert!(foreign_conversation
        .to_string()
        .contains("owning conversation"));

    let missing_message = append_ideation_plan_task_core(
        &fixture.state,
        tasks_follow_up_input(
            &fixture,
            Some(conversation_id),
            Some("missing-message".to_string()),
        ),
    )
    .await
    .unwrap_err();
    assert!(missing_message
        .to_string()
        .contains("source message was not found"));

    assert_eq!(
        fixture
            .state
            .task_repo
            .get_by_ideation_session(&fixture.session_id)
            .await
            .unwrap()
            .len(),
        task_count_before,
        "rejected Tasks identities must not create follow-up tasks",
    );
}

#[tokio::test]
async fn tasks_owned_pipeline_rejects_non_user_source_message() {
    let fixture = accepted_plan_fixture(InternalStatus::Blocked).await;
    let (conversation_id, _) = attach_tasks_conversation(&fixture, Some("open")).await;
    let mut assistant_message = ChatMessage::user_in_project(
        fixture.project_id.clone(),
        "This assistant message cannot authorize follow-up work",
    );
    assistant_message.role = MessageRole::Orchestrator;
    assistant_message.conversation_id =
        Some(ChatConversationId::from_string(conversation_id.clone()));
    let assistant_message = fixture
        .state
        .chat_message_repo
        .create(assistant_message)
        .await
        .unwrap();
    let task_count_before = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();

    let error = append_ideation_plan_task_core(
        &fixture.state,
        tasks_follow_up_input(
            &fixture,
            Some(conversation_id),
            Some(assistant_message.id.as_str().to_string()),
        ),
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("reference a user message from the owning conversation"));
    assert_eq!(
        fixture
            .state
            .task_repo
            .get_by_ideation_session(&fixture.session_id)
            .await
            .unwrap()
            .len(),
        task_count_before,
    );
}

#[tokio::test]
async fn tasks_conversation_cannot_authorize_a_different_pipeline() {
    let fixture = accepted_plan_fixture(InternalStatus::Blocked).await;
    let (conversation_id, message_id) = attach_tasks_conversation_for_session(
        &fixture,
        IdeationSessionId::from_string("different-pipeline"),
        Some("open"),
    )
    .await;
    let task_count_before = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();

    let error = append_ideation_plan_task_core(
        &fixture.state,
        tasks_follow_up_input(&fixture, Some(conversation_id), Some(message_id)),
    )
    .await
    .unwrap_err();

    assert!(error.to_string().contains("not attached to this pipeline"));
    assert_eq!(
        fixture
            .state
            .task_repo
            .get_by_ideation_session(&fixture.session_id)
            .await
            .unwrap()
            .len(),
        task_count_before,
    );
}

#[tokio::test]
async fn tasks_follow_up_rejects_closed_pr_without_creating_task() {
    let fixture = accepted_plan_fixture(InternalStatus::Blocked).await;
    let (conversation_id, message_id) = attach_tasks_conversation(&fixture, Some("closed")).await;
    let task_count_before = fixture
        .state
        .task_repo
        .get_by_ideation_session(&fixture.session_id)
        .await
        .unwrap()
        .len();

    let error = append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: "Too late after PR close".to_string(),
            description: None,
            steps: vec![],
            acceptance_criteria: vec![],
            depends_on_task_ids: vec![],
            priority: None,
            source_conversation_id: Some(conversation_id),
            source_message_id: Some(message_id),
        },
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("pull request is closed or merged"));
    assert_eq!(
        fixture
            .state
            .task_repo
            .get_by_ideation_session(&fixture.session_id)
            .await
            .unwrap()
            .len(),
        task_count_before
    );
}

#[tokio::test]
async fn append_rejects_after_merge_has_started() {
    let fixture = accepted_plan_fixture(InternalStatus::PendingMerge).await;

    let error = append_ideation_plan_task_core(
        &fixture.state,
        AppendIdeationPlanTaskInput {
            project_id: Some(fixture.project_id.as_str().to_string()),
            session_id: fixture.session_id.as_str().to_string(),
            title: "Too late".to_string(),
            description: None,
            steps: vec![],
            acceptance_criteria: vec![],
            depends_on_task_ids: vec![],
            priority: None,
            source_conversation_id: None,
            source_message_id: None,
        },
    )
    .await
    .unwrap_err();

    assert!(error
        .to_string()
        .contains("Cannot append a task to a closed or actively merging plan"));
}
