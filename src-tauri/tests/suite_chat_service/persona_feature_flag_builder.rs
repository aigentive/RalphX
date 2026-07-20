use super::persona_feature_flag_support::*;

#[test]
fn persona_flag_override_chat_service_keeps_builder_override_and_live_default() {
    let _reset = PersonaFlagOverrideReset;
    set_agent_personas_override(Some(true));
    let state = AppState::new_test();

    assert!(persona_flag_override_chat_service(&state).persona_feature_enabled_for_test());
    assert!(
        !persona_flag_override_chat_service(&state)
            .with_persona_feature_enabled(false)
            .persona_feature_enabled_for_test(),
        "the explicit test seam must override the live feature flag"
    );
}

#[tokio::test]
async fn persona_builder_send_no_longer_requires_live_ingest() {
    let temp = tempfile::tempdir().expect("persona builder send temp directory");
    let app_data_dir = temp.path().join("app-data");
    let project_directory = temp.path().join("project");
    fs::create_dir_all(&project_directory).expect("create persona builder project directory");

    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build persona builder mock app");
    let state = app.state::<AppState>();
    let project = state
        .project_repo
        .create(Project::new(
            "Persona Builder Send".to_string(),
            project_directory.to_string_lossy().into_owned(),
        ))
        .await
        .expect("create persona builder project");
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create persona builder conversation");
    let service = state
        .build_chat_service_for_runtime::<tauri::test::MockRuntime>(
            None,
            Some(app.handle().clone()),
        )
        .with_persona_feature_enabled(true);
    let options = SendMessageOptions {
        conversation_id_override: Some(conversation.id),
        ..Default::default()
    };

    let without_ingest = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Draft a focused reviewer persona.",
            options.clone(),
        )
        .await;
    assert!(
        !matches!(without_ingest, Err(ChatServiceError::PersonaUnavailable(_))),
        "Phase 5 retires the ingest-liveness send gate unconditionally"
    );

    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id.as_str(),
    );
    fs::create_dir_all(&ingest_root).expect("create live persona builder ingest root");
    fs::write(ingest_root.join("content"), "approved persona context")
        .expect("write live persona builder ingest content");

    let after_ingest = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Draft a focused reviewer persona.",
            options,
        )
        .await;
    assert!(
        !matches!(after_ingest, Err(ChatServiceError::PersonaUnavailable(_))),
        "legacy ingest presence must not change the retired gate's behavior"
    );
}

#[tokio::test]
async fn persona_builder_send_does_not_reintroduce_the_retired_ingest_gate() {
    let temp = tempfile::tempdir().expect("bound persona builder send temp directory");
    let app_data_dir = temp.path().join("app-data");
    let project_directory = temp.path().join("project");
    fs::create_dir_all(&project_directory).expect("create bound builder project directory");

    let mut initial_state = AppState::new_test();
    initial_state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    let app = tauri::test::mock_builder()
        .manage(initial_state)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("build bound persona builder mock app");
    let state = app.state::<AppState>();
    let project = state
        .project_repo
        .create(Project::new(
            "Bound Persona Builder Send".to_string(),
            project_directory.to_string_lossy().into_owned(),
        ))
        .await
        .expect("create bound persona builder project");
    let draft_id = PersonaId::new();
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    conversation.builder_draft_id = Some(draft_id.as_str().to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("create bound persona builder conversation");
    let now = chrono::Utc::now();
    state
        .persona_repo
        .create(Persona {
            id: draft_id.clone(),
            artifact_id: None,

            project_id: None,
            slug: "send-bound-draft".to_string(),
            name: "send-bound-draft".to_string(),
            description: "Bound send fixture".to_string(),
            content: "---\nname: send-bound-draft\nkind: persona\ndescription: Bound send fixture\n---\nBody".to_string(),
            status: PersonaStatus::Draft,
            version: 1,
            content_hash: "bound-send-hash".to_string(),
            source_session_id: Some(conversation.id.as_str().to_string()),
            source_persona_id: None,
            source_content_hash: None,
            source_json: "{}".to_string(),
            created_at: now,
            updated_at: now,
        })
        .await
        .expect("create live bound draft");
    let service = state
        .build_chat_service_for_runtime::<tauri::test::MockRuntime>(
            None,
            Some(app.handle().clone()),
        )
        .with_persona_feature_enabled(true);
    let options = SendMessageOptions {
        conversation_id_override: Some(conversation.id),
        ..Default::default()
    };

    let with_draft = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "Revise the bound persona without filesystem context.",
            options.clone(),
        )
        .await;
    assert!(
        !matches!(with_draft, Err(ChatServiceError::PersonaUnavailable(_))),
        "a valid bound Draft must pass every normal send guard"
    );

    state
        .persona_repo
        .set_status(&draft_id, PersonaStatus::Active)
        .await
        .expect("make bound row non-Draft");
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(&app_data_dir),
        &conversation.id.as_str(),
    );
    fs::create_dir_all(&ingest_root).expect("create ingest root beside non-Draft binding");
    fs::write(ingest_root.join("context"), "Must not mask invalid binding")
        .expect("write ingest context beside non-Draft binding");
    let non_draft = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "This send must continue past the retired ingest gate.",
            options.clone(),
        )
        .await;
    assert!(
        !matches!(non_draft, Err(ChatServiceError::PersonaUnavailable(_))),
        "draft status is enforced by draft-write paths, not the retired send gate"
    );

    state.persona_repo.delete(&draft_id).await.unwrap();
    let dangling = service
        .send_message(
            ChatContextType::Project,
            project.id.as_str(),
            "This dangling binding must also continue past the retired gate.",
            options,
        )
        .await;
    assert!(
        matches!(
            dangling,
            Err(ChatServiceError::PersonaUnavailable(ref message))
                if message != "[Persona unavailable: PersonaBuilder requires ingested context or a live bound draft]"
        ),
        "a dangling binding remains fail-closed without resurrecting the ingest-era reason: {dangling:?}"
    );
}
