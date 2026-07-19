use std::sync::Arc;

use ralphx_lib::application::builder_attachment_materializer::{
    materialize_builder_attachment, remove_materialized_builder_attachment_if_present,
};
use ralphx_lib::application::chat_attachment_service::ChatAttachmentService;
use ralphx_lib::application::chat_service::format_attachments_for_agent;
use ralphx_lib::application::standalone_workspace::{create_workspace, resolve_workspace};
use ralphx_lib::application::{AppPaths, AppState};
use ralphx_lib::commands::chat_attachment_commands::{
    delete_chat_attachment_for_state, upload_chat_attachment_for_state, UploadChatAttachmentInput,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversation, IdeationSessionId, ProjectId,
};
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
async fn persona_builder_text_attachment_is_materialized_once_and_prompt_references_path_without_inline(
) {
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
        conversation.context_type,
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
async fn persona_builder_attachment_render_fails_when_materialized_file_is_missing() {
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
        conversation.context_type,
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
async fn deleting_persona_builder_attachment_removes_materialized_workspace_copy() {
    let (_temp, state, conversation) = builder_state();
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed builder");
    upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "delete-me.txt".to_string(),
            file_data: b"stale workspace content".to_vec(),
            mime_type: Some("text/plain".to_string()),
        },
        &state,
    )
    .await
    .expect("upload builder attachment");
    let attachment = state
        .chat_attachment_repo
        .find_by_conversation_id(&conversation.id)
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let materialized = ralphx_lib::application::builder_attachment_materializer::materialized_builder_attachment_path(
        state.app_paths.app_data_dir(),
        &attachment,
    )
    .expect("builder copy exists");

    delete_chat_attachment_for_state(attachment.id.as_str(), &state)
        .await
        .expect("delete builder attachment");

    assert!(
        !materialized.exists(),
        "builder attachment deletion must remove the workspace copy"
    );
}

#[tokio::test]
async fn deleting_non_builder_attachment_does_not_mutate_builder_workspace_paths() {
    let (_temp, state, mut conversation) = builder_state();
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::Chat);
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed ordinary conversation");
    let created = upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "ordinary.txt".to_string(),
            file_data: b"ordinary attachment".to_vec(),
            mime_type: Some("text/plain".to_string()),
        },
        &state,
    )
    .await
    .expect("upload ordinary attachment");
    let workspaces_root = ralphx_lib::application::standalone_workspace::standalone_workspaces_root(
        state.app_paths.app_data_dir(),
    );

    delete_chat_attachment_for_state(created.id, &state)
        .await
        .expect("delete ordinary attachment");

    assert!(
        !workspaces_root.exists(),
        "ordinary deletion must not create or inspect a builder workspace"
    );
}

#[tokio::test]
async fn invalid_context_persona_builder_never_materializes_or_removes_workspace_copies() {
    let (_temp, state, _) = builder_state();
    let mut conversation = ChatConversation::new_ideation(IdeationSessionId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .expect("seed invalid-context builder row");

    let created = upload_chat_attachment_for_state(
        UploadChatAttachmentInput {
            conversation_id: conversation.id.as_str(),
            file_name: "invalid-builder.txt".to_string(),
            file_data: b"must remain an ordinary attachment".to_vec(),
            mime_type: Some("text/plain".to_string()),
        },
        &state,
    )
    .await
    .expect("invalid builder identity should retain ordinary attachment behavior");
    let workspaces_root = ralphx_lib::application::standalone_workspace::standalone_workspaces_root(
        state.app_paths.app_data_dir(),
    );
    assert!(
        !workspaces_root.exists(),
        "an unsupported context must not acquire builder workspace write authority"
    );

    let attachment = state
        .chat_attachment_repo
        .get_by_id(&ralphx_lib::domain::entities::ChatAttachmentId::from_string(&created.id))
        .await
        .expect("load uploaded attachment")
        .expect("uploaded attachment should exist");
    materialize_builder_attachment(
        state.app_paths.app_data_dir(),
        &state.attachment_storage_path,
        &attachment,
    )
    .expect("prepare a copy that invalid-context deletion must not touch");
    let materialized = ralphx_lib::application::builder_attachment_materializer::materialized_builder_attachment_path(
        state.app_paths.app_data_dir(),
        &attachment,
    )
    .expect("materialized fixture path");

    delete_chat_attachment_for_state(created.id, &state)
        .await
        .expect("delete ordinary attachment");

    assert!(
        materialized.exists(),
        "an unsupported context must not acquire builder workspace delete authority"
    );
}

#[tokio::test]
async fn persona_builder_binary_attachment_is_rejected_before_storage_with_typed_actionable_error()
{
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
    let prompt = format_attachments_for_agent(&attachments, ChatContextType::Project, None, None)
        .await
        .expect("format ordinary attachment");
    assert_eq!(
        prompt,
        "\n\n<attachments>\n<attachment>\n<filename>notes.txt</filename>\n<mime_type>text/plain</mime_type>\n<content>\nordinary inline context\n</content>\n</attachment>\n</attachments>"
    );
}

#[tokio::test]
async fn persona_builder_attachment_materialization_rejects_workspace_symlink_escape_before_write()
{
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

#[tokio::test]
async fn persona_builder_attachment_removal_is_idempotent_and_rejects_symlinks() {
    use std::os::unix::fs::symlink;

    let (temp, state, conversation) = builder_state();
    let attachment = ChatAttachmentService::new(
        Arc::clone(&state.chat_attachment_repo),
        state.attachment_storage_path.clone(),
    )
    .upload(
        &conversation.id,
        "remove-safely.txt",
        b"source attachment must survive",
        Some("text/plain".to_string()),
    )
    .await
    .expect("seed stored attachment");

    remove_materialized_builder_attachment_if_present(state.app_paths.app_data_dir(), &attachment)
        .expect("a missing builder workspace must be an idempotent no-op");
    create_workspace(state.app_paths.app_data_dir(), &conversation.id.as_str())
        .expect("create empty builder workspace");
    remove_materialized_builder_attachment_if_present(state.app_paths.app_data_dir(), &attachment)
        .expect("a missing materialized file must be an idempotent no-op");

    let materialized = materialize_builder_attachment(
        state.app_paths.app_data_dir(),
        &state.attachment_storage_path,
        &attachment,
    )
    .expect("materialize contained attachment fixture");
    // The production helper returned this canonical, app-owned workspace path.
    // codeql[rust/path-injection]
    std::fs::remove_file(&materialized).expect("replace materialized file with a symlink");
    let outside = temp.path().join("outside-source.txt");
    std::fs::write(&outside, "outside content must survive").expect("seed outside target");
    symlink(&outside, &materialized).expect("seed malicious materialized symlink");

    let error = remove_materialized_builder_attachment_if_present(
        state.app_paths.app_data_dir(),
        &attachment,
    )
    .expect_err("materialized symlinks must fail closed before deletion");

    assert!(matches!(error, AppError::Validation(_)));
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read outside target"),
        "outside content must survive",
        "symlink rejection must not delete or mutate its outside target"
    );
    assert!(
        materialized.is_symlink(),
        "fail-closed deletion must leave the rejected symlink untouched"
    );

    let destination_error = materialize_builder_attachment(
        state.app_paths.app_data_dir(),
        &state.attachment_storage_path,
        &attachment,
    )
    .expect_err("materialization must reject an existing destination symlink");
    assert!(matches!(destination_error, AppError::Validation(_)));
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read destination target"),
        "outside content must survive",
        "destination rejection must not overwrite its outside target"
    );

    // The production helper returned this canonical, app-owned workspace path.
    // codeql[rust/path-injection]
    std::fs::remove_file(&materialized).expect("remove rejected destination symlink");
    let source = std::path::PathBuf::from(&attachment.file_path);
    let canonical_storage = state
        .attachment_storage_path
        .canonicalize()
        .expect("canonicalize app-owned attachment storage");
    let canonical_source = source
        .canonicalize()
        .expect("canonicalize stored attachment fixture");
    assert!(
        canonical_source.starts_with(&canonical_storage),
        "source fixture must remain under app-owned storage"
    );
    // The source was validated under the canonical app-owned storage root.
    // codeql[rust/path-injection]
    std::fs::remove_file(&canonical_source).expect("replace stored source with a symlink");
    // The source was validated under the canonical app-owned storage root.
    // codeql[rust/path-injection]
    symlink(&outside, &canonical_source).expect("seed malicious stored-source symlink");

    let source_error = materialize_builder_attachment(
        state.app_paths.app_data_dir(),
        &state.attachment_storage_path,
        &attachment,
    )
    .expect_err("materialization must reject a symlinked source attachment");
    assert!(matches!(source_error, AppError::Validation(_)));
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read source target"),
        "outside content must survive",
        "source rejection must not mutate its outside target"
    );
}
