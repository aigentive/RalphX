//! Wake message content is proven with a fake chat sender in
//! `application/delegation_park/wake_dispatch_tests.rs`; this suite proves the production
//! settlement/reconciliation wiring and durable park lifecycle.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{extract::State, http::HeaderMap, Json};
use chrono::{DateTime, Utc};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::AgentHarnessKind;
use ralphx_lib::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatConversation, ChatConversationId, DelegatedSession,
    DelegationPark, DelegationParkId, DelegationParkState, MessageRole, Project,
};
use ralphx_lib::domain::repositories::DelegationParkRepository;
use ralphx_lib::http_server::handlers::{park_delegate, wait_delegate};
use ralphx_lib::http_server::types::{DelegateParkRequest, DelegateWaitRequest, HttpServerState};
use ralphx_lib::{error::AppError, error::AppResult};

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a repo-root parent")
        .to_path_buf()
}

fn build_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
    }
}

struct ParentContext {
    project: Project,
    conversation: ChatConversation,
    run: AgentRun,
}

async fn create_parent_context(state: &HttpServerState) -> ParentContext {
    let project = state
        .app_state
        .project_repo
        .create(Project::new(
            "Delegation park HTTP test".to_string(),
            repo_root().display().to_string(),
        ))
        .await
        .expect("create project");
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("create parent conversation");
    let run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .expect("create active parent run");

    ParentContext {
        project,
        conversation,
        run,
    }
}

fn park_headers(parent: &ParentContext) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        parent
            .conversation
            .id
            .as_str()
            .parse()
            .expect("conversation header"),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        parent.run.id.as_str().parse().expect("run header"),
    );
    headers
}

fn park_request(job_ids: Vec<String>) -> DelegateParkRequest {
    DelegateParkRequest {
        job_ids,
        wake_on: None,
        wake_on_failure: None,
        max_wait_secs: Some(60),
    }
}

fn wait_request(job_id: &str) -> DelegateWaitRequest {
    DelegateWaitRequest {
        job_id: Some(job_id.to_string()),
        job_ids: None,
        wait_timeout_ms: None,
        include_delegated_status: Some(false),
        include_child_status: None,
        include_messages: None,
        message_limit: None,
    }
}

/// Registers a running child job and creates its durable delegated conversation/run identity.
async fn seed_running_delegation_job(
    state: &HttpServerState,
    parent: &ParentContext,
    job_id: &str,
) -> (String, ChatConversation, AgentRun) {
    let delegated_session = state
        .app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            parent.project.id.clone(),
            "project".to_string(),
            parent.project.id.as_str().to_string(),
            "ralphx-general-explorer".to_string(),
            AgentHarnessKind::Codex,
        ))
        .await
        .expect("create delegated session");
    let delegated_conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_delegation(
            delegated_session.id.clone(),
        ))
        .await
        .expect("create delegated conversation");
    let delegated_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .expect("create delegated run");

    state
        .delegation_service
        .register_running(
            job_id.to_string(),
            "project".to_string(),
            parent.project.id.as_str().to_string(),
            None,
            None,
            Some(parent.conversation.id.as_str()),
            Some(parent.run.id.as_str()),
            None,
            delegated_session.id.as_str().to_string(),
            Some(delegated_conversation.id.as_str()),
            Some(delegated_run.id.as_str()),
            "ralphx-general-explorer".to_string(),
            None,
            "codex",
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    (job_id.to_string(), delegated_conversation, delegated_run)
}

async fn arm_park(state: &HttpServerState, parent: &ParentContext, job_ids: Vec<String>) -> String {
    park_delegate(
        State(state.clone()),
        park_headers(parent),
        Json(park_request(job_ids)),
    )
    .await
    .expect("park response")
    .0
    .park_id
}

async fn record_handoff(state: &HttpServerState, parent: &ParentContext, content: &str) {
    let mut message = ralphx_lib::domain::entities::ChatMessage::user_in_project(
        parent.project.id.clone(),
        content,
    );
    message.role = MessageRole::Orchestrator;
    message.conversation_id = Some(parent.conversation.id.clone());
    state
        .app_state
        .chat_message_repo
        .create(message)
        .await
        .expect("persist delegated handoff");
}

/// Polls until durable park state is eventually consistent after the spawned wake dispatch.
async fn await_park_state(
    state: &HttpServerState,
    park_id: &DelegationParkId,
    predicate: impl Fn(&DelegationPark) -> bool,
    label: &str,
) -> DelegationPark {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        let park = state
            .app_state
            .delegation_park_repo
            .get(park_id)
            .await
            .expect("read delegation park")
            .expect("delegation park exists");
        if predicate(&park) {
            return park;
        }

        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {label}; last observed park: {park:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn messages_for_parent(
    state: &HttpServerState,
    parent: &ParentContext,
) -> Vec<ralphx_lib::domain::entities::ChatMessage> {
    state
        .app_state
        .chat_message_repo
        .get_by_conversation(&parent.conversation.id)
        .await
        .expect("read parent messages")
}

fn is_hidden_delegation_wake(message: &ralphx_lib::domain::entities::ChatMessage) -> bool {
    message.role == MessageRole::System
        && message
            .metadata
            .as_deref()
            .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
            .is_some_and(|metadata| {
                metadata["source"] == "delegation_park"
                    && metadata["hidden_from_ui"] == true
                    && metadata["recovery_context"] == true
            })
}

async fn await_hidden_wakes_for_parent(
    state: &HttpServerState,
    parent: &ParentContext,
    expected_count: usize,
) -> Vec<ralphx_lib::domain::entities::ChatMessage> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let messages: Vec<_> = messages_for_parent(state, parent)
            .await
            .into_iter()
            .filter(is_hidden_delegation_wake)
            .collect();
        if messages.len() >= expected_count {
            return messages;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {expected_count} hidden parent wakes; observed {}",
            messages.len()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn reconcile_with_timeout(
    state: &HttpServerState,
) -> AppResult<ralphx_lib::application::delegation_park::DelegationParkReconcileSummary> {
    tokio::time::timeout(
        Duration::from_secs(5),
        state
            .app_state
            .build_delegation_park_service()
            .reconcile_all(state.app_state.agent_run_repo.as_ref()),
    )
    .await
    .expect("delegation park reconciliation must not stall")
}

struct NestedReconciliationContext {
    parent: ParentContext,
    parent_park_id: DelegationParkId,
    delegated_parent: ParentContext,
}

async fn seed_nested_reconciliation_context(
    state: &HttpServerState,
    label: &str,
) -> NestedReconciliationContext {
    let parent = create_parent_context(state).await;
    let parent_job_id = format!("{label}-parent-job");
    let (parent_job, delegated_conversation, delegated_run) =
        seed_running_delegation_job(state, &parent, &parent_job_id).await;
    let parent_park_id =
        DelegationParkId::from_string(arm_park(state, &parent, vec![parent_job]).await);
    let delegated_parent = ParentContext {
        project: parent.project.clone(),
        conversation: delegated_conversation,
        run: delegated_run,
    };
    let child_job_id = format!("{label}-child-job");
    let (child_job, _, _) =
        seed_running_delegation_job(state, &delegated_parent, &child_job_id).await;
    arm_park(state, &delegated_parent, vec![child_job]).await;

    NestedReconciliationContext {
        parent,
        parent_park_id,
        delegated_parent,
    }
}

struct FailingArmedParkReadRepository {
    inner: Arc<dyn DelegationParkRepository>,
}

#[async_trait]
impl DelegationParkRepository for FailingArmedParkReadRepository {
    async fn arm(&self, park: DelegationPark) -> AppResult<DelegationPark> {
        self.inner.arm(park).await
    }

    async fn get(&self, id: &DelegationParkId) -> AppResult<Option<DelegationPark>> {
        self.inner.get(id).await
    }

    async fn get_armed_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        Err(AppError::Infrastructure(
            "injected delegation park read failure".to_string(),
        ))
    }

    async fn get_settlement_blocking_for_conversation(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> AppResult<Option<DelegationPark>> {
        Err(AppError::Infrastructure(
            "injected delegation park read failure".to_string(),
        ))
    }

    async fn list_armed(&self) -> AppResult<Vec<DelegationPark>> {
        self.inner.list_armed().await
    }

    async fn list_armed_for_delegated_run(
        &self,
        run_id: &AgentRunId,
    ) -> AppResult<Vec<DelegationPark>> {
        self.inner.list_armed_for_delegated_run(run_id).await
    }

    async fn record_job_settled(
        &self,
        id: &DelegationParkId,
        run_id: &AgentRunId,
        status: &str,
    ) -> AppResult<()> {
        self.inner.record_job_settled(id, run_id, status).await
    }

    async fn claim_wake(&self, id: &DelegationParkId, generation: i64) -> AppResult<bool> {
        self.inner.claim_wake(id, generation).await
    }

    async fn record_wake_failure(&self, id: &DelegationParkId, error: &str) -> AppResult<i32> {
        self.inner.record_wake_failure(id, error).await
    }

    async fn list_wake_stalled(
        &self,
        older_than: chrono::DateTime<Utc>,
    ) -> AppResult<Vec<DelegationPark>> {
        self.inner.list_wake_stalled(older_than).await
    }

    async fn reset_wake_claim(&self, id: &DelegationParkId) -> AppResult<bool> {
        self.inner.reset_wake_claim(id).await
    }

    async fn settle(
        &self,
        id: &DelegationParkId,
        state: DelegationParkState,
        error: Option<&str>,
    ) -> AppResult<()> {
        self.inner.settle(id, state, error).await
    }

    async fn supersede_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<usize> {
        self.inner.supersede_for_conversation(conversation_id).await
    }

    async fn list_expired(&self, now: chrono::DateTime<Utc>) -> AppResult<Vec<DelegationPark>> {
        self.inner.list_expired(now).await
    }
}

#[tokio::test]
async fn park_arms_and_returns_guidance() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, _, _) = seed_running_delegation_job(&state, &parent, "park-guidance").await;

    let response = park_delegate(
        State(state.clone()),
        park_headers(&parent),
        Json(park_request(vec![job_id.clone()])),
    )
    .await
    .expect("park succeeds")
    .0;

    assert!(response.parked);
    assert_eq!(response.watched_jobs.len(), 1);
    assert_eq!(response.watched_jobs[0].job_id, job_id);
    assert!(
        DateTime::parse_from_rfc3339(&response.deadline_at)
            .expect("deadline RFC3339")
            .with_timezone(&Utc)
            > Utc::now()
    );
    assert!(
        response.guidance.to_lowercase().contains("end your turn"),
        "guidance must explicitly permit ending the caller turn: {}",
        response.guidance
    );
    assert!(response.guidance.contains("when a delegate fails"));
    assert!(
        state
            .app_state
            .delegation_park_repo
            .get_armed_for_conversation(&parent.conversation.id)
            .await
            .expect("load armed park")
            .is_some(),
        "successful handler call must persist an armed park"
    );

    let mut no_failure_wake = park_request(vec![job_id]);
    no_failure_wake.wake_on_failure = Some(false);
    let response = park_delegate(
        State(state.clone()),
        park_headers(&parent),
        Json(no_failure_wake),
    )
    .await
    .expect("park without failure wake succeeds")
    .0;
    assert!(!response.guidance.contains("when a delegate fails"));
}

#[tokio::test]
async fn park_rejects_missing_run_identity() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;

    let missing_run = park_delegate(
        State(state.clone()),
        HeaderMap::new(),
        Json(park_request(vec!["job".to_string()])),
    )
    .await
    .expect_err("missing run identity must fail");
    assert_eq!(missing_run.0, axum::http::StatusCode::BAD_REQUEST);

    let mut missing_conversation_headers = HeaderMap::new();
    missing_conversation_headers.insert(
        "x-ralphx-agent-run-id",
        parent.run.id.as_str().parse().expect("run header"),
    );
    let missing_conversation = park_delegate(
        State(state),
        missing_conversation_headers,
        Json(park_request(vec!["job".to_string()])),
    )
    .await
    .expect_err("missing conversation identity must fail");
    assert_eq!(missing_conversation.0, axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn park_rejects_a_stale_caller_run() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    state
        .app_state
        .agent_run_repo
        .complete(&parent.run.id)
        .await
        .expect("complete stale run");
    let active_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(parent.conversation.id))
        .await
        .expect("create current run");
    assert_ne!(active_run.id, parent.run.id);

    let error = park_delegate(
        State(state),
        park_headers(&parent),
        Json(park_request(vec!["job".to_string()])),
    )
    .await
    .expect_err("stale caller run must fail");
    assert_eq!(error.0, axum::http::StatusCode::CONFLICT);
}

#[tokio::test]
async fn park_rejects_a_job_owned_by_another_conversation() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let other_parent = create_parent_context(&state).await;
    let (other_job, _, _) = seed_running_delegation_job(&state, &other_parent, "foreign-job").await;

    let error = park_delegate(
        State(state.clone()),
        park_headers(&parent),
        Json(park_request(vec![other_job])),
    )
    .await
    .expect_err("foreign job must fail");
    assert!(error.0.is_client_error());
    assert!(
        state
            .app_state
            .delegation_park_repo
            .get_armed_for_conversation(&parent.conversation.id)
            .await
            .expect("read parks")
            .is_none(),
        "rejected foreign ownership must not arm a park"
    );
}

#[tokio::test]
async fn settlement_claims_the_parked_wake_exactly_once() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, _, delegated_run) =
        seed_running_delegation_job(&state, &parent, "wake-once").await;
    let park_id =
        DelegationParkId::from_string(arm_park(&state, &parent, vec![job_id.clone()]).await);

    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("mark delegate terminal");
    let settled = wait_delegate(State(state.clone()), Json(wait_request(&job_id)))
        .await
        .expect("production settlement")
        .0;
    assert_eq!(settled.status, "completed");

    let claimed_park = await_park_state(
        &state,
        &park_id,
        |park| {
            park.state != DelegationParkState::Armed
                && park.jobs.iter().any(|job| {
                    job.job_id == job_id && job.settled_status.as_deref() == Some("completed")
                })
        },
        "settlement to record completion and claim the parked wake",
    )
    .await;
    assert_ne!(claimed_park.state, DelegationParkState::Armed);
    assert_eq!(
        claimed_park
            .jobs
            .iter()
            .filter(|job| job.settled_status.is_some())
            .count(),
        1,
        "settlement must durably record exactly the watched job"
    );

    let replay = wait_delegate(State(state.clone()), Json(wait_request(&job_id)))
        .await
        .expect("idempotent settlement read")
        .0;
    assert_eq!(replay.status, "completed");
    let replayed_park = state
        .app_state
        .delegation_park_repo
        .get(&park_id)
        .await
        .expect("read replayed park")
        .expect("park exists");
    assert_eq!(
        replayed_park.state, claimed_park.state,
        "replaying an already-settled job must not claim the park a second time"
    );
    assert_eq!(
        replayed_park
            .jobs
            .iter()
            .filter(|job| job.settled_status.is_some())
            .count(),
        1,
        "replaying an already-settled job must not record another settlement"
    );
}

#[tokio::test]
async fn wake_is_not_dispatched_when_commit_terminal_is_rejected() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, _, _) = seed_running_delegation_job(&state, &parent, "rejected-terminal").await;
    let park_id = arm_park(&state, &parent, vec![job_id.clone()]).await;

    let mut candidate = state
        .delegation_service
        .terminal_candidate(&job_id, "completed", Some("output".to_string()), None)
        .await
        .expect("terminal candidate");
    candidate.delegated_agent_run_id = Some(AgentRunId::new().as_str());
    assert!(
        !state.delegation_service.commit_terminal(candidate).await,
        "mismatched candidate must lose the terminal CAS"
    );
    assert!(messages_for_parent(&state, &parent).await.is_empty());
    let park = state
        .app_state
        .delegation_park_repo
        .get(&ralphx_lib::domain::entities::DelegationParkId::from_string(park_id))
        .await
        .expect("read park")
        .expect("park exists");
    assert_eq!(park.state, DelegationParkState::Armed);
}

#[tokio::test]
async fn any_settled_policy_wakes_before_all_jobs_finish() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (first, _, first_run) = seed_running_delegation_job(&state, &parent, "any-first").await;
    let (second, _, second_run) = seed_running_delegation_job(&state, &parent, "any-second").await;
    let mut request = park_request(vec![first.clone(), second.clone()]);
    request.wake_on = Some("any".to_string());
    let park_id = DelegationParkId::from_string(
        park_delegate(State(state.clone()), park_headers(&parent), Json(request))
            .await
            .expect("park any policy")
            .0
            .park_id,
    );

    state
        .app_state
        .agent_run_repo
        .complete(&first_run.id)
        .await
        .expect("complete first delegate");
    let _ = wait_delegate(State(state.clone()), Json(wait_request(&first)))
        .await
        .expect("settle first delegate");
    let claimed_park = await_park_state(
        &state,
        &park_id,
        |park| park.state != DelegationParkState::Armed,
        "any-settled policy to claim the parked wake",
    )
    .await;
    assert_eq!(
        claimed_park
            .jobs
            .iter()
            .find(|job| job.job_id == first)
            .expect("first job is parked")
            .settled_status
            .as_deref(),
        Some("completed")
    );
    assert!(
        claimed_park
            .jobs
            .iter()
            .find(|job| job.job_id == second)
            .expect("second job is parked")
            .settled_status
            .is_none(),
        "any-settled wake must not wait for or settle the sibling"
    );
    assert_eq!(
        state
            .app_state
            .agent_run_repo
            .get_by_id(&second_run.id)
            .await
            .expect("read sibling run")
            .expect("sibling run exists")
            .status,
        AgentRunStatus::Running,
        "any-settled wake must not require or settle the sibling"
    );
}

#[tokio::test]
async fn failed_delegate_wakes_when_failure_policy_enabled() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (failed_job, _, failed_run) =
        seed_running_delegation_job(&state, &parent, "failed-first").await;
    let (other_job, _, other_run) =
        seed_running_delegation_job(&state, &parent, "failed-second").await;
    let mut request = park_request(vec![failed_job.clone(), other_job.clone()]);
    request.wake_on = Some("all".to_string());
    request.wake_on_failure = Some(true);
    let park_id = DelegationParkId::from_string(
        park_delegate(State(state.clone()), park_headers(&parent), Json(request))
            .await
            .expect("park failure policy")
            .0
            .park_id,
    );

    state
        .app_state
        .agent_run_repo
        .fail(&failed_run.id, "delegate failed")
        .await
        .expect("fail delegate");
    let settled = wait_delegate(State(state.clone()), Json(wait_request(&failed_job)))
        .await
        .expect("settle failed delegate")
        .0;
    assert_eq!(settled.status, "failed");
    let claimed_park = await_park_state(
        &state,
        &park_id,
        |park| park.state != DelegationParkState::Armed,
        "failure policy to claim the parked wake",
    )
    .await;
    assert_eq!(
        claimed_park
            .jobs
            .iter()
            .find(|job| job.job_id == failed_job)
            .expect("failed job is parked")
            .settled_status
            .as_deref(),
        Some("failed")
    );
    assert!(
        claimed_park
            .jobs
            .iter()
            .find(|job| job.job_id == other_job)
            .expect("sibling job is parked")
            .settled_status
            .is_none(),
        "failure policy must wake before all jobs settle"
    );
    assert_eq!(
        state
            .app_state
            .agent_run_repo
            .get_by_id(&other_run.id)
            .await
            .expect("read sibling run")
            .expect("sibling run exists")
            .status,
        AgentRunStatus::Running,
        "failure wake must happen before all-settled when configured"
    );
}

#[tokio::test]
async fn user_message_supersedes_park_and_blocks_a_later_wake() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, _, delegated_run) =
        seed_running_delegation_job(&state, &parent, "superseded-park").await;
    let park_id = arm_park(&state, &parent, vec![job_id.clone()]).await;
    assert_eq!(
        state
            .app_state
            .delegation_park_repo
            .supersede_for_conversation(&parent.conversation.id)
            .await
            .expect("supersede park"),
        1
    );

    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("complete delegate");
    let _ = wait_delegate(State(state.clone()), Json(wait_request(&job_id)))
        .await
        .expect("settle delegate after supersession");
    let park = state
        .app_state
        .delegation_park_repo
        .get(&ralphx_lib::domain::entities::DelegationParkId::from_string(park_id))
        .await
        .expect("read superseded park")
        .expect("park exists");
    assert_eq!(park.state, DelegationParkState::Superseded);
    assert!(
        messages_for_parent(&state, &parent).await.is_empty(),
        "a superseded park must never inject a later wake"
    );
}

#[tokio::test]
async fn parked_delegate_does_not_settle_its_parents_job() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (parent_job, delegated_conversation, delegated_run) =
        seed_running_delegation_job(&state, &parent, "nested-parent-job").await;
    let delegated_parent = ParentContext {
        project: parent.project.clone(),
        conversation: delegated_conversation,
        run: delegated_run.clone(),
    };
    let (child_job, _, _) =
        seed_running_delegation_job(&state, &delegated_parent, "nested-child-job").await;
    arm_park(&state, &delegated_parent, vec![child_job]).await;

    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("terminal delegated coordinator run");
    let observed = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("production nested settlement check")
        .0;
    assert_eq!(
        observed.status, "running",
        "a parked delegated coordinator must remain running to its parent"
    );
    assert_eq!(
        state
            .delegation_service
            .snapshot(&parent_job)
            .await
            .expect("parent job exists")
            .status,
        "running",
        "park gate must suppress the forbidden parent terminal effect"
    );
}

#[tokio::test]
async fn parked_delegate_settles_parent_job_once_its_park_is_gone() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (parent_job, delegated_conversation, delegated_run) =
        seed_running_delegation_job(&state, &parent, "resumed-parent-job").await;
    let delegated_parent = ParentContext {
        project: parent.project.clone(),
        conversation: delegated_conversation,
        run: delegated_run.clone(),
    };
    let (child_job, _, _) =
        seed_running_delegation_job(&state, &delegated_parent, "resumed-child-job").await;
    arm_park(&state, &delegated_parent, vec![child_job]).await;

    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("terminal delegated coordinator run");
    let parked = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("parked nested settlement check")
        .0;
    assert_eq!(parked.status, "running");
    record_handoff(&state, &delegated_parent, "stale launch result").await;
    tokio::time::sleep(Duration::from_millis(1)).await;
    let resumed_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new_continuation(
            delegated_parent.conversation.id,
            delegated_run
                .run_chain_id
                .clone()
                .expect("launch run chain id"),
            delegated_run.id.as_str(),
        ))
        .await
        .expect("create resumed delegated run");
    record_handoff(&state, &delegated_parent, "current resumed result").await;
    state
        .app_state
        .agent_run_repo
        .complete(&resumed_run.id)
        .await
        .expect("complete resumed delegated run");
    state
        .app_state
        .delegation_park_repo
        .supersede_for_conversation(&delegated_parent.conversation.id)
        .await
        .expect("remove nested park");

    let settled = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("settlement after nested park release")
        .0;
    assert_eq!(settled.status, "completed");
    assert_eq!(settled.content.as_deref(), Some("current resumed result"));
    assert_eq!(
        state
            .delegation_service
            .snapshot(&parent_job)
            .await
            .expect("parent job exists")
            .status,
        "completed",
        "once the nested park is gone, the original parent job settles normally"
    );
}

async fn seed_nested_wait_case(
    state: &HttpServerState,
    label: &str,
) -> (String, ParentContext, AgentRun, DelegationParkId) {
    let parent = create_parent_context(state).await;
    let (parent_job, delegated_conversation, delegated_run) =
        seed_running_delegation_job(state, &parent, &format!("{label}-parent")).await;
    let delegated_parent = ParentContext {
        project: parent.project,
        conversation: delegated_conversation,
        run: delegated_run.clone(),
    };
    let (child_job, _, _) =
        seed_running_delegation_job(state, &delegated_parent, &format!("{label}-child")).await;
    let park_id =
        DelegationParkId::from_string(arm_park(state, &delegated_parent, vec![child_job]).await);
    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("complete delegated coordinator launch run");
    (parent_job, delegated_parent, delegated_run, park_id)
}

#[tokio::test]
async fn parked_delegate_does_not_settle_parent_job_during_wake_dispatch() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let (parent_job, _, _, park_id) = seed_nested_wait_case(&state, "nested-waking").await;
    let park = state
        .app_state
        .delegation_park_repo
        .get(&park_id)
        .await
        .expect("read nested park")
        .expect("nested park exists");
    assert!(state
        .app_state
        .delegation_park_repo
        .claim_wake(&park_id, park.generation)
        .await
        .expect("claim nested wake"));

    let observed = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("settlement check during wake dispatch")
        .0;
    assert_eq!(observed.status, "running");
}

#[tokio::test]
async fn parked_delegate_does_not_settle_parent_job_after_wake_before_resume() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let (parent_job, _, _, park_id) = seed_nested_wait_case(&state, "nested-woken").await;
    let park = state
        .app_state
        .delegation_park_repo
        .get(&park_id)
        .await
        .expect("read nested park")
        .expect("nested park exists");
    assert!(state
        .app_state
        .delegation_park_repo
        .claim_wake(&park_id, park.generation)
        .await
        .expect("claim nested wake"));
    state
        .app_state
        .delegation_park_repo
        .settle(&park_id, DelegationParkState::Woken, None)
        .await
        .expect("record nested wake delivery");

    let observed = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("settlement check before resumed run")
        .0;
    assert_eq!(observed.status, "running");
}

#[tokio::test]
async fn parked_delegate_settles_parent_job_on_the_resumed_run() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let (parent_job, delegated_parent, launch_run, park_id) =
        seed_nested_wait_case(&state, "nested-resumed").await;
    record_handoff(&state, &delegated_parent, "stale launch result").await;
    let park = state
        .app_state
        .delegation_park_repo
        .get(&park_id)
        .await
        .expect("read nested park")
        .expect("nested park exists");
    assert!(state
        .app_state
        .delegation_park_repo
        .claim_wake(&park_id, park.generation)
        .await
        .expect("claim nested wake"));
    state
        .app_state
        .delegation_park_repo
        .settle(&park_id, DelegationParkState::Woken, None)
        .await
        .expect("record nested wake delivery");
    tokio::time::sleep(Duration::from_millis(1)).await;
    let resumed_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new_continuation(
            delegated_parent.conversation.id,
            launch_run
                .run_chain_id
                .clone()
                .expect("launch run chain id"),
            launch_run.id.as_str(),
        ))
        .await
        .expect("create resumed delegated run");
    record_handoff(&state, &delegated_parent, "current resumed result").await;
    state
        .app_state
        .agent_run_repo
        .complete(&resumed_run.id)
        .await
        .expect("complete resumed delegated run");

    let settled = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("settlement from resumed run")
        .0;
    assert_eq!(settled.status, "completed");
    assert_eq!(settled.content.as_deref(), Some("current resumed result"));
}

#[tokio::test]
async fn failed_park_unblocks_parent_settlement() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let (parent_job, _, _, park_id) = seed_nested_wait_case(&state, "nested-failed").await;
    state
        .app_state
        .delegation_park_repo
        .settle(
            &park_id,
            DelegationParkState::Failed,
            Some("wake delivery failed"),
        )
        .await
        .expect("fail nested park");

    let settled = wait_delegate(State(state.clone()), Json(wait_request(&parent_job)))
        .await
        .expect("settlement after failed park")
        .0;
    assert_eq!(settled.status, "completed");
}

#[tokio::test]
async fn park_repo_read_error_keeps_parent_settlement_pending() {
    let mut app_state = AppState::new_sqlite_test();
    let durable_repo = Arc::clone(&app_state.delegation_park_repo);
    app_state.delegation_park_repo = Arc::new(FailingArmedParkReadRepository {
        inner: durable_repo,
    });
    let state = build_state(Arc::new(app_state));
    let parent = create_parent_context(&state).await;
    let (job_id, _, delegated_run) =
        seed_running_delegation_job(&state, &parent, "fail-closed-read").await;
    arm_park(&state, &parent, vec![job_id.clone()]).await;

    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("terminal delegated run");
    let error = wait_delegate(State(state.clone()), Json(wait_request(&job_id)))
        .await
        .expect_err("park repository read failure must propagate as pending settlement");
    assert_eq!(error.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        state
            .delegation_service
            .snapshot(&job_id)
            .await
            .expect("job remains registered")
            .status,
        "running",
        "an unreadable nested-park state must never be treated as no park"
    );
    assert!(
        messages_for_parent(&state, &parent).await.is_empty(),
        "failed authority reads must not dispatch a wake"
    );
}

#[tokio::test]
async fn reconciliation_does_not_settle_a_parked_delegates_job() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let nested = seed_nested_reconciliation_context(&state, "reconcile-nested").await;
    state
        .app_state
        .agent_run_repo
        .complete(&nested.delegated_parent.run.id)
        .await
        .expect("complete parked delegate launch run");

    let summary = reconcile_with_timeout(&state)
        .await
        .expect("reconcile nested park");
    let parent_park = state
        .app_state
        .delegation_park_repo
        .get(&nested.parent_park_id)
        .await
        .expect("read grandparent park")
        .expect("grandparent park exists");

    assert_eq!(summary.jobs_settled, 0);
    assert_eq!(parent_park.state, DelegationParkState::Armed);
    assert!(parent_park.jobs[0].settled_status.is_none());
    assert!(
        messages_for_parent(&state, &nested.parent).await.is_empty(),
        "reconciliation must not wake the grandparent while its delegate is parked"
    );
}

#[tokio::test]
async fn reconciliation_settles_a_parked_delegate_once_its_park_clears() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let nested = seed_nested_reconciliation_context(&state, "reconcile-cleared").await;
    state
        .app_state
        .agent_run_repo
        .complete(&nested.delegated_parent.run.id)
        .await
        .expect("complete parked delegate launch run");

    let first_summary = reconcile_with_timeout(&state)
        .await
        .expect("reconcile while nested park is armed");
    assert_eq!(first_summary.jobs_settled, 0);
    assert_eq!(
        state
            .app_state
            .delegation_park_repo
            .supersede_for_conversation(&nested.delegated_parent.conversation.id)
            .await
            .expect("supersede nested delegate park"),
        1
    );

    let reconciliation_state = state.clone();
    let reconciliation = tokio::spawn(async move {
        reconciliation_state
            .app_state
            .build_delegation_park_service()
            .reconcile_all(reconciliation_state.app_state.agent_run_repo.as_ref())
            .await
    });
    let parent_park = await_park_state(
        &state,
        &nested.parent_park_id,
        |park| {
            park.state != DelegationParkState::Armed
                && park.jobs[0].settled_status.as_deref() == Some("completed")
        },
        "reconciliation after the nested park clears",
    )
    .await;
    let hidden_wake_messages = await_hidden_wakes_for_parent(&state, &nested.parent, 1).await;
    reconciliation.abort();
    let _ = reconciliation.await;

    assert_ne!(parent_park.state, DelegationParkState::Armed);
    assert_eq!(
        parent_park.jobs[0].settled_status.as_deref(),
        Some("completed")
    );
    assert_eq!(
        hidden_wake_messages.len(),
        1,
        "the grandparent must receive exactly one hidden wake"
    );
    let metadata: serde_json::Value = serde_json::from_str(
        hidden_wake_messages[0]
            .metadata
            .as_deref()
            .expect("hidden wake metadata"),
    )
    .expect("valid wake metadata");
    assert_eq!(metadata["hidden_from_ui"], true);
    assert_eq!(metadata["recovery_context"], true);
    assert_eq!(metadata["source"], "delegation_park");
}

#[tokio::test]
async fn reconciliation_uses_the_delegates_current_run() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, delegated_conversation, launch_run) =
        seed_running_delegation_job(&state, &parent, "reconcile-current-run").await;
    let park_id = DelegationParkId::from_string(arm_park(&state, &parent, vec![job_id]).await);
    state
        .app_state
        .agent_run_repo
        .complete(&launch_run.id)
        .await
        .expect("complete delegate launch run");
    tokio::time::sleep(Duration::from_millis(1)).await;
    let current_run = state
        .app_state
        .agent_run_repo
        .create(AgentRun::new(delegated_conversation.id))
        .await
        .expect("create resumed delegate run");

    let summary = reconcile_with_timeout(&state)
        .await
        .expect("reconcile current delegate run");
    let park = state
        .app_state
        .delegation_park_repo
        .get(&park_id)
        .await
        .expect("read parent park")
        .expect("parent park exists");

    assert_eq!(current_run.status, AgentRunStatus::Running);
    assert_eq!(summary.jobs_settled, 0);
    assert_eq!(park.state, DelegationParkState::Armed);
    assert!(park.jobs[0].settled_status.is_none());
    assert!(messages_for_parent(&state, &parent).await.is_empty());
}

#[tokio::test]
async fn reconciliation_fails_closed_on_park_read_error() {
    let mut app_state = AppState::new_sqlite_test();
    let durable_repo = Arc::clone(&app_state.delegation_park_repo);
    app_state.delegation_park_repo = Arc::new(FailingArmedParkReadRepository {
        inner: Arc::clone(&durable_repo),
    });
    let state = build_state(Arc::new(app_state));
    let parent = create_parent_context(&state).await;
    let (job_id, _, delegated_run) =
        seed_running_delegation_job(&state, &parent, "reconcile-fail-closed").await;
    let park_id = DelegationParkId::from_string(arm_park(&state, &parent, vec![job_id]).await);
    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("complete delegate before failed reconciliation read");

    let error = reconcile_with_timeout(&state)
        .await
        .expect_err("park read failure must abort reconciliation");
    let park = durable_repo
        .get(&park_id)
        .await
        .expect("read durable parent park")
        .expect("durable parent park exists");

    assert!(matches!(error, AppError::Infrastructure(_)));
    assert_eq!(park.state, DelegationParkState::Armed);
    assert!(park.jobs[0].settled_status.is_none());
    assert!(messages_for_parent(&state, &parent).await.is_empty());
}

#[tokio::test]
async fn startup_reconciliation_claims_a_park_whose_delegate_settled_while_down() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, _, delegated_run) =
        seed_running_delegation_job(&state, &parent, "reconcile-wake").await;
    let park_id = DelegationParkId::from_string(arm_park(&state, &parent, vec![job_id]).await);

    state
        .app_state
        .agent_run_repo
        .complete(&delegated_run.id)
        .await
        .expect("persist terminal run without live settlement");
    let reconciliation_state = state.clone();
    let reconciliation = tokio::spawn(async move {
        reconciliation_state
            .app_state
            .build_delegation_park_service()
            .reconcile_all(reconciliation_state.app_state.agent_run_repo.as_ref())
            .await
    });
    let claimed_park = await_park_state(
        &state,
        &park_id,
        |park| {
            park.state != DelegationParkState::Armed
                && park.jobs.iter().any(|job| {
                    job.delegated_agent_run_id == delegated_run.id
                        && job.settled_status.as_deref() == Some("completed")
                })
        },
        "startup reconciliation to record completion and claim the parked wake",
    )
    .await;
    reconciliation.abort();
    let _ = reconciliation.await;
    assert_ne!(claimed_park.state, DelegationParkState::Armed);
    assert_eq!(
        claimed_park
            .jobs
            .iter()
            .find(|job| job.delegated_agent_run_id == delegated_run.id)
            .expect("delegated run is parked")
            .settled_status
            .as_deref(),
        Some("completed")
    );
}

#[tokio::test]
async fn startup_reconciliation_claims_an_expired_park_without_settled_jobs() {
    let state = build_state(Arc::new(AppState::new_sqlite_test()));
    let parent = create_parent_context(&state).await;
    let (job_id, _, _) = seed_running_delegation_job(&state, &parent, "expired-park").await;
    let mut request = park_request(vec![job_id]);
    request.max_wait_secs = Some(0);
    let park_id = park_delegate(State(state.clone()), park_headers(&parent), Json(request))
        .await
        .expect("arm immediately expired park")
        .0
        .park_id;

    let park_id = DelegationParkId::from_string(park_id);
    let reconciliation_state = state.clone();
    let reconciliation = tokio::spawn(async move {
        reconciliation_state
            .app_state
            .build_delegation_park_service()
            .reconcile_all(reconciliation_state.app_state.agent_run_repo.as_ref())
            .await
    });
    let claimed_park = await_park_state(
        &state,
        &park_id,
        |park| park.state != DelegationParkState::Armed,
        "deadline reconciliation to claim the expired parked wake",
    )
    .await;
    reconciliation.abort();
    let _ = reconciliation.await;
    assert_ne!(claimed_park.state, DelegationParkState::Armed);
    assert!(
        claimed_park
            .jobs
            .iter()
            .all(|job| job.settled_status.is_none()),
        "deadline reconciliation must force a wake even though no job settled"
    );
}
