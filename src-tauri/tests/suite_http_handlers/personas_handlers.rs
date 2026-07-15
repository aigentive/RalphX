use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use ralphx_events::RecordingEventSink;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::PersonaStatus;
use ralphx_lib::http_server::delegation::DelegationService;
use ralphx_lib::http_server::handlers::{
    get_persona_draft, save_persona_draft, SavePersonaDraftRequest,
};
use ralphx_lib::http_server::types::HttpServerState;

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Handler test persona\n---\n{body}")
}

fn enable_personas(enabled: bool) {
    std::env::set_var(
        "RALPHX_UI_AGENT_PERSONAS",
        if enabled { "true" } else { "false" },
    );
}

fn setup_state(event_sink: Option<RecordingEventSink>) -> HttpServerState {
    let mut app_state = AppState::new_sqlite_test();
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

#[tokio::test]
async fn save_draft_handler_rejects_when_flag_off() {
    enable_personas(false);
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
    enable_personas(true);
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
    enable_personas(true);
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
    enable_personas(true);
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
    enable_personas(true);
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
    enable_personas(true);
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
