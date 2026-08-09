use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};
use ralphx_events::RecordingEventSink;
use ralphx_lib::application::personas::{PersonaService, SavePersonaDraftInput};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, IdeationSessionId, PersonaScopeFilter,
    PersonaStatus, ProjectId,
};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::delegation::DelegationService;
use ralphx_lib::http_server::handlers::automations::CALLER_SESSION_ID_HEADER;
use ralphx_lib::http_server::handlers::{
    get_persona_draft, save_persona_draft, SavePersonaDraftRequest,
};
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::agents::{
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
    HttpServerState {
        app_state: Arc::new(app_state),
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Arc::new(DelegationService::new()),
        external_mcp_supervisor: None,
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

fn persona_service(state: &HttpServerState) -> PersonaService {
    PersonaService::new(
        state.app_state.db.clone(),
        Arc::clone(&state.app_state.persona_repo),
        Arc::clone(&state.app_state.chat_conversation_repo),
    )
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
    let headers = conversation_headers(&conversation);
    (conversation, headers)
}

fn conversation_headers(conversation: &ChatConversation) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CALLER_SESSION_ID_HEADER,
        HeaderValue::from_str(&conversation.id.as_str()).unwrap(),
    );
    headers
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
    assert_eq!(
        created.project_id.as_ref().map(ProjectId::as_str),
        Some("persona-save-builder-project")
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
async fn first_bound_draft_claim_conflict_rolls_back_persona_and_artifact() {
    let state = setup_state(None);
    let service = persona_service(&state);
    let (conversation, _) = persona_builder_headers(&state).await;

    let winner = service
        .create_bound_draft(
            true,
            &conversation.id,
            SavePersonaDraftInput {
                project_id: Some(ProjectId::from_string(conversation.context_id.clone())),
                slug: "claim-winner".to_string(),
                content: persona_content("claim-winner", "Winner"),
                source_session_id: Some(conversation.id.as_str()),
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("the first authority should claim the conversation");

    let stale_error = service
        .create_bound_draft(
            true,
            &conversation.id,
            SavePersonaDraftInput {
                project_id: Some(ProjectId::from_string(conversation.context_id.clone())),
                slug: "claim-loser".to_string(),
                content: persona_content("claim-loser", "Loser"),
                source_session_id: Some(conversation.id.as_str()),
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect_err("stale first-save authority must not replace the winner");
    assert!(matches!(stale_error, AppError::Conflict(_)));

    let mut finished_conversation = ChatConversation::new_standalone();
    finished_conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let finished_conversation = state
        .app_state
        .chat_conversation_repo
        .create(finished_conversation)
        .await
        .expect("finished conversation fixture should persist");
    let winner_id = winner.id.as_str().to_string();
    let finished_id = finished_conversation.id.as_str().to_string();
    state
        .app_state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE chat_conversations SET builder_result_persona_id = ?1 WHERE id = ?2",
                rusqlite::params![winner_id, finished_id],
            )?;
            Ok(())
        })
        .await
        .expect("finished binding fixture should persist");
    let finished_error = service
        .create_bound_draft(
            true,
            &finished_conversation.id,
            SavePersonaDraftInput {
                project_id: None,
                slug: "finished-claim-loser".to_string(),
                content: persona_content("finished-claim-loser", "Loser"),
                source_session_id: Some(finished_conversation.id.as_str()),
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect_err("a completed builder binding must reject a new draft claim");
    assert!(matches!(finished_error, AppError::Conflict(_)));

    let stored = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("winner binding should remain readable")
        .expect("winner conversation should remain");
    assert_eq!(stored.builder_draft_id.as_deref(), Some(winner.id.as_str()));
    assert!(stored.builder_result_persona_id.is_none());
    let personas = service
        .list_personas(true, PersonaScopeFilter::All)
        .await
        .expect("personas should remain readable");
    assert_eq!(personas.len(), 1, "losing personas must roll back");
    assert_eq!(personas[0].id, winner.id);
    let artifact_count: i64 = state
        .app_state
        .db
        .run(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM artifacts WHERE type = 'persona'",
                [],
                |row| row.get(0),
            )
            .map_err(AppError::from)
        })
        .await
        .expect("artifact count should remain readable");
    assert_eq!(artifact_count, 1, "losing artifacts must roll back");
}

#[tokio::test]
async fn save_draft_rejects_after_plain_seeded_and_as_new_approval() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let service = persona_service(&state);

    let (plain_conversation, plain_headers) = persona_builder_headers(&state).await;
    let plain = save_persona_draft(
        State(state.clone()),
        plain_headers.clone(),
        Json(request(
            "approved-plain",
            persona_content("approved-plain", "Plain"),
        )),
    )
    .await
    .unwrap()
    .0;
    service.approve_persona(true, &plain.id).await.unwrap();

    let source_draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: None,
                slug: "approved-seeded".to_string(),
                content: persona_content("approved-seeded", "Source"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .unwrap();
    let source = service
        .approve_persona(true, &source_draft.id)
        .await
        .unwrap();
    let mut seeded_conversation = ChatConversation::new_standalone();
    seeded_conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let seeded_conversation = state
        .app_state
        .chat_conversation_repo
        .create(seeded_conversation)
        .await
        .unwrap();
    let seeded = service
        .create_bound_draft(
            true,
            &seeded_conversation.id,
            SavePersonaDraftInput {
                project_id: None,
                slug: source.slug.clone(),
                content: persona_content(&source.slug, "Seeded final"),
                source_session_id: Some(seeded_conversation.id.as_str().to_string()),
                source_persona_id: Some(source.id.clone()),
                source_content_hash: Some(source.content_hash.clone()),
            },
        )
        .await
        .unwrap();
    service.approve_persona(true, &seeded.id).await.unwrap();

    let source_draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: None,
                slug: "approved-as-new".to_string(),
                content: persona_content("approved-as-new", "Source"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .unwrap();
    let source = service
        .approve_persona(true, &source_draft.id)
        .await
        .unwrap();
    let mut as_new_conversation = ChatConversation::new_standalone();
    as_new_conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let as_new_conversation = state
        .app_state
        .chat_conversation_repo
        .create(as_new_conversation)
        .await
        .unwrap();
    let as_new = service
        .create_bound_draft(
            true,
            &as_new_conversation.id,
            SavePersonaDraftInput {
                project_id: None,
                slug: source.slug.clone(),
                content: persona_content(&source.slug, "As-new final"),
                source_session_id: Some(as_new_conversation.id.as_str().to_string()),
                source_persona_id: Some(source.id.clone()),
                source_content_hash: Some(source.content_hash.clone()),
            },
        )
        .await
        .unwrap();
    service.archive_persona(true, &source.id).await.unwrap();
    service
        .approve_persona_as_new(true, &as_new.id, Some("approved-as-new-result"))
        .await
        .unwrap();

    for (conversation, headers, slug) in [
        (plain_conversation, plain_headers, "approved-plain"),
        (
            seeded_conversation.clone(),
            conversation_headers(&seeded_conversation),
            "approved-seeded",
        ),
        (
            as_new_conversation.clone(),
            conversation_headers(&as_new_conversation),
            "approved-as-new-result",
        ),
    ] {
        let error = save_persona_draft(
            State(state.clone()),
            headers,
            Json(request(slug, persona_content(slug, "Rejected overwrite"))),
        )
        .await
        .expect_err("approved builder conversation must reject another save");
        assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(error
            .message
            .as_deref()
            .is_some_and(|message| message.contains("persona already approved")));
        let stored = state
            .app_state
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap();
        assert!(stored.builder_draft_id.is_none());
        assert!(stored.builder_result_persona_id.is_some());
    }
}

#[tokio::test]
async fn standalone_builder_save_creates_a_global_draft() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let mut conversation = ChatConversation::new_standalone();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("standalone builder should persist");

    let created = save_persona_draft(
        State(state.clone()),
        conversation_headers(&conversation),
        Json(request(
            "global-builder-draft",
            persona_content("global-builder-draft", "Global draft"),
        )),
    )
    .await
    .expect("standalone builder save should create a bound draft")
    .0;

    assert!(
        created.project_id.is_none(),
        "Standalone builder drafts must stamp NULL/global project scope"
    );
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
}

#[tokio::test]
async fn builder_conversation_cannot_write_another_conversations_bound_draft() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (_conversation_a, headers_a) = persona_builder_headers(&state).await;
    let bound = save_persona_draft(
        State(state.clone()),
        headers_a.clone(),
        Json(request(
            "bound-owner",
            persona_content("bound-owner", "Bound"),
        )),
    )
    .await
    .unwrap()
    .0;
    let (_conversation_b, headers_b) = persona_builder_headers(&state).await;
    let other = save_persona_draft(
        State(state.clone()),
        headers_b,
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
        headers_a,
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
async fn save_draft_rejects_missing_caller_header_without_updating_requested_draft() {
    let _persona_flag = enable_personas(true).await;
    let event_sink = RecordingEventSink::new();
    let state = setup_state(Some(event_sink.clone()));
    let (_conversation, headers) = persona_builder_headers(&state).await;
    let draft = save_persona_draft(
        State(state.clone()),
        headers,
        Json(request(
            "missing-header-target",
            persona_content("missing-header-target", "Before"),
        )),
    )
    .await
    .expect("fixture draft should save")
    .0;

    let error = save_persona_draft(
        State(state.clone()),
        HeaderMap::new(),
        Json(SavePersonaDraftRequest {
            draft_id: Some(draft.id.as_str().to_string()),
            slug: draft.slug.clone(),
            content: persona_content("missing-header-target", "Hijacked"),
            source_session_id: None,
        }),
    )
    .await
    .expect_err("missing caller identity must reject");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains(CALLER_SESSION_ID_HEADER)));
    let unchanged = state
        .app_state
        .persona_repo
        .get_by_id(&draft.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(unchanged.version, 1);
    assert_eq!(unchanged.content, draft.content);
    assert_eq!(
        event_sink.events().len(),
        1,
        "rejected save must not emit another draft-updated event"
    );
}

#[tokio::test]
async fn save_draft_rejects_nonexistent_caller_conversation() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let missing =
        ChatConversation::new_project(ProjectId::from_string("missing-caller-project".to_string()));

    let error = save_persona_draft(
        State(state),
        conversation_headers(&missing),
        Json(request(
            "missing-caller",
            persona_content("missing-caller", "Body"),
        )),
    )
    .await
    .expect_err("unknown caller conversation must reject");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains("conversation was not found")));
}

#[tokio::test]
async fn save_draft_rejects_non_builder_caller_conversation() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "non-builder-project".to_string(),
        )))
        .await
        .unwrap();

    let error = save_persona_draft(
        State(state),
        conversation_headers(&conversation),
        Json(request(
            "non-builder-caller",
            persona_content("non-builder-caller", "Body"),
        )),
    )
    .await
    .expect_err("non-builder caller conversation must reject");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains("not a valid persona builder conversation")));
}

#[tokio::test]
async fn save_draft_rejects_invalid_context_builder_without_mutating_bound_draft() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let service = persona_service(&state);
    let mut conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed invalid-context builder row");
    let draft = service
        .create_bound_draft(
            true,
            &conversation.id,
            SavePersonaDraftInput {
                project_id: None,
                slug: "invalid-context-bound".to_string(),
                content: persona_content("invalid-context-bound", "Before"),
                source_session_id: Some(conversation.id.as_str()),
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("seed bound draft fixture");

    let error = save_persona_draft(
        State(state.clone()),
        conversation_headers(&conversation),
        Json(SavePersonaDraftRequest {
            draft_id: Some(draft.id.as_str().to_string()),
            slug: draft.slug.clone(),
            content: persona_content(&draft.slug, "After"),
            source_session_id: Some(conversation.id.as_str()),
        }),
    )
    .await
    .expect_err("unsupported context must not gain persona-writer authority");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains("Project or Standalone")));
    let unchanged = state
        .app_state
        .persona_repo
        .get_by_id(&draft.id)
        .await
        .expect("load bound draft")
        .expect("bound draft should remain");
    assert_eq!(unchanged.version, draft.version);
    assert_eq!(unchanged.content, draft.content);
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
async fn get_persona_draft_rejects_missing_caller_identity() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (_, headers) = persona_builder_headers(&state).await;
    let draft = save_persona_draft(
        State(state.clone()),
        headers,
        Json(request(
            "missing-read-identity",
            persona_content("missing-read-identity", "Body"),
        )),
    )
    .await
    .expect("draft should save")
    .0;

    let error = get_persona_draft(
        State(state),
        HeaderMap::new(),
        Path(draft.id.as_str().to_string()),
    )
    .await
    .expect_err("missing caller identity must reject");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains(CALLER_SESSION_ID_HEADER)));
}

#[tokio::test]
async fn get_persona_draft_rejects_non_builder_caller() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (_, builder_headers) = persona_builder_headers(&state).await;
    let draft = save_persona_draft(
        State(state.clone()),
        builder_headers,
        Json(request(
            "non-builder-reader",
            persona_content("non-builder-reader", "Body"),
        )),
    )
    .await
    .expect("draft should save")
    .0;
    let conversation = state
        .app_state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "non-builder-reader-project".to_string(),
        )))
        .await
        .expect("seed non-builder caller");

    let error = get_persona_draft(
        State(state),
        conversation_headers(&conversation),
        Path(draft.id.as_str().to_string()),
    )
    .await
    .expect_err("non-builder caller must reject");

    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains("not a valid persona builder conversation")));
}

#[tokio::test]
async fn get_persona_draft_rejects_draft_outside_caller_binding() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (_, first_headers) = persona_builder_headers(&state).await;
    let first_draft = save_persona_draft(
        State(state.clone()),
        first_headers,
        Json(request(
            "first-bound-reader",
            persona_content("first-bound-reader", "Body"),
        )),
    )
    .await
    .expect("first draft should save")
    .0;
    let (_, second_headers) = persona_builder_headers(&state).await;
    let second_draft = save_persona_draft(
        State(state.clone()),
        second_headers.clone(),
        Json(request(
            "second-bound-reader",
            persona_content("second-bound-reader", "Body"),
        )),
    )
    .await
    .expect("second draft should save")
    .0;

    let error = get_persona_draft(
        State(state),
        second_headers,
        Path(first_draft.id.as_str().to_string()),
    )
    .await
    .expect_err("builder must not read outside its bound draft");

    assert_ne!(first_draft.id, second_draft.id);
    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(error
        .message
        .as_deref()
        .is_some_and(|message| message.contains("outside its bound draft")));
}

#[tokio::test]
async fn get_persona_draft_allows_bound_builder_read() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (conversation, headers) = persona_builder_headers(&state).await;
    let draft = save_persona_draft(
        State(state.clone()),
        headers,
        Json(request("round-trip", persona_content("round-trip", "Body"))),
    )
    .await
    .expect("draft should save")
    .0;

    let loaded = get_persona_draft(
        State(state),
        conversation_headers(&conversation),
        Path(draft.id.as_str().to_string()),
    )
    .await
    .expect("draft should load")
    .0;

    assert_eq!(loaded.id, draft.id);
    let source_session_id = conversation.id.as_str();
    assert_eq!(
        loaded.source_session_id.as_deref(),
        Some(source_session_id.as_str())
    );
}

#[tokio::test]
async fn save_draft_handler_updates_draft_and_bumps_version() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (conversation, headers) = persona_builder_headers(&state).await;
    let draft = save_persona_draft(
        State(state.clone()),
        headers.clone(),
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
        headers,
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
    let source_session_id = conversation.id.as_str();
    assert_eq!(
        updated.source_session_id.as_deref(),
        Some(source_session_id.as_str())
    );
}

#[tokio::test]
async fn save_draft_handler_allows_archived_slug_reuse_but_not_live_collision() {
    let _persona_flag = enable_personas(true).await;
    let state = setup_state(None);
    let (_first_conversation, first_headers) = persona_builder_headers(&state).await;
    let first = save_persona_draft(
        State(state.clone()),
        first_headers,
        Json(request(
            "handler-slug",
            persona_content("handler-slug", "First"),
        )),
    )
    .await
    .expect("first draft should save")
    .0;
    let (_collision_conversation, collision_headers) = persona_builder_headers(&state).await;
    let live_collision = save_persona_draft(
        State(state.clone()),
        collision_headers,
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
    let (_reused_conversation, reused_headers) = persona_builder_headers(&state).await;
    let reused = save_persona_draft(
        State(state),
        reused_headers,
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
    let (_conversation, headers) = persona_builder_headers(&state).await;
    let draft = save_persona_draft(
        State(state.clone()),
        headers.clone(),
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
        headers,
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
    let (conversation, headers) = persona_builder_headers(&state).await;
    let conversation_id = conversation.id.as_str();

    let draft = save_persona_draft(
        State(state),
        headers,
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
    assert_eq!(payload.len(), 5);
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
    assert_eq!(
        payload
            .get("artifact_id")
            .and_then(serde_json::Value::as_str),
        draft.artifact_id.as_ref().map(|id| id.as_str())
    );
    assert_eq!(
        payload
            .get("builder_conversation_id")
            .and_then(serde_json::Value::as_str),
        Some(conversation_id.as_str())
    );
    assert!(!payload.contains_key("content"));
    assert!(!payload.contains_key("body"));
}
