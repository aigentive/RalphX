use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tempfile::TempDir;

use super::conversation_folder_reference_service::{
    ConversationFolderReferenceService, FOLDER_REF_SKIPPED_UNAVAILABLE,
};
use crate::domain::entities::{AgentConversationWorkspaceMode, ChatContextType, Project};
use crate::domain::entities::{
    ChatConversationId, ConversationFolderReference, ConversationFolderReferenceId,
};
use crate::domain::repositories::{ConversationFolderReferenceRepository, ProjectRepository};
use crate::error::AppError;
use crate::infrastructure::memory::{
    MemoryConversationFolderReferenceRepository, MemoryProjectRepository,
};
use crate::utils::path_safety::validate_absolute_non_root_path;

struct Fixture {
    _temp: TempDir,
    app_data: PathBuf,
    folder: PathBuf,
    service: ConversationFolderReferenceService,
}

struct FailingListRepository;

#[async_trait]
impl ConversationFolderReferenceRepository for FailingListRepository {
    async fn create_if_below_live_cap(
        &self,
        _reference: ConversationFolderReference,
        _max_live_references: usize,
    ) -> crate::error::AppResult<ConversationFolderReference> {
        unreachable!("test only exercises list_live")
    }

    async fn list_live(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<Vec<ConversationFolderReference>> {
        Err(AppError::Database("simulated list failure".to_string()))
    }

    async fn soft_remove(
        &self,
        _id: &ConversationFolderReferenceId,
        _conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<bool> {
        unreachable!("test only exercises list_live")
    }

    async fn delete_by_conversation_id(
        &self,
        _conversation_id: &ChatConversationId,
    ) -> crate::error::AppResult<()> {
        unreachable!("test only exercises list_live")
    }
}

fn safe_child(root: &Path, name: &str) -> PathBuf {
    validate_absolute_non_root_path(&root.join(name), "folder reference test path")
        .expect("safe test path")
}

fn fixture(max_live: usize) -> Fixture {
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("create temp directory");
    let app_data = safe_child(temp.path(), "app-data");
    let folder = safe_child(temp.path(), "folder");
    std::fs::create_dir(&app_data).expect("create app data");
    std::fs::create_dir(&folder).expect("create referenced folder");
    let service = ConversationFolderReferenceService::new(
        Arc::new(MemoryConversationFolderReferenceRepository::new()),
        app_data.clone(),
        max_live,
    );
    Fixture {
        _temp: temp,
        app_data,
        folder,
        service,
    }
}

#[tokio::test]
async fn folder_reference_validation_matrix_rejects_unsafe_paths_and_stores_canonical() {
    let fixture = fixture(5);
    let conversation_id = ChatConversationId::new();

    for unsafe_path in [
        PathBuf::from("relative"),
        PathBuf::from("/var/../etc"),
        PathBuf::from("/"),
    ] {
        let result = fixture
            .service
            .add(conversation_id, &unsafe_path, "Unsafe".to_string())
            .await;
        assert!(
            result.is_err(),
            "unsafe path should be rejected: {unsafe_path:?}"
        );
    }

    let app_data_child = safe_child(&fixture.app_data, "private");
    std::fs::create_dir(&app_data_child).expect("create app data child");
    assert!(fixture
        .service
        .add(conversation_id, &app_data_child, "Private".to_string())
        .await
        .is_err());
    assert!(fixture
        .service
        .add(conversation_id, &fixture.folder, "bad\nname".to_string())
        .await
        .is_err());

    #[cfg(unix)]
    {
        let symlink = safe_child(fixture._temp.path(), "folder-link");
        std::os::unix::fs::symlink(&fixture.folder, &symlink).expect("create symlink");
        assert!(fixture
            .service
            .add(conversation_id, &symlink, "Link".to_string())
            .await
            .is_err());
    }

    let created = fixture
        .service
        .add(conversation_id, &fixture.folder, "Folder".to_string())
        .await
        .expect("valid folder accepted");
    assert_eq!(
        PathBuf::from(created.folder_path),
        std::fs::canonicalize(&fixture.folder).expect("canonical folder")
    );
}

#[tokio::test]
async fn folder_reference_soft_cap_counts_only_live_rows() {
    let fixture = fixture(5);
    let conversation_id = ChatConversationId::new();
    let mut references = Vec::new();
    for index in 0..5 {
        let folder = safe_child(fixture._temp.path(), &format!("folder-{index}"));
        std::fs::create_dir(&folder).expect("create distinct referenced folder");
        references.push(
            fixture
                .service
                .add(conversation_id, &folder, format!("Folder {index}"))
                .await
                .expect("first five references succeed"),
        );
    }

    let sixth = fixture
        .service
        .add(conversation_id, &fixture.folder, "Sixth".to_string())
        .await;
    assert!(matches!(
        sixth,
        Err(AppError::ConversationFolderReferenceLimit { limit: 5, .. })
    ));

    fixture
        .service
        .remove(&references[0].id, &conversation_id)
        .await
        .expect("soft remove succeeds");
    fixture
        .service
        .add(conversation_id, &fixture.folder, "Replacement".to_string())
        .await
        .expect("replacement succeeds after soft removal");
    assert_eq!(
        fixture
            .service
            .list_live(&conversation_id)
            .await
            .expect("list live")
            .len(),
        5
    );
}

#[tokio::test]
async fn duplicate_live_folder_reference_is_rejected_and_readd_after_remove_succeeds() {
    let fixture = fixture(1);
    let conversation_id = ChatConversationId::new();
    let first = fixture
        .service
        .add(conversation_id, &fixture.folder, "Folder".to_string())
        .await
        .expect("first add succeeds");
    let duplicate = fixture
        .service
        .add(conversation_id, &fixture.folder, "Duplicate".to_string())
        .await;
    assert!(matches!(
        duplicate,
        Err(AppError::ConversationFolderReferenceDuplicate { .. })
    ));
    fixture
        .service
        .remove(&first.id, &conversation_id)
        .await
        .expect("remove first reference");
    fixture
        .service
        .add(conversation_id, &fixture.folder, "Re-added".to_string())
        .await
        .expect("re-add after soft remove succeeds");
}

#[tokio::test]
async fn folder_reference_prompt_block_is_absent_empty_and_xml_escapes_metadata() {
    let fixture = fixture(5);
    let conversation_id = ChatConversationId::new();
    assert_eq!(
        fixture
            .service
            .render_prompt_block(&conversation_id)
            .await
            .expect("empty render"),
        None
    );

    fixture
        .service
        .add(
            conversation_id,
            &fixture.folder,
            "<script>&\"more".to_string(),
        )
        .await
        .expect("add reference");
    let block = fixture
        .service
        .render_prompt_block(&conversation_id)
        .await
        .expect("render block")
        .expect("non-empty block");
    assert!(block.contains("<referenced_folders>"));
    assert!(block.contains("&lt;script&gt;&amp;&quot;more"));
    assert!(!block.contains("<script>"));
}

#[tokio::test]
async fn folder_reference_read_time_revalidation_excludes_deleted_root_with_diagnostic() {
    let fixture = fixture(5);
    let conversation_id = ChatConversationId::new();
    fixture
        .service
        .add(conversation_id, &fixture.folder, "Folder".to_string())
        .await
        .expect("add reference");
    std::fs::remove_dir(&fixture.folder).expect("remove referenced root");

    let validated = fixture
        .service
        .list_live_validated(&conversation_id)
        .await
        .expect("repository read succeeds");
    assert!(validated.references.is_empty());
    assert_eq!(validated.skipped.len(), 1);
    assert_eq!(validated.skipped[0].reason, FOLDER_REF_SKIPPED_UNAVAILABLE);
    assert_eq!(
        fixture
            .service
            .render_prompt_block(&conversation_id)
            .await
            .expect("invalid row is fail-soft"),
        None
    );
}

#[tokio::test]
async fn folder_reference_repository_list_failure_still_aborts() {
    let fixture = fixture(5);
    let service = ConversationFolderReferenceService::new(
        Arc::new(FailingListRepository),
        fixture.app_data.clone(),
        5,
    );
    let error = service
        .list_live_validated(&ChatConversationId::new())
        .await
        .expect_err("repository errors must remain fail-closed");
    assert!(matches!(error, AppError::Database(message) if message.contains("simulated")));
}

#[tokio::test]
async fn one_invalid_reference_does_not_hide_or_leak_the_valid_reference() {
    let fixture = fixture(5);
    let good_folder = safe_child(fixture._temp.path(), "good-folder");
    std::fs::create_dir(&good_folder).expect("create good folder");
    let conversation_id = ChatConversationId::new();
    fixture
        .service
        .add(conversation_id, &fixture.folder, "Bad".to_string())
        .await
        .expect("add soon-to-be-invalid folder");
    fixture
        .service
        .add(conversation_id, &good_folder, "Good".to_string())
        .await
        .expect("add good folder");
    std::fs::remove_dir(&fixture.folder).expect("remove bad folder");

    let validated = fixture
        .service
        .list_live_validated(&conversation_id)
        .await
        .expect("repository read succeeds");
    assert_eq!(validated.references.len(), 1);
    assert_eq!(
        validated.references[0].folder_path,
        good_folder.to_string_lossy()
    );
    assert_eq!(validated.skipped.len(), 1);
    let prompt = fixture
        .service
        .render_prompt_block(&conversation_id)
        .await
        .expect("render valid subset")
        .expect("good reference remains");
    assert!(prompt.contains(&good_folder.to_string_lossy().to_string()));
    assert!(!prompt.contains(&fixture.folder.to_string_lossy().to_string()));
}

#[tokio::test]
async fn app_data_exact_and_ancestor_are_rejected_while_sibling_is_accepted() {
    let fixture = fixture(5);
    let conversation_id = ChatConversationId::new();
    let ancestor = fixture
        .app_data
        .parent()
        .expect("app data has parent")
        .to_path_buf();
    let sibling = safe_child(&ancestor, "app-data-sibling");
    std::fs::create_dir(&sibling).expect("create sibling");

    assert!(fixture
        .service
        .add(conversation_id, &fixture.app_data, "Exact".to_string())
        .await
        .is_err());
    assert!(fixture
        .service
        .add(conversation_id, &ancestor, "Ancestor".to_string())
        .await
        .is_err());
    fixture
        .service
        .add(conversation_id, &sibling, "Sibling".to_string())
        .await
        .expect("sibling accepted");
}

#[tokio::test]
async fn missing_app_data_root_rejects_registration() {
    let fixture = fixture(5);
    std::fs::remove_dir(&fixture.app_data).expect("remove app data root");
    let error = fixture
        .service
        .add(
            ChatConversationId::new(),
            &fixture.folder,
            "Folder".to_string(),
        )
        .await
        .expect_err("missing app data canonicalization must fail closed");
    assert!(matches!(
        error,
        AppError::ConversationFolderReferenceAppDataUnavailable { .. }
    ));
}

#[tokio::test]
async fn project_folder_reference_roots_append_without_replacing_project_root() {
    let fixture = fixture(5);
    let second_folder = safe_child(fixture._temp.path(), "folder-two");
    let project_folder = safe_child(fixture._temp.path(), "project");
    let working_folder = safe_child(fixture._temp.path(), "workspace");
    std::fs::create_dir(&second_folder).expect("create second folder");
    std::fs::create_dir(&project_folder).expect("create project folder");
    std::fs::create_dir(&working_folder).expect("create working folder");
    let conversation_id = ChatConversationId::new();
    fixture
        .service
        .add(conversation_id, &fixture.folder, "One".to_string())
        .await
        .expect("add first folder");
    fixture
        .service
        .add(conversation_id, &second_folder, "Two".to_string())
        .await
        .expect("add second folder");
    let folder_repo = Arc::new(MemoryConversationFolderReferenceRepository::new());
    let folder_service =
        ConversationFolderReferenceService::new(folder_repo.clone(), fixture.app_data.clone(), 5);
    folder_service
        .add(conversation_id, &fixture.folder, "One".to_string())
        .await
        .expect("seed first shared folder");
    folder_service
        .add(conversation_id, &second_folder, "Two".to_string())
        .await
        .expect("seed second shared folder");
    std::fs::remove_dir(&fixture.folder).expect("invalidate first shared folder");
    let prompt = folder_service
        .render_prompt_block(&conversation_id)
        .await
        .expect("render valid subset")
        .expect("second folder remains");
    assert!(prompt.contains(&format!("path=\"{}\"", second_folder.to_string_lossy())));
    assert!(!prompt.contains(&format!("path=\"{}\"", fixture.folder.to_string_lossy())));
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let project = Project::new(
        "Folder Roots".to_string(),
        project_folder.to_string_lossy().into_owned(),
    );
    let project_id = project.id.clone();
    project_repo.create(project).await.expect("seed project");
    let conversation_id_string = conversation_id.as_str();

    let roots = crate::application::chat_service::chat_service_context::resolve_mcp_filesystem_read_roots_with_folder_references(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo.clone(),
        &working_folder,
        Some(AgentConversationWorkspaceMode::Edit),
        Some(&conversation_id_string),
        Some(&fixture.app_data),
        &fixture.app_data,
        folder_repo.clone(),
    )
    .await
    .expect("resolve project roots");
    assert!(roots.contains(&project_folder));
    assert!(!roots.contains(&fixture.folder));
    assert!(roots.contains(&second_folder));

    let builder_roots = crate::application::chat_service::chat_service_context::resolve_mcp_filesystem_read_roots_with_folder_references(
        ChatContextType::Project,
        Some(project_id.as_str()),
        project_repo,
        &working_folder,
        Some(AgentConversationWorkspaceMode::PersonaBuilder),
        Some(&conversation_id_string),
        Some(&fixture.app_data),
        &fixture.app_data,
        folder_repo.clone(),
    )
    .await
    .expect("resolve builder roots");
    assert!(!builder_roots.contains(&fixture.folder));
    assert!(!builder_roots.contains(&second_folder));
}

#[test]
fn folder_reference_overlay_is_ordered_after_persona_and_reaches_codex_composition() {
    let folder_block = "<referenced_folders>\n  <folder path=\"/safe\" display_name=\"Safe\" />\n</referenced_folders>";
    let combined =
        crate::infrastructure::agents::persona_overlay::render_ordered_prompt_overlay_block(
            Some("<ralphx_agent_persona>Persona</ralphx_agent_persona>"),
            Some(folder_block),
        )
        .expect("ordered overlay");
    assert!(combined.find("<ralphx_agent_persona>") < combined.find("<referenced_folders>"));

    let plugin_dir = validate_absolute_non_root_path(
        &std::env::current_dir()
            .expect("current directory")
            .join("plugins/app"),
        "Codex folder reference overlay test plugin directory",
    )
    .expect("safe plugin directory");
    let composition =
        crate::infrastructure::agents::codex::compose_codex_prompt_for_profile_with_outcome(
            "User turn",
            Some(&plugin_dir),
            Some("ralphx-chat-project"),
            None,
            Some(&combined),
        );
    assert!(composition.prompt.contains(folder_block));
}
