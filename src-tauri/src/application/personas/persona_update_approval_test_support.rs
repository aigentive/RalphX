#![cfg(test)]

use std::sync::Arc;

use super::{PersonaService, SavePersonaDraftInput};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversation, Persona, ProjectId,
};
use crate::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use crate::testing::SqliteTestDb;

pub(super) fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Update approval test\n---\n{body}")
}

pub(super) fn sqlite_service(db: &SqliteTestDb) -> PersonaService {
    let shared = db.shared_conn();
    PersonaService::new(
        DbConnection::from_shared(Arc::clone(&shared)),
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared))),
        Arc::new(SqliteChatConversationRepository::from_shared(shared)),
    )
}

pub(super) async fn create_active(service: &PersonaService, slug: &str) -> Persona {
    let draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: None,
                slug: slug.to_string(),
                content: persona_content(slug, "Initial source body"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .expect("source draft should create");
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("source draft should approve")
}

pub(super) async fn create_active_in_project(
    service: &PersonaService,
    slug: &str,
    project_id: &ProjectId,
) -> Persona {
    let draft = service
        .create_draft(
            true,
            SavePersonaDraftInput {
                project_id: Some(project_id.clone()),
                slug: slug.to_string(),
                content: persona_content(slug, "Project source body"),
                source_session_id: None,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .unwrap();
    service.approve_persona(true, &draft.id).await.unwrap()
}

pub(super) async fn create_builder_conversation(service: &PersonaService) -> ChatConversation {
    let mut conversation = ChatConversation::new_project(ProjectId::new());
    conversation.agent_mode = Some(AgentConversationWorkspaceMode::PersonaBuilder);
    service
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("builder conversation should persist")
}

pub(super) async fn seeded_fixture(
    db: &SqliteTestDb,
    slug: &str,
) -> (PersonaService, Persona, Persona, Vec<ChatConversation>) {
    let service = sqlite_service(db);
    let source = create_active(&service, slug).await;
    let first = create_builder_conversation(&service).await;
    let second = create_builder_conversation(&service).await;
    let draft = service
        .create_bound_draft(
            true,
            &first.id,
            SavePersonaDraftInput {
                project_id: source.project_id.clone(),
                slug: source.slug.clone(),
                content: persona_content(slug, "Builder revision"),
                source_session_id: Some(first.id.as_str().to_string()),
                source_persona_id: Some(source.id.clone()),
                source_content_hash: Some(source.content_hash.clone()),
            },
        )
        .await
        .expect("seeded draft should create and bind");
    service
        .chat_conversation_repo
        .update_builder_draft_binding(&second.id, Some(draft.id.as_str()))
        .await
        .expect("second restored conversation should bind to the same draft");
    (service, source, draft, vec![first, second])
}

pub(super) async fn assert_bindings(
    service: &PersonaService,
    conversations: &[ChatConversation],
    expected: Option<&str>,
) {
    for conversation in conversations {
        let loaded = service
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should exist");
        assert_eq!(loaded.builder_draft_id.as_deref(), expected);
    }
}

pub(super) async fn assert_pending_bindings(
    service: &PersonaService,
    conversations: &[ChatConversation],
    draft_id: &str,
) {
    for conversation in conversations {
        let loaded = service
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .expect("conversation lookup should succeed")
            .expect("conversation should exist");
        assert_eq!(loaded.builder_draft_id.as_deref(), Some(draft_id));
        assert!(loaded.builder_result_persona_id.is_none());
        assert!(loaded.persona_id.is_none());
    }
}

pub(super) async fn assert_finished_bindings(
    service: &PersonaService,
    conversations: &[ChatConversation],
    result_persona_id: &str,
) {
    for conversation in conversations {
        let loaded = service
            .chat_conversation_repo
            .get_by_id(&conversation.id)
            .await
            .unwrap()
            .unwrap();
        assert!(loaded.builder_draft_id.is_none());
        assert_eq!(
            loaded.builder_result_persona_id.as_deref(),
            Some(result_persona_id)
        );
    }
}

pub(super) fn artifact_row(
    db: &SqliteTestDb,
    artifact_id: &crate::domain::entities::ArtifactId,
) -> (Option<String>, String, serde_json::Value) {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT previous_version_id, created_by, metadata_json
             FROM artifacts WHERE id = ?1",
            [artifact_id.as_str()],
            |row| {
                let metadata: String = row.get(2)?;
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    serde_json::from_str(&metadata).unwrap(),
                ))
            },
        )
        .unwrap()
    })
}

pub(super) fn chain_ids(
    db: &SqliteTestDb,
    tip: &crate::domain::entities::ArtifactId,
) -> Vec<String> {
    db.with_connection(|conn| {
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
            .query_map([tip.as_str()], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
}
