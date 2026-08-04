use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};
use chrono::Utc;
use ralphx_lib::application::managed_team::{new_coordinator_run_binding, ManagedTeamService};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, AgentRun, ChatConversation, CoordinationMode, Project,
    TeamMember, TeamMemberId, TeamMemberStatus, TeamRunBindingStatus, TeamSessionStatus,
};
use ralphx_lib::http_server::handlers::{
    add_managed_team_member, assign_managed_team_member, exit_managed_team,
    list_idle_managed_team_members, send_managed_team_message, stop_managed_team_member,
};
use ralphx_lib::http_server::types::{
    AddManagedTeamMemberRequest, AssignManagedTeamMemberRequest, ExitManagedTeamRequest,
    HttpServerState, SendManagedTeamMessageRequest, StopManagedTeamMemberRequest,
};
use ralphx_lib::infrastructure::memory::{
    MemoryQueuedMessageRepository, MemoryTeamCoordinationTransitionRepository,
    MemoryTeamMessageRepository, MemoryTeamRepository, MemoryTeamRunBindingRepository,
    MemoryTeamWakeBatchRepository, MemoryTeamWorkspaceReservationRepository,
};

struct Fixture {
    state: HttpServerState,
    headers: HeaderMap,
    session: ralphx_lib::domain::entities::TeamSession,
    run: AgentRun,
}

async fn fixture(agent_name: Option<&str>) -> Fixture {
    let mut app_state = AppState::new_sqlite_test();
    let sessions = MemoryTeamRepository::new_shared_sessions();
    app_state.managed_team = Arc::new(ManagedTeamService::new(
        Arc::new(MemoryTeamRepository::with_sessions(Arc::clone(&sessions))),
        Arc::new(MemoryTeamCoordinationTransitionRepository::with_sessions(
            sessions,
        )),
        Arc::new(MemoryTeamRunBindingRepository::new()),
        Arc::new(MemoryTeamMessageRepository::new()),
        Arc::new(MemoryTeamWakeBatchRepository::new()),
        Arc::new(MemoryQueuedMessageRepository::new()),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::new(MemoryTeamWorkspaceReservationRepository::new()),
        Arc::clone(&app_state.ui_feature_flag_overrides_repo),
    ));
    let app_state = Arc::new(app_state);
    app_state
        .ui_feature_flag_overrides_repo
        .update_agent_capabilities(Some(true), None, None)
        .await
        .expect("enable Team capability");
    let project = app_state
        .project_repo
        .create(Project::new(
            "Team authority project".to_string(),
            std::env::current_dir()
                .expect("absolute workspace path")
                .display()
                .to_string(),
        ))
        .await
        .expect("create project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Edit);
    // Deliberately never set: this regression protects top-level coordinators.
    assert!(conversation.parent_conversation_id.is_none());
    assert!(conversation.bound_agent_name.is_none());
    let conversation = app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create parentless coordinator conversation");
    let session = app_state
        .managed_team
        .ensure_team(project.id, &conversation.id)
        .await
        .expect("ensure Team session");
    let mut run = AgentRun::new(conversation.id);
    run.agent_name = agent_name.map(str::to_string);
    let run = app_state
        .agent_run_repo
        .create(run)
        .await
        .expect("create active coordinator run");
    let mut binding = new_coordinator_run_binding(session.id.clone(), conversation.id, run.id);
    binding.status = TeamRunBindingStatus::Running;
    app_state
        .managed_team
        .run_binding_repo()
        .create(binding)
        .await
        .expect("create coordinator binding");
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        HeaderValue::from_str(&conversation.id.as_str()).expect("conversation header"),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_str(&run.id.as_str()).expect("run header"),
    );
    Fixture {
        state: HttpServerState {
            app_state,
            execution_state: Arc::new(ExecutionState::new()),
            delegation_service: Default::default(),
        },
        headers,
        session,
        run,
    }
}

fn add_request(name: &str) -> AddManagedTeamMemberRequest {
    AddManagedTeamMemberRequest {
        name: name.to_string(),
        canonical_agent_name: "ralphx-general-worker".to_string(),
        role_summary: "test teammate".to_string(),
        harness: None,
        logical_model: None,
        logical_effort: None,
    }
}

fn error_status<T>(result: Result<T, (StatusCode, Json<serde_json::Value>)>) -> StatusCode {
    match result {
        Ok(_) => panic!("handler should reject request"),
        Err(error) => error.0,
    }
}

async fn seed_authoritative_member(fixture: &Fixture) -> TeamMember {
    let now = Utc::now();
    let member = TeamMember {
        id: TeamMemberId::new(),
        team_id: fixture.session.id.clone(),
        normalized_name: "member".to_string(),
        name: "Member".to_string(),
        canonical_agent_name: "ralphx-general-worker".to_string(),
        role_summary: "member".to_string(),
        harness: None,
        logical_model: None,
        logical_effort: None,
        delegated_session_id: None,
        generation: 0,
        current_run_id: Some(fixture.run.id),
        current_assignment_id: None,
        status: TeamMemberStatus::Working,
        last_activity_at: None,
        last_error: None,
        created_at: now,
        updated_at: now,
        stopped_at: None,
    };
    fixture
        .state
        .app_state
        .managed_team
        .team_repo()
        .create_member(member)
        .await
        .expect("seed member")
}

#[tokio::test]
async fn parentless_coordinator_passes_authority_for_add_and_list() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let added = add_managed_team_member(
        State(fixture.state.clone()),
        fixture.headers.clone(),
        Json(add_request("Worker")),
    )
    .await
    .expect("add passes authority");
    let listed = list_idle_managed_team_members(State(fixture.state), fixture.headers)
        .await
        .expect("list passes authority");
    assert_eq!(added.0.name, "Worker");
    assert_eq!(listed.0.len(), 1);
}

#[tokio::test]
async fn parentless_coordinator_passes_authority_for_stop_and_exit() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let _ = add_managed_team_member(
        State(fixture.state.clone()),
        fixture.headers.clone(),
        Json(add_request("Worker")),
    )
    .await
    .expect("add member");
    let _ = stop_managed_team_member(
        State(fixture.state.clone()),
        fixture.headers.clone(),
        Json(StopManagedTeamMemberRequest {
            member_name: "Worker".to_string(),
        }),
    )
    .await
    .expect("stop passes authority");
    exit_managed_team(
        State(fixture.state.clone()),
        fixture.headers,
        Json(ExitManagedTeamRequest {
            action: "suspend".to_string(),
        }),
    )
    .await
    .expect("exit passes authority");
    let conversation = fixture
        .state
        .app_state
        .chat_conversation_repo
        .get_by_id(&fixture.run.conversation_id)
        .await
        .expect("read conversation")
        .expect("conversation");
    assert_eq!(conversation.coordination_mode, CoordinationMode::Solo);
}

#[tokio::test]
async fn assign_authority_passes_without_bound_agent_name() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let status = error_status(
        assign_managed_team_member(
            State(fixture.state),
            fixture.headers,
            Json(AssignManagedTeamMemberRequest {
                member_name: "missing".to_string(),
                task_ref: "task".to_string(),
                work_classification: "read_only".to_string(),
                writable_paths: vec![],
                generated_outputs: vec![],
                resource_locks: vec![],
            }),
        )
        .await,
    );
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_run_agent_name_fails_closed() {
    let fixture = fixture(None).await;
    assert_eq!(
        error_status(list_idle_managed_team_members(State(fixture.state), fixture.headers).await),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn member_shaped_binding_is_rejected() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let member = seed_authoritative_member(&fixture).await;
    let binding = fixture
        .state
        .app_state
        .managed_team
        .run_binding_repo()
        .get_by_agent_run_id(&fixture.run.id)
        .await
        .expect("read binding")
        .expect("binding");
    let mut replacement = binding.clone();
    replacement.team_member_id = Some(member.id);
    replacement.team_member_generation = Some(member.generation);
    replacement.work_classification =
        ralphx_lib::domain::entities::TeamWorkClassification::ReadOnly;
    fixture
        .state
        .app_state
        .managed_team
        .run_binding_repo()
        .transition(&binding.id, binding.version, replacement)
        .await
        .expect("replace binding");
    assert_eq!(
        error_status(list_idle_managed_team_members(State(fixture.state), fixture.headers).await),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn stale_run_header_is_rejected() {
    let mut fixture = fixture(Some("ralphx-general-worker")).await;
    fixture.headers.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_static("stale-run"),
    );
    assert_eq!(
        error_status(list_idle_managed_team_members(State(fixture.state), fixture.headers).await),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn closed_session_is_rejected() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let mut closed = fixture.session.clone();
    closed.status = TeamSessionStatus::Closed;
    closed.closed_at = Some(Utc::now());
    fixture
        .state
        .app_state
        .managed_team
        .team_repo()
        .update_session(closed, fixture.session.version)
        .await
        .expect("close session");
    assert_eq!(
        error_status(list_idle_managed_team_members(State(fixture.state), fixture.headers).await),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn messaging_path_still_authorizes_coordinator() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let status = error_status(
        send_managed_team_message(
            State(fixture.state),
            fixture.headers,
            Json(SendManagedTeamMessageRequest {
                target: "broadcast".to_string(),
                member_name: None,
                kind: None,
                content: "hello".to_string(),
            }),
        )
        .await,
    );
    assert_eq!(status, StatusCode::CONFLICT);
}
