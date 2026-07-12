use std::sync::Arc;

use ralphx_events::RecordingEventSink;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::persona_commands::{
    approve_persona, approve_persona_for_state, archive_persona, archive_persona_for_state,
    create_persona_draft, create_persona_draft_for_state, delete_persona_draft,
    delete_persona_draft_for_state, get_persona, get_persona_for_state, list_personas,
    list_personas_for_state, update_persona, update_persona_for_state, CreatePersonaDraftInput,
    ListPersonasInput, PersonaIdInput, UpdatePersonaInput,
};
use ralphx_lib::domain::entities::PersonaStatus;
use ralphx_lib::infrastructure::sqlite::SqlitePersonaRepository;
use tauri::Manager;

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Test persona\n---\n{body}")
}

fn command_app() -> (tauri::App<tauri::test::MockRuntime>, RecordingEventSink) {
    let mut state = AppState::new_sqlite_test();
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(
        state.db.inner(),
    )));
    let events = RecordingEventSink::new();
    state.events = Arc::new(events.clone());
    let app = tauri::test::mock_builder()
        .manage(state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("persona command mock app should build");
    (app, events)
}

fn assert_disabled<T: std::fmt::Debug>(result: Result<T, String>) {
    let error = result.expect_err("the default mock runtime feature flag should be off");
    assert!(
        error.starts_with("[Personas disabled:"),
        "feature-disabled command error should keep the persona boundary prefix: {error}"
    );
}

#[test]
fn persona_commands_use_struct_param_wrapping() {
    let create: CreatePersonaDraftInput = serde_json::from_str(
        r#"{"slug":"persona-one","content":"body","sourceSessionId":"session-1"}"#,
    )
    .expect("camelCase create input should deserialize inside the input wrapper");
    assert_eq!(create.source_session_id.as_deref(), Some("session-1"));

    let update: UpdatePersonaInput =
        serde_json::from_str(r#"{"id":"persona-1","content":"updated"}"#)
            .expect("camelCase update input should deserialize inside the input wrapper");
    assert_eq!(update.id, "persona-1");

    let id: PersonaIdInput = serde_json::from_str(r#"{"id":"persona-1"}"#)
        .expect("id input should deserialize inside the input wrapper");
    assert_eq!(id.id, "persona-1");

    let snake_case: Result<CreatePersonaDraftInput, _> = serde_json::from_str(
        r#"{"slug":"persona-one","content":"body","source_session_id":"session-1"}"#,
    );
    assert!(
        snake_case.is_ok(),
        "optional unknown snake_case fields are ignored"
    );
    assert!(snake_case.unwrap().source_session_id.is_none());
}

#[tokio::test]
async fn list_personas_command_lists_enabled_personas_and_rejects_the_disabled_wrapper() {
    let (app, _) = command_app();

    let personas =
        list_personas_for_state(ListPersonasInput {}, app.state::<AppState>().inner(), true)
            .await
            .expect("enabled list command should return the current empty collection");
    assert!(personas.is_empty());

    assert_disabled(list_personas(ListPersonasInput {}, app.state::<AppState>()).await);
}

#[tokio::test]
async fn get_persona_command_returns_created_draft_and_maps_invalid_or_missing_ids() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let created = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "get-persona".to_string(),
            content: persona_content("get-persona", "Draft body"),
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should create");

    let fetched = get_persona_for_state(
        PersonaIdInput {
            id: created.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled get command should return the draft");
    assert_eq!(fetched.id, created.id);

    let missing = get_persona_for_state(
        PersonaIdInput {
            id: "missing-persona".to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect_err("missing persona should map the service error to IPC text");
    assert!(missing.contains("Persona not found: missing-persona"));

    assert_eq!(
        get_persona_for_state(
            PersonaIdInput {
                id: "  ".to_string()
            },
            state.inner(),
            true,
        )
        .await
        .expect_err("blank IDs should be rejected before repository lookup"),
        "persona id cannot be empty"
    );
    assert_disabled(
        get_persona(
            PersonaIdInput {
                id: created.id.as_str().to_string(),
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn create_persona_draft_command_emits_a_redacted_event_and_maps_validation_errors() {
    let (app, events) = command_app();
    let state = app.state::<AppState>();
    let created = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "create-persona".to_string(),
            content: persona_content("create-persona", "secret draft body"),
            source_session_id: Some("source-session".to_string()),
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled create command should persist a draft");
    assert_eq!(created.status, PersonaStatus::Draft);

    let emitted = events.events();
    assert_eq!(emitted.len(), 1, "creating a draft should emit one update");
    assert_eq!(emitted[0].event, "persona:draft_updated");
    assert_eq!(
        emitted[0].payload["draft_id"].as_str(),
        Some(created.id.as_str())
    );
    assert_eq!(emitted[0].payload["version"], 1);
    assert!(emitted[0].payload.get("content").is_none());

    let invalid = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "invalid-persona".to_string(),
            content: "not persona markdown".to_string(),
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect_err("invalid persona content should map validation into the IPC error string");
    assert!(invalid.contains("missing YAML frontmatter"));

    assert_disabled(
        create_persona_draft(
            CreatePersonaDraftInput {
                slug: "disabled-create".to_string(),
                content: persona_content("disabled-create", "Never persisted"),
                source_session_id: None,
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn update_persona_command_updates_active_content_and_rejects_invalid_ids_or_flags() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "update-persona".to_string(),
            content: persona_content("update-persona", "Draft body"),
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should create");
    approve_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should approve");

    let updated = update_persona_for_state(
        UpdatePersonaInput {
            id: draft.id.as_str().to_string(),
            content: persona_content("update-persona", "Updated active body"),
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled update command should update the active persona");
    assert_eq!(
        updated.content,
        persona_content("update-persona", "Updated active body")
    );
    assert_eq!(updated.version, 3);

    assert_eq!(
        update_persona_for_state(
            UpdatePersonaInput {
                id: String::new(),
                content: persona_content("update-persona", "Ignored"),
            },
            state.inner(),
            true,
        )
        .await
        .expect_err("blank IDs should map to the command validation error"),
        "persona id cannot be empty"
    );
    assert_disabled(
        update_persona(
            UpdatePersonaInput {
                id: draft.id.as_str().to_string(),
                content: persona_content("update-persona", "Disabled"),
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn approve_persona_command_promotes_a_draft_and_rejects_invalid_ids_or_flags() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "approve-persona".to_string(),
            content: persona_content("approve-persona", "Draft body"),
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should create");

    let approved = approve_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled approve command should promote the draft");
    assert_eq!(approved.status, PersonaStatus::Active);

    assert_eq!(
        approve_persona_for_state(PersonaIdInput { id: String::new() }, state.inner(), true,)
            .await
            .expect_err("blank IDs should map to the command validation error"),
        "persona id cannot be empty"
    );
    assert_disabled(
        approve_persona(
            PersonaIdInput {
                id: draft.id.as_str().to_string(),
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn archive_persona_command_archives_active_personas_and_rejects_invalid_ids_or_flags() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "archive-persona".to_string(),
            content: persona_content("archive-persona", "Draft body"),
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should create");
    approve_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should approve");

    let archived = archive_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled archive command should archive the active persona");
    assert_eq!(archived.status, PersonaStatus::Archived);

    assert_eq!(
        archive_persona_for_state(PersonaIdInput { id: String::new() }, state.inner(), true,)
            .await
            .expect_err("blank IDs should map to the command validation error"),
        "persona id cannot be empty"
    );
    assert_disabled(
        archive_persona(
            PersonaIdInput {
                id: draft.id.as_str().to_string(),
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn delete_persona_draft_command_removes_drafts_and_rejects_invalid_ids_or_flags() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            slug: "delete-persona".to_string(),
            content: persona_content("delete-persona", "Draft body"),
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should create");

    delete_persona_draft_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled delete command should remove the draft");
    let missing = get_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect_err("deleted draft should no longer be readable");
    assert!(missing.contains("Persona not found"));

    assert_eq!(
        delete_persona_draft_for_state(PersonaIdInput { id: String::new() }, state.inner(), true,)
            .await
            .expect_err("blank IDs should map to the command validation error"),
        "persona id cannot be empty"
    );
    assert_disabled(
        delete_persona_draft(
            PersonaIdInput {
                id: "still-valid-id".to_string(),
            },
            state,
        )
        .await,
    );
}
