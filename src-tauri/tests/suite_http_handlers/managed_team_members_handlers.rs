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
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, AgentRun, ChatContextType, ChatConversation,
    CoordinationMode, DelegatedSession, DelegatedSessionId, Project, TeamMember, TeamMemberId,
    TeamMemberStatus, TeamRunBindingStatus, TeamSessionStatus,
};
use ralphx_lib::http_server::handlers::{
    add_managed_team_member, assign_managed_team_member, exit_managed_team,
    get_managed_team_member_status, list_idle_managed_team_members, send_managed_team_message,
    stop_managed_team_member,
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

async fn seed_provider(app_state: &AppState, harness: AgentHarnessKind, enabled: bool, is_default: bool) {
    let mut settings = AgentProviderSettings::disabled_defaults(harness);
    settings.enabled = enabled;
    settings.is_default = is_default;
    app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("provider settings should persist");
}

/// RX-TEAM-006: `team_add_member` must pre-flight the provider onboarding gate instead of
/// deferring failure to first dispatch.
#[tokio::test]
async fn add_member_rejects_disabled_provider_harness() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    seed_provider(&fixture.state.app_state, AgentHarnessKind::Claude, true, true).await;
    seed_provider(&fixture.state.app_state, AgentHarnessKind::Codex, false, false).await;

    let mut request = add_request("Worker");
    request.harness = Some("codex".to_string());
    let status = error_status(
        add_managed_team_member(State(fixture.state.clone()), fixture.headers.clone(), Json(request))
            .await,
    );
    assert_eq!(status, StatusCode::CONFLICT);

    let listed = list_idle_managed_team_members(State(fixture.state), fixture.headers)
        .await
        .expect("list passes authority");
    assert!(
        listed.0.is_empty(),
        "no member row should be created when the provider gate rejects the request"
    );
}

#[tokio::test]
async fn add_member_allows_enabled_provider_harness() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    seed_provider(&fixture.state.app_state, AgentHarnessKind::Claude, true, true).await;
    seed_provider(&fixture.state.app_state, AgentHarnessKind::Codex, true, false).await;

    let mut request = add_request("Worker");
    request.harness = Some("codex".to_string());
    let added = add_managed_team_member(
        State(fixture.state),
        fixture.headers,
        Json(request),
    )
    .await
    .expect("enabled provider harness should be allowed");
    assert_eq!(added.0.status, "idle");
}

#[tokio::test]
async fn add_member_omitted_harness_checks_default_provider() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    // Codex is the enabled default; Claude (DEFAULT_AGENT_HARNESS) is left disabled to prove the
    // omitted-harness path gates on DEFAULT_AGENT_HARNESS specifically, not the default provider.
    seed_provider(&fixture.state.app_state, AgentHarnessKind::Codex, true, true).await;
    seed_provider(&fixture.state.app_state, AgentHarnessKind::Claude, false, false).await;

    let request = add_request("Worker");
    assert!(request.harness.is_none());
    let status = error_status(
        add_managed_team_member(State(fixture.state.clone()), fixture.headers.clone(), Json(request))
            .await,
    );
    assert_eq!(status, StatusCode::CONFLICT);

    let listed = list_idle_managed_team_members(State(fixture.state), fixture.headers)
        .await
        .expect("list passes authority");
    assert!(listed.0.is_empty());
}

async fn seed_member_with_delegated_session(
    fixture: &Fixture,
    delegated_session_id: Option<DelegatedSessionId>,
    status: TeamMemberStatus,
) -> TeamMember {
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
        delegated_session_id,
        generation: 0,
        current_run_id: Some(fixture.run.id),
        current_assignment_id: None,
        status,
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

/// RX-TEAM-005a: `team_status` joins the roster to delegated-session liveness for a working
/// member whose delegated session is still resolvable.
#[tokio::test]
async fn member_status_reports_working_member_liveness() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let delegated_session = DelegatedSession::new(
        fixture.session.project_id.clone(),
        ChatContextType::Project.to_string(),
        fixture.session.project_id.as_str(),
        "ralphx-general-worker",
        AgentHarnessKind::Claude,
    );
    let delegated_session = fixture
        .state
        .app_state
        .delegated_session_repo
        .create(delegated_session)
        .await
        .expect("create delegated session");
    let member = seed_member_with_delegated_session(
        &fixture,
        Some(delegated_session.id.clone()),
        TeamMemberStatus::Working,
    )
    .await;

    let status = get_managed_team_member_status(State(fixture.state), fixture.headers)
        .await
        .expect("status passes authority");
    assert_eq!(status.0.len(), 1);
    let entry = &status.0[0];
    assert_eq!(entry.name, member.name);
    assert_eq!(entry.is_running, Some(true));
    assert!(
        entry.last_active_at.is_some(),
        "working member should report a liveness timestamp"
    );
}

/// Degrade, never fail: a member whose delegated session was already cleared still appears in
/// the roster projection with no liveness fields, and the request still returns 200.
#[tokio::test]
async fn member_status_degrades_when_delegated_session_is_missing() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let missing_session_id = DelegatedSessionId::new();
    let member = seed_member_with_delegated_session(
        &fixture,
        Some(missing_session_id),
        TeamMemberStatus::Working,
    )
    .await;

    let status = get_managed_team_member_status(State(fixture.state), fixture.headers)
        .await
        .expect("status must degrade rather than fail when the delegated session is missing");
    assert_eq!(status.0.len(), 1);
    let entry = &status.0[0];
    assert_eq!(entry.name, member.name);
    assert!(entry.is_running.is_none());
    assert!(entry.last_active_at.is_none());
    assert!(entry.estimated_status.is_none());
    assert!(entry.latest_run.is_none());
}

/// Unlike `team_roster`, `team_status` is coordinator-only: a member-shaped binding must be
/// rejected with 403.
#[tokio::test]
async fn member_status_rejects_member_authority() {
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
        error_status(
            get_managed_team_member_status(State(fixture.state), fixture.headers).await
        ),
        StatusCode::FORBIDDEN
    );
}

/// `team_status` must never leak a second Team's members into the caller's projection.
#[tokio::test]
async fn member_status_never_crosses_team_boundaries() {
    let fixture = fixture(Some("ralphx-general-worker")).await;
    let member_a = seed_authoritative_member(&fixture).await;

    let project_b = fixture
        .state
        .app_state
        .project_repo
        .create(Project::new(
            "Second Team project".to_string(),
            std::env::current_dir()
                .expect("absolute workspace path")
                .display()
                .to_string(),
        ))
        .await
        .expect("create second project");
    let mut conversation_b = ChatConversation::new_project(project_b.id.clone());
    conversation_b.coordination_mode = CoordinationMode::RxNativeTeam;
    conversation_b.agent_mode = Some(AgentConversationWorkspaceMode::Edit);
    let conversation_b = fixture
        .state
        .app_state
        .chat_conversation_repo
        .create(conversation_b)
        .await
        .expect("create second coordinator conversation");
    let session_b = fixture
        .state
        .app_state
        .managed_team
        .ensure_team(project_b.id, &conversation_b.id)
        .await
        .expect("ensure second Team session");
    let mut run_b = AgentRun::new(conversation_b.id);
    run_b.agent_name = Some("ralphx-general-worker".to_string());
    let run_b = fixture
        .state
        .app_state
        .agent_run_repo
        .create(run_b)
        .await
        .expect("create second coordinator run");
    let mut binding_b =
        new_coordinator_run_binding(session_b.id.clone(), conversation_b.id, run_b.id);
    binding_b.status = TeamRunBindingStatus::Running;
    fixture
        .state
        .app_state
        .managed_team
        .run_binding_repo()
        .create(binding_b)
        .await
        .expect("create second coordinator binding");
    let mut headers_b = HeaderMap::new();
    headers_b.insert(
        "x-ralphx-conversation-id",
        HeaderValue::from_str(&conversation_b.id.as_str()).expect("conversation header"),
    );
    headers_b.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_str(&run_b.id.as_str()).expect("run header"),
    );

    let status_b = get_managed_team_member_status(State(fixture.state.clone()), headers_b)
        .await
        .expect("status passes authority for second Team");
    assert!(
        status_b.0.is_empty(),
        "second Team's status must not see the first Team's member"
    );

    let status_a = get_managed_team_member_status(State(fixture.state), fixture.headers)
        .await
        .expect("status passes authority for first Team");
    assert_eq!(status_a.0.len(), 1);
    assert_eq!(status_a.0[0].name, member_a.name);
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
