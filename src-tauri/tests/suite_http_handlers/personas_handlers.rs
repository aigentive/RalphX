use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};
use ralphx_events::RecordingEventSink;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, PersonaStatus, ProjectId,
};
use ralphx_lib::http_server::delegation::DelegationService;
use ralphx_lib::http_server::handlers::automations::CALLER_SESSION_ID_HEADER;
use ralphx_lib::http_server::handlers::{
    get_persona_draft, save_persona_draft, SavePersonaDraftRequest,
};
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::agents::claude::{
    reset_agent_personas_override_for_test, set_agent_personas_override,
};
use ralphx_lib::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use ralphx_lib::testing::SqliteTestDb;

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Handler test persona\n---\n{body}")
}

struct PersonaFlagGuard {
    _lock: tokio::sync::MutexGuard<'static, ()>,
}

impl Drop for PersonaFlagGuard {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
    }
}

fn persona_flag_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn enable_personas(enabled: bool) -> PersonaFlagGuard {
    let lock = persona_flag_lock().lock().await;
    set_agent_personas_override(Some(enabled));
    PersonaFlagGuard { _lock: lock }
}

fn setup_state(event_sink: Option<RecordingEventSink>) -> HttpServerState {
    let mut app_state = AppState::new_sqlite_test();
    let db = SqliteTestDb::new("persona_handler_binding");
    let shared = db.shared_conn();
    app_state.db = DbConnection::from_shared(Arc::clone(&shared));
    app_state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    app_state.chat_conversation_repo =
        Arc::new(SqliteChatConversationRepository::from_shared(shared));
    if let Some(event_sink) = event_sink {
        app_state.events = Arc::new(event_sink);
    }
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state: Arc::new(app_state),
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Arc::new(DelegationService::new()),
    }
}

fn request(slug: &str, content: String) -> SavePersonaDraftRequest {
    SavePersonaDraftRequest {
        draft_id: None,
        slug: slug.to_string(),
        content,
        source_session_id: None,
    }
}

async fn persona_builder_headers(state: &HttpServerState) -> (ChatConversation, HeaderMap) {
    let mut conversation = ChatConversation::new_project(ProjectId::from_string(
        "persona-save-builder-project".to_string(),
    ));
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        CALLER_SESSION_ID_HEADER,
        HeaderValue::from_str(&conversation.id.as_str()).unwrap(),
    );
    (conversation, headers)
}

#[tokio::test]
async fn builder_save_creates_and_binds_then_redirects_omitted_draft_id() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (conversation, headers) = persona_builder_headers(&state).await;
    let created = save_persona_draft(
        State(state.clone()),
        headers.clone(),
        Json(request(
            "bound-builder",
            persona_content("bound-builder", "Before"),
        )),
    )
    .await
    .expect("first builder save should create and bind")
    .0;
    let stored = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.builder_draft_id.as_deref(),
        Some(created.id.as_str())
    );

    let updated = save_persona_draft(
        State(state),
        headers,
        Json(request(
            "bound-builder",
            persona_content("bound-builder", "After"),
        )),
    )
    .await
    .expect("the conversation binding should redirect an omitted draft id")
    .0;
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.version, 2);
}

#[tokio::test]
async fn builder_save_rejects_a_draft_id_outside_its_binding() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (_conversation, headers) = persona_builder_headers(&state).await;
    let bound = save_persona_draft(
        State(state.clone()),
        headers.clone(),
        Json(request(
            "bound-owner",
            persona_content("bound-owner", "Bound"),
        )),
    )
    .await
    .unwrap()
    .0;
    let other = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(request(
            "other-draft",
            persona_content("other-draft", "Other"),
        )),
    )
    .await
    .unwrap()
    .0;

    let error = save_persona_draft(
        State(state.clone()),
        headers,
        Json(SavePersonaDraftRequest {
            draft_id: Some(other.id.as_str().to_string()),
            slug: "other-draft".to_string(),
            content: persona_content("other-draft", "Hijacked"),
            source_session_id: None,
        }),
    )
    .await
    .expect_err("a builder conversation must not write another draft");
    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        state
            .app_state
            .persona_repo
            .get_by_id(&bound.id)
            .await
            .unwrap()
            .unwrap()
            .version,
        1
    );
    assert_eq!(
        state
            .app_state
            .persona_repo
            .get_by_id(&other.id)
            .await
            .unwrap()
            .unwrap()
            .version,
        1
    );
}

#[tokio::test]
async fn save_draft_handler_rejects_when_flag_off() {
    let _persona_flag = enable_personas(false).await;
    let state = setup_state(None);

    let error = save_persona_draft(
        State(state),
        HeaderMap::new(),
        Json(request(
            "disabled-handler",
            persona_content("disabled-handler", "Body"),
        )),
    )
    .await
    .expect_err("disabled handler must reject before persistence");

    assert_eq!(error.status, StatusCode::FORBIDDEN);
    let payload: serde_json::Value = serde_json::from_str(
        error
            .message
            .as_deref()
            .expect("typed error payload is required"),
    )
    .expect("error payload should be JSON");
    assert_eq!(payload["code"], "persona_feature_disabled");
}

#[tokio::test]
async fn get_persona_draft_handler_round_trips() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let draft = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(request("round-trip", persona_content("round-trip", "Body"))),
    )
    .await
    .expect("draft should save")
    .0;

    let loaded = get_persona_draft(State(state), Path(draft.id.as_str().to_string()))
        .await
        .expect("draft should load")
        .0;

    assert_eq!(loaded.id, draft.id);
    assert_eq!(loaded.source_session_id, None);
}

#[tokio::test]
async fn save_draft_handler_updates_draft_and_bumps_version() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let draft = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(request(
            "handler-update",
            persona_content("handler-update", "Before"),
        )),
    )
    .await
    .expect("draft should save")
    .0;

    let updated = save_persona_draft(
        State(state),
        HeaderMap::new(),
        Json(SavePersonaDraftRequest {
            draft_id: Some(draft.id.as_str().to_string()),
            slug: "handler-update".to_string(),
            content: persona_content("handler-update", "After"),
            source_session_id: Some("ignored-on-update".to_string()),
        }),
    )
    .await
    .expect("draft should update")
    .0;

    assert_eq!(updated.version, 2);
    assert_eq!(updated.content, persona_content("handler-update", "After"));
    assert_eq!(updated.source_session_id, None);
}

#[tokio::test]
async fn save_draft_handler_allows_archived_slug_reuse_but_not_live_collision() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let first = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(request(
            "handler-slug",
            persona_content("handler-slug", "First"),
        )),
    )
    .await
    .expect("first draft should save")
    .0;
    let live_collision = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(request(
            "handler-slug",
            persona_content("handler-slug", "Second"),
        )),
    )
    .await
    .expect_err("live draft slug should collide");
    assert_eq!(live_collision.status, StatusCode::UNPROCESSABLE_ENTITY);

    state
        .app_state
        .persona_repo
        .set_status(&first.id, PersonaStatus::Archived)
        .await
        .expect("test fixture should archive the row");
    let reused = save_persona_draft(
        State(state),
        HeaderMap::new(),
        Json(request(
            "handler-slug",
            persona_content("handler-slug", "Reused"),
        )),
    )
    .await
    .expect("archived slug should be reusable")
    .0;
    assert_ne!(reused.id, first.id);
}

#[tokio::test]
async fn save_draft_handler_cannot_mutate_active_persona() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let draft = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(request(
            "handler-active",
            persona_content("handler-active", "Before"),
        )),
    )
    .await
    .expect("draft should save")
    .0;
    state
        .app_state
        .persona_repo
        .set_status(&draft.id, PersonaStatus::Active)
        .await
        .expect("draft should become active");

    let error = save_persona_draft(
        State(state),
        HeaderMap::new(),
        Json(SavePersonaDraftRequest {
            draft_id: Some(draft.id.as_str().to_string()),
            slug: "handler-active".to_string(),
            content: persona_content("handler-active", "After"),
            source_session_id: None,
        }),
    )
    .await
    .expect_err("handler must not mutate an active persona");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn save_draft_handler_emits_body_free_event_payload() {
    let _persona_flag = enable_personas(true).await;
    let event_sink = RecordingEventSink::new();
    let state = setup_state(Some(event_sink.clone()));

    let draft = save_persona_draft(
        State(state),
        HeaderMap::new(),
        Json(request(
            "event-persona",
            persona_content("event-persona", "Secret body"),
        )),
    )
    .await
    .expect("draft should save")
    .0;

    let events = event_sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "persona:draft_updated");
    let payload = events[0]
        .payload
        .as_object()
        .expect("payload should be object");
    assert_eq!(payload.len(), 3);
    assert_eq!(
        payload.get("draft_id").and_then(serde_json::Value::as_str),
        Some(draft.id.as_str())
    );
    assert_eq!(
        payload.get("version").and_then(serde_json::Value::as_i64),
        Some(1)
    );
    assert_eq!(
        payload
            .get("content_hash")
            .and_then(serde_json::Value::as_str),
        Some(draft.content_hash.as_str())
    );
    assert!(!payload.contains_key("content"));
    assert!(!payload.contains_key("body"));
}
