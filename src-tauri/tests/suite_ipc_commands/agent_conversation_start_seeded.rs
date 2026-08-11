use super::agent_conversation_start_support::*;

#[tokio::test]
async fn abort_seeded_agent_conversation_removes_all_pre_start_state() {
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("abort fixture directory should be created");
    let app_data_dir = temp.path().join("app-data");
    let db = SqliteTestDb::new("abort_seeded_persona_chain");
    let shared = db.shared_conn();
    let mut state = AppState::new_test();
    state.db = DbConnection::from_shared(Arc::clone(&shared));
    state.persona_repo = Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared)));
    state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(shared));
    state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    state.attachment_storage_path = state.app_paths.attachment_storage_path();

    let conversation =
        ChatConversation::new_project(ProjectId::from_string("abort-seeded-project".to_string()));
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seeded conversation should persist");
    let persona_service = PersonaService::new(
        state.db.clone(),
        state.persona_repo.clone(),
        state.chat_conversation_repo.clone(),
    );
    let draft = persona_service
        .create_bound_draft(
            true,
            &conversation.id,
            SavePersonaDraftInput {
                project_id: None,
                slug: "abort-bound-draft".to_string(),
                content: "---\nname: abort-bound-draft\nkind: persona\ndescription: Abort fixture\n---\nNever started."
                    .to_string(),
                source_session_id: Some(conversation.id.as_str().to_string()),
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("bound draft and artifact chain should persist");
    let artifact_ids = db.with_connection(|conn| {
        let mut statement = conn
            .prepare(
                "WITH RECURSIVE chain(id, previous_version_id) AS (
                     SELECT id, previous_version_id FROM artifacts WHERE id = ?1
                     UNION ALL
                     SELECT artifacts.id, artifacts.previous_version_id
                     FROM artifacts JOIN chain ON artifacts.id = chain.previous_version_id
                 ) SELECT id FROM chain",
            )
            .unwrap();
        statement
            .query_map([draft.artifact_id.as_ref().unwrap().as_str()], |row| {
                row.get(0)
            })
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap()
    });
    assert!(!artifact_ids.is_empty());
    state
        .conversation_folder_reference_repo
        .create_if_below_live_cap(
            ConversationFolderReference::new(
                conversation.id,
                temp.path().join("referenced-folder").to_string_lossy(),
                "referenced-folder",
            ),
            5,
        )
        .await
        .expect("folder reference should persist");
    let attachment_service = ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    );
    let attachment = attachment_service
        .upload(
            &conversation.id,
            "seed.txt",
            b"seed",
            Some("text/plain".to_string()),
        )
        .await
        .expect("seed attachment should upload");
    let workspace = create_workspace(&app_data_dir, &conversation.id.as_str())
        .expect("seed workspace should be created");

    abort_seeded_agent_conversation(&state, &conversation.id)
        .await
        .expect("never-started seed should abort");

    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .conversation_folder_reference_repo
        .list_live(&conversation.id)
        .await
        .unwrap()
        .is_empty());
    assert!(state
        .chat_attachment_repo
        .get_by_id(&attachment.id)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .persona_repo
        .get_by_id(&draft.id)
        .await
        .unwrap()
        .is_none());
    db.with_connection(|conn| {
        for artifact_id in artifact_ids {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM artifacts WHERE id = ?1",
                    [artifact_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "abort must delete every draft artifact row");
        }
    });
    assert!(!workspace.exists(), "aborted workspace must be absent");
}

#[tokio::test]
async fn abort_seeded_agent_conversation_refuses_a_started_conversation_without_cleanup() {
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("abort fixture directory should be created");
    let app_data_dir = temp.path().join("app-data");
    let mut state = AppState::new_test();
    state.app_paths = AppPaths::new(app_data_dir.clone(), None);
    state.attachment_storage_path = state.app_paths.attachment_storage_path();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(ProjectId::from_string(
            "abort-started-project".to_string(),
        )))
        .await
        .expect("started conversation should persist");
    state
        .agent_run_repo
        .create(AgentRun::new(conversation.id))
        .await
        .expect("run proves the conversation started");
    let workspace = create_workspace(&app_data_dir, &conversation.id.as_str())
        .expect("started workspace should be created");

    let error = abort_seeded_agent_conversation(&state, &conversation.id)
        .await
        .expect_err("started conversation must refuse seeded abort");

    assert!(matches!(
        error,
        ralphx_lib::error::AppError::SeededAgentConversationAlreadyStarted { .. }
    ));
    assert!(state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .is_some());
    assert!(state
        .agent_run_repo
        .get_latest_for_conversation(&conversation.id)
        .await
        .unwrap()
        .is_some());
    assert!(workspace.exists(), "refused abort must preserve workspace");
}
