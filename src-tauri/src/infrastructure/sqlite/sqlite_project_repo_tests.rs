use super::*;
use crate::domain::entities::{
    ChatConversation, GitMode, MergeValidationMode, Persona, PersonaId, PersonaStatus,
};
use crate::domain::repositories::{ChatConversationRepository, PersonaRepository};
use crate::infrastructure::sqlite::{SqliteChatConversationRepository, SqlitePersonaRepository};
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite_project_repo_tests")
}

fn create_test_project(name: &str, path: &str) -> Project {
    Project::new(name.to_string(), path.to_string())
}

fn scoped_persona(id: &str, project_id: &ProjectId, status: PersonaStatus) -> Persona {
    let now = Utc::now();
    Persona {
        id: PersonaId::from(id),
        artifact_id: None,

        project_id: Some(project_id.clone()),
        slug: id.to_string(),
        name: id.to_string(),
        description: "deletion sweep fixture".to_string(),
        content: "persona body".to_string(),
        status,
        version: 1,
        content_hash: format!("hash-{id}"),
        source_session_id: None,
        source_persona_id: None,
        source_content_hash: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    }
}

// ==================== CRUD TESTS ====================

#[tokio::test]
async fn test_create_inserts_project_and_returns_it() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = create_test_project("Test Project", "/test/path");

    let result = repo.create(project.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, project.id);
    assert_eq!(created.name, "Test Project");
    assert_eq!(created.working_directory, "/test/path");
}

#[tokio::test]
async fn test_create_restores_archived_project_for_same_working_directory() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());

    let mut archived = create_test_project("Archived Project", "/restore/path");
    archived.merge_validation_mode = MergeValidationMode::Warn;
    archived.github_pr_enabled = false;
    archived.base_branch = Some("main".to_string());
    repo.create(archived.clone()).await.unwrap();
    repo.archive(&archived.id).await.unwrap();

    let mut restored_request = create_test_project("Restored Project", "/restore/path");
    restored_request.base_branch = Some("develop".to_string());
    restored_request.worktree_parent_directory = Some("/tmp/restored-worktrees".to_string());

    let restored = repo.create(restored_request).await.unwrap();

    assert_eq!(restored.id, archived.id);
    assert_eq!(restored.name, "Restored Project");
    assert_eq!(restored.base_branch, Some("develop".to_string()));
    assert_eq!(
        restored.worktree_parent_directory,
        Some("/tmp/restored-worktrees".to_string())
    );
    assert_eq!(restored.merge_validation_mode, MergeValidationMode::Warn);
    assert!(!restored.github_pr_enabled);
    assert!(restored.archived_at.is_none());

    let projects = repo.get_all().await.unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, archived.id);
}

#[tokio::test]
async fn test_get_by_id_retrieves_project_correctly() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = create_test_project("Test Project", "/test/path");

    repo.create(project.clone()).await.unwrap();
    let result = repo.get_by_id(&project.id).await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let found_project = found.unwrap();
    assert_eq!(found_project.id, project.id);
    assert_eq!(found_project.name, "Test Project");
    assert_eq!(found_project.working_directory, "/test/path");
}

#[tokio::test]
async fn test_get_by_id_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let id = ProjectId::new();

    let result = repo.get_by_id(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_all_returns_all_projects() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());

    let project1 = create_test_project("Alpha Project", "/alpha");
    let project2 = create_test_project("Beta Project", "/beta");
    let project3 = create_test_project("Gamma Project", "/gamma");

    repo.create(project3).await.unwrap();
    repo.create(project1).await.unwrap();
    repo.create(project2).await.unwrap();

    let result = repo.get_all().await;

    assert!(result.is_ok());
    let projects = result.unwrap();
    assert_eq!(projects.len(), 3);
    // Should be sorted by name
    assert_eq!(projects[0].name, "Alpha Project");
    assert_eq!(projects[1].name, "Beta Project");
    assert_eq!(projects[2].name, "Gamma Project");
}

#[tokio::test]
async fn test_get_all_returns_empty_for_no_projects() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());

    let result = repo.get_all().await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_update_modifies_project_fields() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let mut project = create_test_project("Original Name", "/original/path");

    repo.create(project.clone()).await.unwrap();

    project.name = "Updated Name".to_string();
    project.working_directory = "/updated/path".to_string();
    project.git_mode = GitMode::Worktree;
    project.base_branch = Some("develop".to_string());

    let update_result = repo.update(&project).await;
    assert!(update_result.is_ok());

    let found = repo.get_by_id(&project.id).await.unwrap().unwrap();
    assert_eq!(found.name, "Updated Name");
    assert_eq!(found.working_directory, "/updated/path");
    assert_eq!(found.git_mode, GitMode::Worktree);
    assert_eq!(found.base_branch, Some("develop".to_string()));
}

#[tokio::test]
async fn test_delete_removes_project_from_database() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = create_test_project("To Delete", "/delete/me");

    repo.create(project.clone()).await.unwrap();

    let delete_result = repo.delete(&project.id).await;
    assert!(delete_result.is_ok());

    let found = repo.get_by_id(&project.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_create_and_retrieve_preserves_all_fields() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());

    let mut project = Project::new("Full Project".to_string(), "/full/path".to_string());
    project.git_mode = GitMode::Worktree;
    project.base_branch = Some("main".to_string());

    repo.create(project.clone()).await.unwrap();
    let found = repo.get_by_id(&project.id).await.unwrap().unwrap();

    assert_eq!(found.id, project.id);
    assert_eq!(found.name, project.name);
    assert_eq!(found.working_directory, project.working_directory);
    assert_eq!(found.git_mode, GitMode::Worktree);
    assert_eq!(found.base_branch, Some("main".to_string()));
}

#[tokio::test]
async fn test_get_by_id_uses_schema_default_when_merge_validation_mode_is_omitted() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let mut project = create_test_project("Schema Default", "/schema/default");
    project.merge_validation_mode = MergeValidationMode::Warn;

    let project_id = project.id.clone();
    db.insert_project_using_schema_defaults(project);

    let stored_mode: String = db.with_connection(|conn| {
        conn.query_row(
            "SELECT merge_validation_mode FROM projects WHERE id = ?1",
            [project_id.as_str()],
            |row| row.get(0),
        )
        .expect("stored merge_validation_mode should exist")
    });
    assert_eq!(stored_mode, "off");

    let found = repo.get_by_id(&project_id).await.unwrap().unwrap();
    assert_eq!(found.merge_validation_mode, MergeValidationMode::Off);
}

// ==================== GET BY WORKING DIRECTORY TESTS ====================

#[tokio::test]
async fn test_get_by_working_directory_returns_project() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = create_test_project("Test Project", "/test/path");

    repo.create(project.clone()).await.unwrap();

    let result = repo.get_by_working_directory("/test/path").await;

    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, project.id);
}

#[tokio::test]
async fn test_get_by_working_directory_returns_none_for_nonexistent() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());
    let project = create_test_project("Test Project", "/test/path");

    repo.create(project).await.unwrap();

    let result = repo.get_by_working_directory("/different/path").await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_working_directory_finds_correct_project() {
    let db = setup_test_db();
    let repo = SqliteProjectRepository::from_shared(db.shared_conn());

    let project1 = create_test_project("Project 1", "/path/one");
    let project2 = create_test_project("Project 2", "/path/two");

    repo.create(project1.clone()).await.unwrap();
    repo.create(project2.clone()).await.unwrap();

    let found = repo.get_by_working_directory("/path/two").await.unwrap();

    assert!(found.is_some());
    assert_eq!(found.unwrap().id, project2.id);
}

#[tokio::test]
async fn delete_with_dependent_sweep_archives_personas_deletes_drafts_and_clears_bindings() {
    let db = setup_test_db();
    let shared = db.shared_conn();
    let repo = SqliteProjectRepository::from_shared(Arc::clone(&shared));
    let persona_repo = SqlitePersonaRepository::from_shared(Arc::clone(&shared));
    let conversation_repo = SqliteChatConversationRepository::from_shared(shared);
    let deleted_project = create_test_project("Deleted", "/deleted");
    let other_project = create_test_project("Other", "/other");
    repo.create(deleted_project.clone()).await.unwrap();
    repo.create(other_project.clone()).await.unwrap();
    let draft = scoped_persona("deleted-draft", &deleted_project.id, PersonaStatus::Draft);
    let active = scoped_persona("deleted-active", &deleted_project.id, PersonaStatus::Active);
    let other = scoped_persona("other-active", &other_project.id, PersonaStatus::Active);
    persona_repo.create(draft.clone()).await.unwrap();
    persona_repo.create(active.clone()).await.unwrap();
    persona_repo.create(other.clone()).await.unwrap();
    db.with_connection(|conn| {
        conn.execute_batch(
            "INSERT INTO artifacts (
                 id, type, name, content_type, content_text, bucket_id, created_by,
                 version, metadata_json
             ) VALUES
                 ('deleted-draft-v1', 'persona', 'Draft', 'inline', 'v1',
                  'persona-library', 'agent', 1, '{}'),
                 ('deleted-draft-v2', 'persona', 'Draft', 'inline', 'v2',
                  'persona-library', 'agent', 2, '{}');
             UPDATE artifacts SET previous_version_id = 'deleted-draft-v1'
             WHERE id = 'deleted-draft-v2';
             UPDATE personas SET artifact_id = 'deleted-draft-v2'
             WHERE id = 'deleted-draft';",
        )
        .unwrap();
    });
    let mut conversation = ChatConversation::new_project(deleted_project.id.clone());
    conversation.persona_id = Some(active.id.to_string());
    conversation.builder_draft_id = Some(draft.id.to_string());
    conversation.builder_result_persona_id = Some(active.id.to_string());
    let conversation = conversation_repo.create(conversation).await.unwrap();

    repo.delete_with_dependent_sweep(&deleted_project.id)
        .await
        .unwrap();

    assert!(persona_repo.get_by_id(&draft.id).await.unwrap().is_none());
    assert_eq!(
        persona_repo
            .get_by_id(&active.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PersonaStatus::Archived
    );
    assert_eq!(
        persona_repo
            .get_by_id(&other.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        PersonaStatus::Active
    );
    let conversation = conversation_repo
        .get_by_id(&conversation.id)
        .await
        .unwrap()
        .unwrap();
    assert!(conversation.persona_id.is_none());
    assert!(conversation.builder_draft_id.is_none());
    assert!(conversation.builder_result_persona_id.is_none());
    db.with_connection(|conn| {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM artifacts
                 WHERE id IN ('deleted-draft-v1', 'deleted-draft-v2')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "project sweep must delete draft artifact chains");
    });
    assert!(repo.get_by_id(&deleted_project.id).await.unwrap().is_none());
    assert!(repo.get_by_id(&other_project.id).await.unwrap().is_some());
}
