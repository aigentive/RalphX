#![cfg(test)]

use std::sync::Arc;

use ralphx_domain::personas::validation::compute_content_hash;

use super::{PersonaService, SavePersonaDraftInput};
use crate::domain::entities::PersonaId;
use crate::error::AppError;
use crate::infrastructure::sqlite::{
    DbConnection, SqliteChatConversationRepository, SqlitePersonaRepository,
};
use crate::testing::SqliteTestDb;

pub(super) fn persona_content(slug: &str, body: &str) -> String {
    format!("---\nname: {slug}\nkind: persona\ndescription: Test persona\n---\n{body}")
}

pub(super) fn persona_artifacts(
    db: &SqliteTestDb,
    persona_id: &PersonaId,
) -> Vec<(String, String)> {
    db.with_connection(|conn| {
        let mut statement = conn
            .prepare(
                "SELECT artifacts.id, artifacts.created_by
                 FROM artifacts
                 JOIN personas ON personas.id = ?1
                 WHERE artifacts.type = 'persona'
                   AND (artifacts.id = personas.artifact_id
                        OR artifacts.id IN (
                            WITH RECURSIVE chain(id) AS (
                                SELECT personas.artifact_id
                                UNION ALL
                                SELECT artifacts.previous_version_id
                                FROM artifacts JOIN chain ON artifacts.id = chain.id
                                WHERE artifacts.previous_version_id IS NOT NULL
                            ) SELECT id FROM chain
                        ))
                 ORDER BY artifacts.version",
            )
            .unwrap();
        statement
            .query_map([persona_id.as_str()], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
}

pub(super) fn artifact_count(db: &SqliteTestDb) -> i64 {
    db.with_connection(|conn| {
        conn.query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))
            .unwrap()
    })
}

pub(super) fn fail_persona_artifact_appends(db: &SqliteTestDb) {
    db.with_connection(|conn| {
        conn.execute_batch(
            "CREATE TRIGGER fail_persona_artifact_append
             BEFORE INSERT ON artifacts WHEN NEW.type = 'persona'
             BEGIN SELECT RAISE(ABORT, 'forced persona artifact failure'); END;",
        )
        .expect("artifact append failure trigger should install");
    });
}

/// Expected hash derived through the shared parser, so tests never
/// hand-replicate `split_frontmatter`'s exact frontmatter/body boundaries.
pub(super) fn expected_hash(content: &str) -> String {
    let (frontmatter, body) = ralphx_domain::personas::skill_markdown::split_frontmatter(content)
        .expect("test persona content should carry frontmatter");
    compute_content_hash(frontmatter, body)
}

pub(super) fn memory_service() -> PersonaService {
    let conn = rusqlite::Connection::open_in_memory().expect("persona test database");
    crate::infrastructure::sqlite::migrations::run_migrations(&conn)
        .expect("persona test migrations");
    let shared = Arc::new(tokio::sync::Mutex::new(conn));
    PersonaService::new(
        DbConnection::from_shared(Arc::clone(&shared)),
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared))),
        Arc::new(SqliteChatConversationRepository::from_shared(shared)),
    )
}

pub(super) fn sqlite_service(db: &SqliteTestDb) -> PersonaService {
    let shared = db.shared_conn();
    PersonaService::new(
        DbConnection::from_shared(Arc::clone(&shared)),
        Arc::new(SqlitePersonaRepository::from_shared(Arc::clone(&shared))),
        Arc::new(SqliteChatConversationRepository::from_shared(shared)),
    )
}

pub(super) fn draft_input(slug: &str, body: &str) -> SavePersonaDraftInput {
    SavePersonaDraftInput {
        project_id: None,
        slug: slug.to_string(),
        content: persona_content(slug, body),
        source_session_id: Some("source-session".to_string()),
        source_persona_id: None,
        source_content_hash: None,
    }
}

pub(super) async fn create_active(service: &PersonaService, slug: &str) -> PersonaId {
    let draft = service
        .create_draft(true, draft_input(slug, "Initial body"))
        .await
        .expect("draft should be created");
    service
        .approve_persona(true, &draft.id)
        .await
        .expect("draft should be approved");
    draft.id
}

pub(super) fn assert_disabled(result: Result<impl std::fmt::Debug, AppError>) {
    assert!(matches!(result, Err(AppError::FeatureDisabled(_))));
}
