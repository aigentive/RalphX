use std::sync::Arc;

use crate::application::managed_team::{
    ManagedTeamAssignmentRequest, ManagedTeamMemberSpec, ManagedTeamService,
    ManagedTeamWorkspaceRequest,
};
use crate::application::AgentTaskService;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentTaskAssignmentId, AgentTaskCreate, AgentTaskScope, ChatConversation,
    DelegatedSessionId, ProjectId, TeamMemberStatus, TeamRunBindingStatus, TeamWorkClassification,
    TeamWorkspaceReservation, TeamWorkspaceReservationId,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentTaskRepository, TeamWorkspaceReservationRepository,
    UiFeatureFlagOverridesRepository,
};
use crate::infrastructure::memory::{
    MemoryAgentRunRepository, MemoryAgentTaskRepository, MemoryChatConversationRepository,
    MemoryQueuedMessageRepository, MemoryTeamCoordinationTransitionRepository,
    MemoryTeamMessageRepository, MemoryTeamRepository, MemoryTeamRunBindingRepository,
    MemoryTeamWakeBatchRepository, MemoryTeamWorkspaceReservationRepository,
    MemoryUiFeatureFlagOverridesRepository,
};
use crate::testing::team_fixtures::{member_run_binding, team_agent_run_id, team_conversation_id};

fn build_service() -> ManagedTeamService {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::new(MemoryChatConversationRepository::new()),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    )
}

fn build_service_with_runs() -> (ManagedTeamService, Arc<MemoryAgentRunRepository>) {
    let sessions = MemoryTeamRepository::new_shared_sessions();
    let runs = Arc::new(MemoryAgentRunRepository::new());
    let service = ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::new(MemoryChatConversationRepository::new()),
        Arc::clone(&runs) as Arc<dyn AgentRunRepository>,
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    (service, runs)
}

fn task(title: &str) -> AgentTaskCreate {
    AgentTaskCreate {
        title: title.to_string(),
        details: format!("{title} details"),
        active_label: None,
        owner_agent: None,
        metadata: None,
        blocked_by: Vec::new(),
        blocks: Vec::new(),
    }
}

async fn create_member(service: &ManagedTeamService) -> crate::domain::entities::TeamMember {
    ensure_project_conversation(
        service,
        ProjectId::from_string("project-1".to_string()),
        team_conversation_id(1),
    )
    .await;
    let team = service
        .ensure_team(
            ProjectId::from_string("project-1".to_string()),
            &team_conversation_id(1),
        )
        .await
        .unwrap();
    service
        .add_member(
            &team.id,
            ManagedTeamMemberSpec {
                name: "Writer One".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "writes bounded code".to_string(),
                harness: Some(AgentHarnessKind::Codex),
                logical_model: Some("gpt-5.6".to_string()),
                logical_effort: None,
            },
        )
        .await
        .unwrap()
}

async fn ensure_project_conversation(
    service: &ManagedTeamService,
    project_id: ProjectId,
    conversation_id: crate::domain::entities::ChatConversationId,
) {
    if service
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .unwrap()
        .is_none()
    {
        let mut conversation = ChatConversation::new_project(project_id);
        conversation.id = conversation_id;
        service
            .chat_conversation_repo
            .create(conversation)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn add_member_is_lazy_normalized_and_harness_neutral() {
    let service = build_service();
    let member = create_member(&service).await;

    assert_eq!(member.normalized_name, "writer one");
    assert_eq!(member.status, TeamMemberStatus::Idle);
    assert_eq!(member.harness, Some(AgentHarnessKind::Codex));
    assert!(member.delegated_session_id.is_none());
    assert!(member.current_run_id.is_none());
}

#[tokio::test]
async fn add_member_rejects_normalized_name_conflicts() {
    let service = build_service();
    let member = create_member(&service).await;
    let duplicate = service
        .add_member(
            &member.team_id,
            ManagedTeamMemberSpec {
                name: " writer   one ".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "duplicate".to_string(),
                harness: Some(AgentHarnessKind::Claude),
                logical_model: None,
                logical_effort: None,
            },
        )
        .await;

    assert!(duplicate.is_err());
}

#[tokio::test]
async fn idle_members_preserve_mixed_claude_and_codex_harness_metadata() {
    let service = build_service();
    let codex = create_member(&service).await;
    let claude = service
        .add_member(
            &codex.team_id,
            ManagedTeamMemberSpec {
                name: "Reviewer Two".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "reviews bounded changes".to_string(),
                harness: Some(AgentHarnessKind::Claude),
                logical_model: Some("claude-sonnet".to_string()),
                logical_effort: None,
            },
        )
        .await
        .unwrap();

    let idle = service.idle_members(&codex.team_id).await.unwrap();
    assert_eq!(idle.len(), 2);
    assert!(idle.iter().any(|member| {
        member.id == codex.id && member.harness == Some(AgentHarnessKind::Codex)
    }));
    assert!(idle.iter().any(|member| {
        member.id == claude.id && member.harness == Some(AgentHarnessKind::Claude)
    }));
}

#[tokio::test]
async fn mixed_claude_codex_leads_and_members_follow_the_same_assignment_saga() {
    for (lead_harness, member_harness) in [
        (AgentHarnessKind::Claude, AgentHarnessKind::Codex),
        (AgentHarnessKind::Codex, AgentHarnessKind::Claude),
    ] {
        let (service, runs) = build_service_with_runs();
        ensure_project_conversation(
            &service,
            ProjectId::from_string("project-1".to_string()),
            team_conversation_id(1),
        )
        .await;
        let team = service
            .ensure_team(
                ProjectId::from_string("project-1".to_string()),
                &team_conversation_id(1),
            )
            .await
            .unwrap();
        let member = service
            .add_member(
                &team.id,
                ManagedTeamMemberSpec {
                    name: format!("member-{member_harness:?}"),
                    canonical_agent_name: "ralphx-general-worker".to_string(),
                    role_summary: "mixed-provider assignment member".to_string(),
                    harness: Some(member_harness),
                    logical_model: None,
                    logical_effort: None,
                },
            )
            .await
            .unwrap();
        let mut lead = AgentRun::new(team_conversation_id(1));
        lead.id = team_agent_run_id(if lead_harness == AgentHarnessKind::Claude {
            41
        } else {
            42
        });
        lead.harness = Some(lead_harness);
        runs.create(lead.clone()).await.unwrap();
        let repo = Arc::new(MemoryAgentTaskRepository::new());
        let tasks = AgentTaskService::new(Arc::clone(&repo) as Arc<dyn AgentTaskRepository>);
        let mut scope = AgentTaskScope::new("conversation", team_conversation_id(1).as_str());
        scope.project_id = Some(ProjectId::from_string("project-1".to_string()));
        tasks
            .create_task(&scope, task("mixed provider task"))
            .await
            .unwrap();
        tasks
            .create_task(&scope, task("second task keeps ledger claimable"))
            .await
            .unwrap();

        let plan = service
            .plan_member_assignment(
                &tasks,
                ManagedTeamAssignmentRequest {
                    team_id: team.id.clone(),
                    member_name: member.normalized_name.clone(),
                    expected_member_generation: member.generation,
                    caller_scope: scope,
                    caller_agent_run_id: lead.id,
                    task_ref: "1".to_string(),
                    delegated_session_id: DelegatedSessionId::new(),
                    delegated_conversation_id: team_conversation_id(2),
                    planned_agent_run_id: AgentRunId::new(),
                    work_classification: TeamWorkClassification::ReadOnly,
                    workspace: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(plan.member.harness, Some(member_harness));
        assert_eq!(plan.binding.team_id, team.id);
        service
            .fail_member_assignment_launch(&tasks, &plan, "mixed_provider_test_cleanup")
            .await;
    }
}

#[tokio::test]
async fn launch_rechecks_member_only_configured_concurrency_after_parallel_planning() {
    let (service, runs) = build_service_with_runs();
    let first = create_member(&service).await;
    let second = service
        .add_member(
            &first.team_id,
            ManagedTeamMemberSpec {
                name: "Second writer".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "second bounded writer".to_string(),
                harness: Some(AgentHarnessKind::Claude),
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();
    let mut session = service
        .team_repo()
        .get_session(&first.team_id)
        .await
        .unwrap()
        .unwrap();
    session.configured_concurrency = 1;
    session.version += 1;
    assert!(service
        .team_repo()
        .update_session(session, 0)
        .await
        .unwrap());
    let repo = Arc::new(MemoryAgentTaskRepository::new());
    let tasks = AgentTaskService::new(Arc::clone(&repo) as Arc<dyn AgentTaskRepository>);
    let mut scope = AgentTaskScope::new("conversation", team_conversation_id(1).as_str());
    scope.project_id = Some(ProjectId::from_string("project-1".to_string()));
    tasks
        .create_task(&scope, task("first concurrent task"))
        .await
        .unwrap();
    tasks
        .create_task(&scope, task("second concurrent task"))
        .await
        .unwrap();
    let first_plan = service
        .plan_member_assignment(
            &tasks,
            ManagedTeamAssignmentRequest {
                team_id: first.team_id.clone(),
                member_name: first.normalized_name.clone(),
                expected_member_generation: first.generation,
                caller_scope: scope.clone(),
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "1".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(2),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::ReadOnly,
                workspace: None,
            },
        )
        .await
        .unwrap();
    let second_plan = service
        .plan_member_assignment(
            &tasks,
            ManagedTeamAssignmentRequest {
                team_id: second.team_id.clone(),
                member_name: second.normalized_name.clone(),
                expected_member_generation: second.generation,
                caller_scope: scope,
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "2".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(3),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::ReadOnly,
                workspace: None,
            },
        )
        .await
        .unwrap();
    for plan in [&first_plan, &second_plan] {
        let mut run = AgentRun::new(plan.binding.conversation_id);
        run.id = plan.binding.agent_run_id;
        run.harness = Some(AgentHarnessKind::Codex);
        runs.create(run).await.unwrap();
    }
    service
        .mark_member_assignment_launching(&first_plan)
        .await
        .unwrap();
    assert!(service
        .mark_member_assignment_launching(&second_plan)
        .await
        .unwrap_err()
        .to_string()
        .contains("configured concurrency"));
    service
        .fail_member_assignment_launch(&tasks, &first_plan, "test_cleanup")
        .await;
    service
        .fail_member_assignment_launch(&tasks, &second_plan, "test_cleanup")
        .await;
}

#[tokio::test]
async fn write_assignment_fails_closed_without_workspace_reservation() {
    let service = build_service();
    let member = create_member(&service).await;
    let repo = Arc::new(MemoryAgentTaskRepository::new());
    let task_service = AgentTaskService::new(Arc::clone(&repo) as Arc<dyn AgentTaskRepository>);
    let mut scope = AgentTaskScope::new("conversation", team_conversation_id(1).as_str());
    scope.project_id = Some(ProjectId::from_string("project-1".to_string()));
    task_service
        .create_task(&scope, task("first"))
        .await
        .unwrap();
    task_service
        .create_task(&scope, task("second"))
        .await
        .unwrap();

    let result = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: member.team_id.clone(),
                member_name: member.normalized_name.clone(),
                expected_member_generation: member.generation,
                caller_scope: scope,
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "1".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(2),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::Write,
                workspace: None,
            },
        )
        .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("require a workspace reservation"));
}

#[tokio::test]
async fn stale_member_generation_rejects_before_task_reservation() {
    let service = build_service();
    let member = create_member(&service).await;
    let task_service = AgentTaskService::new(
        Arc::new(MemoryAgentTaskRepository::new()) as Arc<dyn AgentTaskRepository>
    );
    let result = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: member.team_id.clone(),
                member_name: member.normalized_name.clone(),
                expected_member_generation: member.generation + 1,
                caller_scope: AgentTaskScope::new("conversation", team_conversation_id(1).as_str()),
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "1".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(2),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::ReadOnly,
                workspace: None,
            },
        )
        .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("generation is stale"));
}

#[tokio::test]
async fn busy_member_rejects_retask_and_launch_failure_reopens_task() {
    let service = build_service();
    let member = create_member(&service).await;
    let repo = Arc::new(MemoryAgentTaskRepository::new());
    let task_service = AgentTaskService::new(Arc::clone(&repo) as Arc<dyn AgentTaskRepository>);
    let mut scope = AgentTaskScope::new("conversation", team_conversation_id(1).as_str());
    scope.project_id = Some(ProjectId::from_string("project-1".to_string()));
    task_service
        .create_task(&scope, task("first"))
        .await
        .unwrap();
    task_service
        .create_task(&scope, task("second"))
        .await
        .unwrap();

    let plan = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: member.team_id.clone(),
                member_name: member.normalized_name.clone(),
                expected_member_generation: member.generation,
                caller_scope: scope.clone(),
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "1".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(2),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::ReadOnly,
                workspace: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        plan.assignment.assignment.team_id,
        Some(member.team_id.clone())
    );
    assert_eq!(
        plan.assignment.assignment.team_member_id,
        Some(member.id.clone())
    );
    assert_eq!(
        plan.assignment.assignment.team_member_generation,
        Some(member.generation)
    );
    assert_eq!(plan.binding.team_member_id, Some(member.id.clone()));
    assert_eq!(plan.binding.team_member_generation, Some(member.generation));
    let retask = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: member.team_id.clone(),
                member_name: member.normalized_name.clone(),
                expected_member_generation: member.generation,
                caller_scope: scope.clone(),
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "2".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(3),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::ReadOnly,
                workspace: None,
            },
        )
        .await;
    assert!(retask.unwrap_err().to_string().contains("member is busy"));

    service
        .fail_member_assignment_launch(&task_service, &plan, "injected_launch_failure")
        .await;
    let reopened = task_service.get_task(&scope, "1").await.unwrap().unwrap();
    assert_eq!(reopened.state.as_str(), "open");
    let idle = service
        .member_by_normalized_name(&member.team_id, &member.normalized_name)
        .await
        .unwrap();
    assert_eq!(idle.status, TeamMemberStatus::Idle);

    let retry = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: member.team_id.clone(),
                member_name: member.normalized_name.clone(),
                expected_member_generation: idle.generation,
                caller_scope: scope,
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "2".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(4),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::ReadOnly,
                workspace: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(retry.member.status, TeamMemberStatus::Working);
    service
        .fail_member_assignment_launch(&task_service, &retry, "test_cleanup")
        .await;
}

#[tokio::test]
async fn conflicting_write_reservation_rejects_second_member_before_task_reservation() {
    let service = build_service();
    let first = create_member(&service).await;
    let second = service
        .add_member(
            &first.team_id,
            ManagedTeamMemberSpec {
                name: "Writer Two".to_string(),
                canonical_agent_name: "ralphx-general-worker".to_string(),
                role_summary: "owns a distinct write task".to_string(),
                harness: Some(AgentHarnessKind::Claude),
                logical_model: None,
                logical_effort: None,
            },
        )
        .await
        .unwrap();
    let repo = Arc::new(MemoryAgentTaskRepository::new());
    let task_service = AgentTaskService::new(Arc::clone(&repo) as Arc<dyn AgentTaskRepository>);
    let mut scope = AgentTaskScope::new("conversation", team_conversation_id(1).as_str());
    scope.project_id = Some(ProjectId::from_string("project-1".to_string()));
    task_service
        .create_task(&scope, task("first"))
        .await
        .unwrap();
    task_service
        .create_task(&scope, task("second"))
        .await
        .unwrap();

    let first_plan = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: first.team_id.clone(),
                member_name: first.normalized_name.clone(),
                expected_member_generation: first.generation,
                caller_scope: scope.clone(),
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "1".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(2),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::Write,
                workspace: Some(ManagedTeamWorkspaceRequest {
                    writable_paths: vec!["src/member.rs".to_string()],
                    generated_outputs: Vec::new(),
                    resource_locks: Vec::new(),
                }),
            },
        )
        .await
        .unwrap();
    let conflict = service
        .plan_member_assignment(
            &task_service,
            ManagedTeamAssignmentRequest {
                team_id: second.team_id.clone(),
                member_name: second.normalized_name.clone(),
                expected_member_generation: second.generation,
                caller_scope: scope.clone(),
                caller_agent_run_id: team_agent_run_id(1),
                task_ref: "2".to_string(),
                delegated_session_id: DelegatedSessionId::new(),
                delegated_conversation_id: team_conversation_id(3),
                planned_agent_run_id: AgentRunId::new(),
                work_classification: TeamWorkClassification::Write,
                workspace: Some(ManagedTeamWorkspaceRequest {
                    writable_paths: vec!["src/member.rs".to_string()],
                    generated_outputs: Vec::new(),
                    resource_locks: Vec::new(),
                }),
            },
        )
        .await;

    assert!(conflict
        .unwrap_err()
        .to_string()
        .contains("conflicts with an active reservation"));
    assert_eq!(
        task_service
            .get_task(&scope, "2")
            .await
            .unwrap()
            .unwrap()
            .state
            .as_str(),
        "open"
    );
    service
        .fail_member_assignment_launch(&task_service, &first_plan, "test_cleanup")
        .await;
}

#[tokio::test]
async fn idle_member_can_be_stopped_without_provider_process() {
    let service = build_service();
    let member = create_member(&service).await;
    let stopped = service
        .stop_member(&member.team_id, &member.normalized_name, member.generation)
        .await
        .unwrap();

    assert_eq!(stopped.status, TeamMemberStatus::Stopped);
    assert!(stopped.current_run_id.is_none());
}

#[tokio::test]
async fn terminal_binding_recovery_releases_its_exact_workspace_reservation() {
    let service = build_service();
    let member = create_member(&service).await;
    let assignment_id = AgentTaskAssignmentId::from_string("assignment-1");
    let mut binding = member_run_binding(
        "terminal-binding",
        member.team_id.as_str(),
        23,
        member.id.as_str(),
        member.generation,
    );
    binding.assignment_id = Some(assignment_id.clone());
    binding.status = TeamRunBindingStatus::Failed;
    service.run_binding_repo().create(binding).await.unwrap();
    let reservation_repo = Arc::new(MemoryTeamWorkspaceReservationRepository::new());
    let recovery_service = ManagedTeamService::new(
        service.team_repo(),
        Arc::new(MemoryTeamCoordinationTransitionRepository::new()),
        service.run_binding_repo(),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::new(MemoryChatConversationRepository::new()),
        Arc::new(MemoryAgentRunRepository::new()),
        Arc::clone(&reservation_repo) as Arc<dyn TeamWorkspaceReservationRepository>,
        Arc::new(MemoryUiFeatureFlagOverridesRepository::new())
            as Arc<dyn UiFeatureFlagOverridesRepository>,
    );
    reservation_repo
        .acquire(TeamWorkspaceReservation {
            id: TeamWorkspaceReservationId::new(),
            team_id: member.team_id.clone(),
            team_member_id: member.id.clone(),
            assignment_id: Some(assignment_id.clone()),
            team_member_generation: member.generation,
            writable_paths: vec!["src/team.rs".to_string()],
            generated_outputs: Vec::new(),
            resource_locks: Vec::new(),
            work_classification: TeamWorkClassification::Write,
            attempt_number: 1,
            acquired_at: chrono::Utc::now(),
            released_at: None,
        })
        .await
        .unwrap();

    assert_eq!(
        recovery_service
            .recover_terminal_binding_reservations()
            .await
            .unwrap(),
        1
    );
    assert!(reservation_repo
        .list_active_for_assignment(assignment_id.as_str())
        .await
        .unwrap()
        .is_empty());
}
