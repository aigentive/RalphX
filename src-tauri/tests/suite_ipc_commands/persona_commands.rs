use std::sync::Arc;

use ralphx_domain::personas::validation::validate_persona_content;
use ralphx_events::RecordingEventSink;
use ralphx_lib::application::personas::PERSONA_DRAFT_CONFLICT_CODE;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::persona_commands::{
    approve_persona, approve_persona_for_state, archive_persona, archive_persona_for_state,
    create_persona_draft, create_persona_draft_for_state, delete_persona_draft,
    delete_persona_draft_for_state, get_persona, get_persona_for_state, list_personas,
    list_personas_for_state, update_persona, update_persona_draft, update_persona_draft_for_state,
    update_persona_for_state, CreatePersonaDraftInput, ListPersonasInput, PersonaIdInput,
    UpdatePersonaDraftInput, UpdatePersonaInput,
};
use ralphx_lib::domain::entities::{ArtifactContent, Persona, PersonaStatus};
use ralphx_lib::infrastructure::sqlite::SqlitePersonaRepository;
use tauri::Manager;

fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Test persona\n---\n{body}")
}

fn persona_content_with_description(slug: &str, description: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: {description}\n---\n{body}")
}

async fn replace_structured_fields_with_stale_values(state: &AppState, persona: &Persona) {
    let id = persona.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE personas SET name = 'Stale name', description = 'Stale description'
                 WHERE id = ?1",
                [id],
            )?;
            Ok(())
        })
        .await
        .expect("stale structured persona fixture should persist");
}

async fn assert_persona_artifact_matches_structured_update(state: &AppState, persona: &Persona) {
    let parsed = validate_persona_content(&persona.slug, &persona.content)
        .expect("updated persona should retain canonical markdown");
    assert_eq!(persona.name, parsed.frontmatter.name);
    assert_eq!(persona.description, parsed.frontmatter.description);

    let artifact = state
        .artifact_repo
        .get_by_id(
            persona
                .artifact_id
                .as_ref()
                .expect("updated persona should have an artifact tip"),
        )
        .await
        .expect("artifact lookup should succeed")
        .expect("updated persona artifact should exist");
    assert_eq!(artifact.name, persona.name);
    assert_eq!(artifact.metadata.created_by, "user");
    assert_eq!(i64::from(artifact.metadata.version), persona.version);
    let metadata = artifact
        .metadata
        .custom_metadata
        .expect("persona artifact should carry custom metadata");
    assert_eq!(metadata["persona_version"], persona.version);
    assert_eq!(metadata["created_by"], "user");
    assert_eq!(
        artifact.content,
        ArtifactContent::inline(persona.content.clone())
    );
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
    assert_eq!(create.content.as_deref(), Some("body"));
    assert_eq!(create.source_session_id.as_deref(), Some("session-1"));

    let update: UpdatePersonaInput =
        serde_json::from_str(r#"{"id":"persona-1","content":"updated"}"#)
            .expect("camelCase update input should deserialize inside the input wrapper");
    assert_eq!(update.id, "persona-1");
    assert_eq!(update.content.as_deref(), Some("updated"));

    let draft_update: UpdatePersonaDraftInput = serde_json::from_str(
        r#"{"id":"draft-1","content":"updated","expectedContentHash":"hash-v1"}"#,
    )
    .expect("camelCase draft CAS input should deserialize inside the input wrapper");
    assert_eq!(
        draft_update.expected_content_hash.as_deref(),
        Some("hash-v1")
    );

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
async fn update_persona_draft_command_enforces_cas_and_emits_only_after_success() {
    let (app, events) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "manual-draft".to_string(),
            content: Some(persona_content("manual-draft", "Initial body")),
            description: None,
            body: None,
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should create");
    replace_structured_fields_with_stale_values(state.inner(), &draft).await;
    let events_after_create = events.events().len();

    let updated_content = persona_content_with_description(
        "manual-draft",
        "Updated draft description",
        "Manual edit",
    );

    let updated = update_persona_draft_for_state(
        UpdatePersonaDraftInput {
            id: draft.id.as_str().to_string(),
            content: updated_content.clone(),
            expected_content_hash: Some(draft.content_hash.clone()),
        },
        state.inner(),
        true,
    )
    .await
    .expect("matching draft hash should update through the command path");
    assert_eq!(updated.version, draft.version + 1);
    assert_eq!(updated.name, "manual-draft");
    assert_eq!(updated.description, "Updated draft description");
    assert_eq!(updated.content, updated_content);
    assert_eq!(
        get_persona_for_state(
            PersonaIdInput {
                id: draft.id.as_str().to_string(),
            },
            state.inner(),
            true,
        )
        .await
        .expect("updated draft should reload"),
        updated
    );
    assert_persona_artifact_matches_structured_update(state.inner(), &updated).await;
    let emitted = events.events();
    assert_eq!(emitted.len(), events_after_create + 1);
    assert_eq!(emitted.last().unwrap().event, "persona:draft_updated");
    assert_eq!(emitted.last().unwrap().payload["version"], updated.version);
    assert!(emitted.last().unwrap().payload.get("content").is_none());

    let stale_error = update_persona_draft_for_state(
        UpdatePersonaDraftInput {
            id: draft.id.as_str().to_string(),
            content: persona_content("manual-draft", "Stale overwrite"),
            expected_content_hash: Some(draft.content_hash.clone()),
        },
        state.inner(),
        true,
    )
    .await
    .expect_err("stale hash must surface a stable typed conflict string");
    assert!(stale_error.starts_with(PERSONA_DRAFT_CONFLICT_CODE));
    assert_eq!(events.events().len(), events_after_create + 1);
    assert_eq!(
        get_persona_for_state(
            PersonaIdInput {
                id: draft.id.as_str().to_string(),
            },
            state.inner(),
            true,
        )
        .await
        .unwrap(),
        updated,
        "conflict must not mutate the draft"
    );

    approve_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("fixture draft should activate");
    let events_before_active_rejection = events.events().len();
    let active_error = update_persona_draft_for_state(
        UpdatePersonaDraftInput {
            id: draft.id.as_str().to_string(),
            content: persona_content("manual-draft", "Invalid active edit"),
            expected_content_hash: Some(updated.content_hash),
        },
        state.inner(),
        true,
    )
    .await
    .expect_err("manual draft command must reject active personas");
    assert!(active_error.contains("must be draft"));
    assert_eq!(events.events().len(), events_before_active_rejection);

    assert_disabled(
        update_persona_draft(
            UpdatePersonaDraftInput {
                id: draft.id.as_str().to_string(),
                content: persona_content("manual-draft", "Disabled"),
                expected_content_hash: None,
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn list_personas_command_lists_enabled_personas_and_rejects_the_disabled_wrapper() {
    let (app, _) = command_app();

    let personas = list_personas_for_state(
        ListPersonasInput { scope: None },
        app.state::<AppState>().inner(),
        true,
    )
    .await
    .expect("enabled list command should return the current empty collection");
    assert!(personas.is_empty());

    assert_disabled(
        list_personas(ListPersonasInput { scope: None }, app.state::<AppState>()).await,
    );
}

#[tokio::test]
async fn get_persona_command_returns_created_draft_and_maps_invalid_or_missing_ids() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let created = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "get-persona".to_string(),
            content: Some(persona_content("get-persona", "Draft body")),
            description: None,
            body: None,
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
            project_id: None,
            slug: "create-persona".to_string(),
            content: Some(persona_content("create-persona", "secret draft body")),
            description: None,
            body: None,
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
            project_id: None,
            slug: "invalid-persona".to_string(),
            content: Some("not persona markdown".to_string()),
            description: None,
            body: None,
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
                project_id: None,
                slug: "disabled-create".to_string(),
                content: Some(persona_content("disabled-create", "Never persisted")),
                description: None,
                body: None,
                source_session_id: None,
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn create_persona_draft_rejects_blank_project_ids_and_accepts_valid_or_absent_scope() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();

    for project_id in ["", "  "] {
        let error = create_persona_draft_for_state(
            CreatePersonaDraftInput {
                project_id: Some(project_id.to_string()),
                slug: "blank-project-id".to_string(),
                content: Some(persona_content("blank-project-id", "Draft body")),
                description: None,
                body: None,
                source_session_id: None,
            },
            state.inner(),
            true,
        )
        .await
        .expect_err("blank persona project ids must be rejected");
        assert_eq!(
            error,
            "Validation error: persona project id cannot be empty"
        );
    }

    let scoped = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: Some(" project-a ".to_string()),
            slug: "scoped-project-id".to_string(),
            content: Some(persona_content("scoped-project-id", "Draft body")),
            description: None,
            body: None,
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("a valid project id should create a scoped draft");
    assert_eq!(
        scoped.project_id.as_ref().map(|id| id.as_str()),
        Some("project-a")
    );

    let global = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "absent-project-id".to_string(),
            content: Some(persona_content("absent-project-id", "Draft body")),
            description: None,
            body: None,
            source_session_id: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("an absent project id should create a global draft");
    assert!(global.project_id.is_none());
}

#[tokio::test]
async fn persona_create_command_composes_structured_fields_before_validation() {
    let (app, _) = command_app();
    let created = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "design-voice".to_string(),
            content: None,
            description: Some("Opinionated: product design".to_string()),
            body: Some("Prefer concise, practical recommendations.".to_string()),
            source_session_id: None,
        },
        app.state::<AppState>().inner(),
        true,
    )
    .await
    .expect("structured create should compose valid persona content");

    assert_eq!(created.slug, "design-voice");
    assert_eq!(created.description, "Opinionated: product design");
    assert!(created.content.starts_with("---\n"));
    assert!(created.content.contains("kind: persona"));
    assert!(created
        .content
        .ends_with("Prefer concise, practical recommendations.\n"));
}

#[tokio::test]
async fn persona_create_command_requires_content_or_complete_structured_fields() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    for (description, body) in [
        (None, None),
        (Some("Description".to_string()), Some("  ".to_string())),
        (Some("  ".to_string()), Some("Instructions".to_string())),
    ] {
        let error = create_persona_draft_for_state(
            CreatePersonaDraftInput {
                project_id: None,
                slug: "incomplete-persona".to_string(),
                content: None,
                description,
                body,
                source_session_id: None,
            },
            state.inner(),
            true,
        )
        .await
        .expect_err("incomplete structured persona input must be rejected");

        assert_eq!(
            error,
            "persona content or description+instructions required"
        );
    }
}

#[tokio::test]
async fn update_persona_command_updates_active_content_and_rejects_invalid_ids_or_flags() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "update-persona".to_string(),
            content: Some(persona_content("update-persona", "Draft body")),
            description: None,
            body: None,
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

    let active_before_update = get_persona_for_state(
        PersonaIdInput {
            id: draft.id.as_str().to_string(),
        },
        state.inner(),
        true,
    )
    .await
    .expect("active fixture should reload");
    replace_structured_fields_with_stale_values(state.inner(), &active_before_update).await;
    let updated_content = persona_content_with_description(
        "update-persona",
        "Updated active description",
        "Updated active body",
    );

    let updated = update_persona_for_state(
        UpdatePersonaInput {
            id: draft.id.as_str().to_string(),
            content: Some(updated_content.clone()),
            description: None,
            body: None,
        },
        state.inner(),
        true,
    )
    .await
    .expect("enabled update command should update the active persona");
    assert_eq!(updated.name, "update-persona");
    assert_eq!(updated.description, "Updated active description");
    assert_eq!(updated.content, updated_content);
    assert_eq!(updated.version, 3);
    assert_eq!(
        get_persona_for_state(
            PersonaIdInput {
                id: draft.id.as_str().to_string(),
            },
            state.inner(),
            true,
        )
        .await
        .expect("updated active persona should reload"),
        updated
    );
    assert_persona_artifact_matches_structured_update(state.inner(), &updated).await;

    assert_eq!(
        update_persona_for_state(
            UpdatePersonaInput {
                id: String::new(),
                content: Some(persona_content("update-persona", "Ignored")),
                description: None,
                body: None,
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
                content: Some(persona_content("update-persona", "Disabled")),
                description: None,
                body: None,
            },
            state,
        )
        .await,
    );
}

#[tokio::test]
async fn persona_update_command_recomposes_structured_fields_with_the_existing_slug() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "immutable-slug".to_string(),
            content: Some(persona_content("immutable-slug", "Draft body")),
            description: None,
            body: None,
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
            content: None,
            description: Some("A changed description".to_string()),
            body: Some("Use a calmer tone.".to_string()),
        },
        state.inner(),
        true,
    )
    .await
    .expect("structured update should compose with the persisted slug");

    assert_eq!(updated.slug, "immutable-slug");
    let parsed = validate_persona_content("immutable-slug", &updated.content)
        .expect("updated content should remain a valid persona document");
    assert_eq!(parsed.frontmatter.description, "A changed description");
    assert_eq!(parsed.body, "\nUse a calmer tone.\n");

    for (description, body) in [
        (Some("Description".to_string()), Some(" ".to_string())),
        (Some(" ".to_string()), Some("Instructions".to_string())),
    ] {
        let error = update_persona_for_state(
            UpdatePersonaInput {
                id: draft.id.as_str().to_string(),
                content: None,
                description,
                body,
            },
            state.inner(),
            true,
        )
        .await
        .expect_err("incomplete structured persona update must be rejected");

        assert_eq!(
            error,
            "persona content or description+instructions required"
        );
    }
}

#[tokio::test]
async fn approve_persona_command_promotes_a_draft_and_rejects_invalid_ids_or_flags() {
    let (app, _) = command_app();
    let state = app.state::<AppState>();
    let draft = create_persona_draft_for_state(
        CreatePersonaDraftInput {
            project_id: None,
            slug: "approve-persona".to_string(),
            content: Some(persona_content("approve-persona", "Draft body")),
            description: None,
            body: None,
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
            project_id: None,
            slug: "archive-persona".to_string(),
            content: Some(persona_content("archive-persona", "Draft body")),
            description: None,
            body: None,
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
            project_id: None,
            slug: "delete-persona".to_string(),
            content: Some(persona_content("delete-persona", "Draft body")),
            description: None,
            body: None,
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
