use chrono::Utc;
use ralphx_lib::application::persona_ingest::{
    build_persona_ingest_file_path, persona_builder_ingest_session_is_live,
    persona_ingest_conversation_path, persona_ingest_storage_path,
};
use ralphx_lib::application::personas::PERSONA_FEATURE_DISABLED_PREFIX;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::persona_builder_commands::{
    create_persona_builder_conversation, create_persona_builder_conversation_for_state,
    get_persona_builder_ingest_status_for_state, ingest_persona_context,
    ingest_persona_context_for_state, CreatePersonaBuilderConversationInput,
    IngestPersonaContextInput, PersonaBuilderIngestStatusInput,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, Persona, PersonaId, PersonaStatus, ProjectId,
};
use ralphx_lib::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use ralphx_lib::testing::SqliteTestDb;
use std::fs;
use std::sync::Arc;
use tauri::Manager;

fn persona_builder_command_app() -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(AppState::new_test())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app should build")
}

#[tokio::test]
async fn builder_conversation_created_only_via_flag_gated_settings_command() {
    let state = AppState::new_test();
    let input = CreatePersonaBuilderConversationInput {
        project_id: "project-persona-builder-command".to_string(),
        source_persona_id: None,
    };

    let disabled = create_persona_builder_conversation_for_state(input.clone(), &state, false)
        .await
        .expect_err("flag-off PersonaBuilder creation must return the typed disabled error");
    assert!(
        disabled.contains(PERSONA_FEATURE_DISABLED_PREFIX),
        "unexpected disabled error: {disabled}"
    );

    let response = create_persona_builder_conversation_for_state(input, &state, true)
        .await
        .expect("flag-on PersonaBuilder creation should persist its conversation");
    assert_eq!(response.context_id, "project-persona-builder-command");
    assert_eq!(response.agent_mode.as_deref(), Some("persona_builder"));
    assert_eq!(response.title.as_deref(), Some("Persona builder"));

    let stored = state
        .chat_conversation_repo
        .get_by_id(&ralphx_lib::domain::entities::ChatConversationId::from_string(response.id))
        .await
        .expect("conversation lookup succeeds")
        .expect("created conversation persists");
    assert_eq!(
        stored.agent_mode,
        Some(AgentConversationWorkspaceMode::PersonaBuilder)
    );
    assert_eq!(stored.title.as_deref(), Some("Persona builder"));
    assert!(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&stored.id)
        .await
        .expect("workspace lookup succeeds")
        .is_none());

    let project_id = ProjectId::from_string(stored.context_id);
    assert_eq!(project_id.as_str(), "project-persona-builder-command");
}

#[tokio::test]
async fn builder_conversation_rejects_blank_project_ids() {
    let state = AppState::new_test();

    for project_id in ["", "  "] {
        let error = create_persona_builder_conversation_for_state(
            CreatePersonaBuilderConversationInput {
                project_id: project_id.to_string(),
                source_persona_id: None,
            },
            &state,
            true,
        )
        .await
        .expect_err("blank persona builder project ids must be rejected");
        assert_eq!(
            error,
            "Validation error: persona project id cannot be empty"
        );
    }
}

fn source_persona(id: &str, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: Some(ProjectId::from_string("project-persona-update".to_string())),
        slug: "existing-reviewer".to_string(),
        name: "Existing Reviewer".to_string(),
        description: "Existing persona to update".to_string(),
        content: "---\nname: existing-reviewer\nkind: persona\ndescription: Existing persona to update\n---\nOriginal body".to_string(),
        status,
        version: 7,
        content_hash: "source-hash-v7".to_string(),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn update_mode_seeds_binds_and_reuses_one_draft_conversation() {
    let db = SqliteTestDb::new("persona_builder_update_mode");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    let source = source_persona("source-persona", PersonaStatus::Active);
    state.persona_repo.create(source.clone()).await.unwrap();
    let input = CreatePersonaBuilderConversationInput {
        project_id: "project-persona-update".to_string(),
        source_persona_id: Some(source.id.as_str().to_string()),
    };

    let first = create_persona_builder_conversation_for_state(input.clone(), &state, true)
        .await
        .expect("source mode should seed and bind a draft");
    let draft_id = first
        .builder_draft_id
        .as_deref()
        .expect("create response must carry the authoritative draft id");
    let draft = state
        .persona_repo
        .get_by_id(&PersonaId::from(draft_id))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.status, PersonaStatus::Draft);
    assert_eq!(draft.source_persona_id.as_ref(), Some(&source.id));
    assert_eq!(draft.source_content_hash.as_deref(), Some("source-hash-v7"));
    assert_eq!(draft.content, source.content);

    let second = create_persona_builder_conversation_for_state(input, &state, true)
        .await
        .expect("re-entry should reuse the draft-first binding");
    assert_eq!(second.id, first.id);
    assert_eq!(second.builder_draft_id, first.builder_draft_id);
    let sourced_drafts = state
        .persona_repo
        .list_by_status(PersonaStatus::Draft)
        .await
        .unwrap()
        .into_iter()
        .filter(|persona| persona.source_persona_id.as_ref() == Some(&source.id))
        .count();
    assert_eq!(sourced_drafts, 1);
}

#[test]
fn persona_builder_command_input_uses_camel_case_project_id() {
    let input: CreatePersonaBuilderConversationInput =
        serde_json::from_str(r#"{"projectId":"project-persona-builder-input"}"#)
            .expect("camelCase projectId should deserialize");
    assert_eq!(input.project_id, "project-persona-builder-input");
    assert_eq!(input.source_persona_id, None);
    let update: CreatePersonaBuilderConversationInput = serde_json::from_str(
        r#"{"projectId":"project-persona-builder-input","sourcePersonaId":"persona-1"}"#,
    )
    .expect("camelCase sourcePersonaId should deserialize");
    assert_eq!(update.source_persona_id.as_deref(), Some("persona-1"));

    let ingest: IngestPersonaContextInput = serde_json::from_str(
        r#"{"conversationId":"conversation-persona-builder-input","pickedPaths":["/tmp/context.md","/tmp/context-dir"]}"#,
    )
    .expect("camelCase ingestion input should deserialize");
    assert_eq!(ingest.conversation_id, "conversation-persona-builder-input");
    assert_eq!(ingest.picked_paths, ["/tmp/context.md", "/tmp/context-dir"]);

    let status: PersonaBuilderIngestStatusInput =
        serde_json::from_str(r#"{"conversationId":"conversation-persona-builder-input"}"#)
            .expect("camelCase conversationId should deserialize");
    assert_eq!(status.conversation_id, "conversation-persona-builder-input");
}

#[test]
fn persona_builder_ingest_session_liveness_requires_a_non_empty_owned_store() {
    let temp = tempfile::tempdir().expect("persona ingest liveness temp directory");
    let app_data_dir = temp.path().join("app-data");
    let conversation_id = "persona-builder-live-ingest";
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        conversation_id,
    );

    assert!(
        !persona_builder_ingest_session_is_live(None, conversation_id),
        "an absent app data directory must fail closed"
    );
    assert!(
        !persona_builder_ingest_session_is_live(Some(&app_data_dir), conversation_id),
        "a missing ingest directory must not be live"
    );

    fs::create_dir_all(&ingest_root).expect("create empty persona ingest root");
    assert!(
        !persona_builder_ingest_session_is_live(Some(&app_data_dir), conversation_id),
        "an empty ingest directory must not be live"
    );

    fs::write(ingest_root.join("content"), "approved context")
        .expect("write persona ingest content");
    assert!(
        persona_builder_ingest_session_is_live(Some(&app_data_dir), conversation_id),
        "a non-empty validated ingest directory must be live"
    );
}

#[tokio::test]
async fn persona_builder_command_adapters_use_mock_app_state_and_live_feature_flag() {
    let app = persona_builder_command_app();

    let creation_error = create_persona_builder_conversation(
        CreatePersonaBuilderConversationInput {
            project_id: "project-persona-builder-wrapper".to_string(),
            source_persona_id: None,
        },
        app.state(),
    )
    .await
    .expect_err("the checked-in feature flag must keep the builder entry point unavailable");
    assert!(creation_error.contains(PERSONA_FEATURE_DISABLED_PREFIX));

    let ingest_error = ingest_persona_context(
        IngestPersonaContextInput {
            conversation_id: "missing-persona-builder-wrapper".to_string(),
            picked_paths: vec!["not-inspected-while-disabled".to_string()],
        },
        app.state(),
        app.handle().clone(),
    )
    .await
    .expect_err("the checked-in feature flag must reject ingestion through the command adapter");
    assert!(ingest_error.contains(PERSONA_FEATURE_DISABLED_PREFIX));
}

#[tokio::test]
async fn ingest_command_rejects_flag_off() {
    let state = AppState::new_test();
    let input = IngestPersonaContextInput {
        conversation_id: "missing-persona-builder".to_string(),
        picked_paths: vec!["not-inspected-while-disabled".to_string()],
    };

    let error = ingest_persona_context_for_state(input, &state, false, std::path::Path::new("."))
        .await
        .expect_err("flag-off ingestion must reject before conversation or filesystem access");

    assert!(error.contains(PERSONA_FEATURE_DISABLED_PREFIX));
}

#[tokio::test]
async fn ingest_command_rejects_non_persona_builder_conversation() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-non-persona-builder".to_string(),
        )))
        .await
        .expect("non-PersonaBuilder fixture conversation");
    let input = IngestPersonaContextInput {
        conversation_id: conversation.id.as_str().to_string(),
        picked_paths: vec!["not-inspected-for-wrong-mode".to_string()],
    };

    let error = ingest_persona_context_for_state(input, &state, true, std::path::Path::new("."))
        .await
        .expect_err("non-PersonaBuilder conversation must reject before filesystem access");

    assert!(error.contains("PersonaBuilder"));
}

#[tokio::test]
async fn ingest_command_rejects_an_empty_path_batch() {
    let state = AppState::new_test();
    let conversation = create_persona_builder_conversation_for_state(
        CreatePersonaBuilderConversationInput {
            project_id: "project-persona-builder-empty-ingest".to_string(),
            source_persona_id: None,
        },
        &state,
        true,
    )
    .await
    .expect("PersonaBuilder conversation");
    let temp = tempfile::tempdir().expect("ingest temp directory");

    let error = ingest_persona_context_for_state(
        IngestPersonaContextInput {
            conversation_id: conversation.id,
            picked_paths: Vec::new(),
        },
        &state,
        true,
        temp.path(),
    )
    .await
    .expect_err("an empty path batch must reject");

    assert!(error.contains("at least one"));
}

#[tokio::test]
async fn ingest_command_copies_context_for_a_persona_builder_conversation() {
    let state = AppState::new_test();
    let conversation = create_persona_builder_conversation_for_state(
        CreatePersonaBuilderConversationInput {
            project_id: "project-persona-builder-ingest".to_string(),
            source_persona_id: None,
        },
        &state,
        true,
    )
    .await
    .expect("PersonaBuilder conversation should persist before ingest");
    let temp = tempfile::tempdir().expect("ingest temp directory");
    let picked_path = temp.path().join("context.md");
    fs::write(&picked_path, "Persona context\n").expect("write picked context");
    let app_data_dir = temp.path().join("app-data");

    let manifest = ingest_persona_context_for_state(
        IngestPersonaContextInput {
            conversation_id: conversation.id.clone(),
            picked_paths: vec![picked_path.to_string_lossy().to_string()],
        },
        &state,
        true,
        &app_data_dir,
    )
    .await
    .expect("PersonaBuilder ingestion should copy approved context into app-owned storage");

    assert_eq!(manifest.copied.len(), 1);
    assert_eq!(manifest.copied[0].path, "context.md");
    assert!(manifest.skipped.is_empty());
    assert!(manifest.rejected.is_empty());

    let destination_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id,
    );
    let canonical_picked_path = picked_path.canonicalize().expect("canonical picked path");
    let copied_path = build_persona_ingest_file_path(
        &destination_root,
        &canonical_picked_path,
        std::path::Path::new("context.md"),
    )
    .expect("ingest destination is derived from the approved relative path");
    assert_eq!(
        fs::read_to_string(copied_path).expect("app-owned copy should be readable"),
        "Persona context\n"
    );
    assert!(destination_root.join("manifest.json").is_file());
}

#[tokio::test]
async fn ingest_command_maps_absent_persona_builder_conversation_to_not_found() {
    let state = AppState::new_test();
    let temp = tempfile::tempdir().expect("ingest temp directory");
    let app_data_dir = temp.path().join("app-data");

    let error = ingest_persona_context_for_state(
        IngestPersonaContextInput {
            conversation_id: "missing-persona-builder".to_string(),
            picked_paths: vec![temp.path().join("context.md").to_string_lossy().to_string()],
        },
        &state,
        true,
        &app_data_dir,
    )
    .await
    .expect_err("an absent conversation must not ingest picked context");

    assert_eq!(error, "PersonaBuilder conversation was not found");
    assert!(
        !app_data_dir.exists(),
        "a missing conversation must not create app-owned ingest storage"
    );
}

#[tokio::test]
async fn persona_ingest_status_command_rejects_flag_off() {
    let state = AppState::new_test();

    let error = get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: "missing-persona-builder".to_string(),
        },
        &state,
        false,
        std::path::Path::new("."),
    )
    .await
    .expect_err("flag-off status lookup must reject before conversation or filesystem access");

    assert!(error.contains(PERSONA_FEATURE_DISABLED_PREFIX));
}

#[tokio::test]
async fn persona_ingest_status_command_rejects_missing_conversation() {
    let state = AppState::new_test();
    let temp = tempfile::tempdir().expect("persona ingest status temp directory");
    let app_data_dir = temp.path().join("app-data");

    let error = get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: "missing-persona-builder".to_string(),
        },
        &state,
        true,
        &app_data_dir,
    )
    .await
    .expect_err("missing PersonaBuilder conversation must reject status lookup");

    assert_eq!(error, "PersonaBuilder conversation was not found");
    assert!(
        !app_data_dir.exists(),
        "a missing conversation must not create app-owned ingest storage"
    );
}

#[tokio::test]
async fn persona_ingest_status_command_rejects_non_builder_conversation() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "project-non-persona-builder-status".to_string(),
        )))
        .await
        .expect("non-PersonaBuilder fixture conversation");
    let temp = tempfile::tempdir().expect("persona ingest status temp directory");
    let app_data_dir = temp.path().join("app-data");

    let error = get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: conversation.id.as_str().to_string(),
        },
        &state,
        true,
        &app_data_dir,
    )
    .await
    .expect_err("non-PersonaBuilder conversation must reject status lookup");

    assert_eq!(
        error,
        "Persona context ingestion requires a PersonaBuilder conversation"
    );
}

#[tokio::test]
async fn persona_ingest_status_command_reports_not_live_without_ingested_files() {
    let state = AppState::new_test();
    let conversation = create_persona_builder_conversation_for_state(
        CreatePersonaBuilderConversationInput {
            project_id: "project-persona-builder-status-empty".to_string(),
            source_persona_id: None,
        },
        &state,
        true,
    )
    .await
    .expect("PersonaBuilder conversation should persist before status lookup");
    let temp = tempfile::tempdir().expect("persona ingest status temp directory");
    let app_data_dir = temp.path().join("app-data");

    let status = get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: conversation.id,
        },
        &state,
        true,
        &app_data_dir,
    )
    .await
    .expect("PersonaBuilder status lookup should succeed without ingest storage");

    assert!(!status.live);
}

#[tokio::test]
async fn persona_ingest_status_command_reports_live_after_ingesting_a_file() {
    let state = AppState::new_test();
    let conversation = create_persona_builder_conversation_for_state(
        CreatePersonaBuilderConversationInput {
            project_id: "project-persona-builder-status-live".to_string(),
            source_persona_id: None,
        },
        &state,
        true,
    )
    .await
    .expect("PersonaBuilder conversation should persist before status lookup");
    let temp = tempfile::tempdir().expect("persona ingest status temp directory");
    let app_data_dir = temp.path().join("app-data");
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id,
    );
    fs::create_dir_all(&ingest_root).expect("create PersonaBuilder ingest root");
    fs::write(ingest_root.join("context.md"), "Persona context\n")
        .expect("write app-owned PersonaBuilder context");

    let status = get_persona_builder_ingest_status_for_state(
        PersonaBuilderIngestStatusInput {
            conversation_id: conversation.id,
        },
        &state,
        true,
        &app_data_dir,
    )
    .await
    .expect("PersonaBuilder status lookup should succeed after ingesting context");

    assert!(status.live);
}
