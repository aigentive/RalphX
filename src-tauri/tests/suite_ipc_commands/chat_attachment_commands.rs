use std::sync::Arc;

use ralphx_lib::application::builder_attachment_materializer::materialize_builder_attachment;
use ralphx_lib::application::chat_attachment_service::ChatAttachmentService;
use ralphx_lib::application::chat_service::format_attachments_for_agent;
use ralphx_lib::application::standalone_workspace::{create_workspace, resolve_workspace};
use ralphx_lib::application::{AppPaths, AppState};
use ralphx_lib::commands::chat_attachment_commands::{
    upload_chat_attachment_for_state, UploadChatAttachmentInput,
};
use ralphx_lib::domain::entities::{AgentConversationWorkspaceMode, ChatConversation, ProjectId};
use ralphx_lib::error::AppError;

fn builder_state() -> (tempfile::TempDir, AppState, ChatConversation) {
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("temp directory");
    let app_data_dir = temp.path().join("app-data");
    std::fs::create_dir(&app_data_dir).expect("create app data");
    let mut state = AppState::new_test();
    state.app_paths = AppPaths::new(app_data_dir, None);
    state.attachment_storage_path = state.app_paths.attachment_storage_path();
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    (temp, state, conversation)
}

#[tokio::test]
async fn builder_text_attachment_is_materialized_once_and_prompt_references_path_without_inline() {
    let (_temp, state, conversation) = builder_state();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed builder");
    let body = b"private builder context marker";
    let created = upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "notes.txt".to_string(),
            file_data: body.to_vec(),
            mime_type: Some("text/plain".to_string()),
        },
        &state,
    )
    .await
    .expect("text attachment uploads");
    let attachments = state
        .chat_attachment_repo
        .find_by_conversation_id(&conversation.id)
        .await
        .expect("list attachments");
    assert_eq!(attachments.len(), 1);
    let workspace = resolve_workspace(state.app_paths.app_data_dir(), &conversation.id.as_str())
        .expect("attach-time materializer creates legacy workspace");
    let prompt = format_attachments_for_agent(
        &attachments,
        conversation.agent_mode,
        Some(state.app_paths.app_data_dir()),
    )
    .await
    .expect("format builder attachment");
    assert!(prompt.contains("fs_read_file"));
    assert!(prompt.contains(workspace.to_string_lossy().as_ref()));
    assert!(!prompt.contains("private builder context marker"));

    let first_paths = std::fs::read_dir(workspace.join("attachments"))
        .expect("attachment materialization directory")
        .map(|entry| entry.expect("entry").path())
        .collect::<Vec<_>>();
    ralphx_lib::application::builder_attachment_materializer::sync_builder_attachments(
        state.app_paths.app_data_dir(),
        &state.attachment_storage_path,
        &conversation.id,
        Arc::clone(&state.chat_attachment_repo),
    )
    .await
    .expect("repeat sync is idempotent");
    let second_paths = std::fs::read_dir(workspace.join("attachments"))
        .expect("attachment materialization directory")
        .map(|entry| entry.expect("entry").path())
        .collect::<Vec<_>>();
    assert_eq!(
        first_paths, second_paths,
        "repeat sync must not duplicate files"
    );
    assert_eq!(created.conversation_id, conversation.id.as_str());
}

#[tokio::test]
async fn builder_attachment_render_fails_when_materialized_file_is_missing() {
    let (_temp, state, conversation) = builder_state();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed builder");
    upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "notes.txt".to_string(),
            file_data: b"materialized then removed".to_vec(),
            mime_type: Some("text/plain".to_string()),
        },
        &state,
    )
    .await
    .expect("text attachment uploads");
    let attachment = state
        .chat_attachment_repo
        .find_by_conversation_id(&conversation.id)
        .await
        .expect("list attachments")
        .into_iter()
        .next()
        .expect("uploaded attachment");
    let materialized = ralphx_lib::application::builder_attachment_materializer::materialized_builder_attachment_path(
        state.app_paths.app_data_dir(),
        &attachment,
    )
    .expect("materialized attachment path");
    // codeql[rust/path-injection]
    std::fs::remove_file(&materialized).expect("remove materialized attachment");

    let error = format_attachments_for_agent(
        &[attachment],
        conversation.agent_mode,
        Some(state.app_paths.app_data_dir()),
    )
    .await
    .expect_err("a missing materialized file must abort the builder send");

    assert!(
        error.contains("builder attachment"),
        "missing-file failure must retain typed materialization context: {error}"
    );
    assert!(
        !error.contains("<file_path>"),
        "the failed render must not return a dangling prompt path"
    );
}

#[tokio::test]
async fn builder_binary_attachment_is_rejected_before_storage_with_typed_actionable_error() {
    let (_temp, state, conversation) = builder_state();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed builder");
    let error = upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "image.png".to_string(),
            file_data: vec![0, 159, 146, 150],
            mime_type: Some("image/png".to_string()),
        },
        &state,
    )
    .await
    .expect_err("binary builder attachment must fail");
    assert!(matches!(error, AppError::PersonaBuilderTextAttachmentOnly));
    assert_eq!(
        error.to_string(),
        "The persona builder can only read text context — PDFs/images aren't supported"
    );
    assert!(state
        .chat_attachment_repo
        .find_by_conversation_id(&conversation.id)
        .await
        .expect("list attachments")
        .is_empty());
}

#[tokio::test]
async fn non_builder_attachment_prompt_keeps_exact_inline_format() {
    let (_temp, state, mut conversation) = builder_state();
    conversation.agent_mode = None;
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed ordinary conversation");
    upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "notes.txt".to_string(),
            file_data: b"ordinary inline context".to_vec(),
            mime_type: Some("text/plain".to_string()),
        },
        &state,
    )
    .await
    .expect("ordinary attachment uploads");
    let attachments = state
        .chat_attachment_repo
        .find_by_conversation_id(&conversation.id)
        .await
        .expect("list attachments");
    let prompt = format_attachments_for_agent(&attachments, None, None)
        .await
        .expect("format ordinary attachment");
    assert_eq!(
        prompt,
        "\n\n<attachments>\n<attachment>\n<filename>notes.txt</filename>\n<mime_type>text/plain</mime_type>\n<content>\nordinary inline context\n</content>\n</attachment>\n</attachments>"
    );
}

#[tokio::test]
async fn builder_attachment_materialization_rejects_workspace_symlink_escape_before_write() {
    use std::os::unix::fs::symlink;

    let (temp, state, conversation) = builder_state();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed builder");
    let attachment = ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    )
    .upload(
        &conversation.id,
        "escape.txt",
        b"must stay contained",
        Some("text/plain".to_string()),
    )
    .await
    .expect("seed stored attachment");
    let workspace = create_workspace(state.app_paths.app_data_dir(), &conversation.id.as_str())
        .expect("create workspace");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).expect("create outside target");
    symlink(&outside, workspace.join("attachments")).expect("seed malicious symlink");

    let error = materialize_builder_attachment(
        state.app_paths.app_data_dir(),
        &state.attachment_storage_path,
        &attachment,
    )
    .expect_err("symlinked workspace attachment root must fail closed");
    assert!(matches!(error, AppError::Validation(_)));
    assert_eq!(
        std::fs::read_dir(&outside)
            .expect("read outside target")
            .count(),
        0,
        "containment rejection must happen before creating any outside entry"
    );
}
