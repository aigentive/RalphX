use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::domain::entities::{
    ArtifactId, Persona, PersonaId, PersonaScopeFilter, PersonaStatus, ProjectId,
};
use crate::domain::repositories::PersonaRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

pub(crate) const PERSONA_COLUMNS: &str = "id, artifact_id, project_id, slug, name, description, content, status, version, content_hash, source_session_id, source_persona_id, source_content_hash, source_json, created_at, updated_at";
const ACTIVE_SLUG_SCOPED_INDEX: &str = "personas_active_slug_scoped";

pub struct SqlitePersonaRepository {
    db: DbConnection,
}

impl SqlitePersonaRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

pub(crate) fn persona_from_row(row: &rusqlite::Row) -> rusqlite::Result<Persona> {
    let status = row
        .get::<_, String>("status")?
        .parse::<PersonaStatus>()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    let created_at = DateTime::parse_from_rfc3339(&row.get::<_, String>("created_at")?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                13,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&row.get::<_, String>("updated_at")?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                14,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .with_timezone(&Utc);

    Ok(Persona {
        id: PersonaId::from(row.get::<_, String>("id")?),
        artifact_id: row
            .get::<_, Option<String>>("artifact_id")?
            .map(ArtifactId::from_string),
        project_id: row
            .get::<_, Option<String>>("project_id")?
            .map(ProjectId::from_string),
        slug: row.get("slug")?,
        name: row.get("name")?,
        description: row.get("description")?,
        content: row.get("content")?,
        status,
        version: row.get("version")?,
        content_hash: row.get("content_hash")?,
        source_session_id: row.get("source_session_id")?,
        source_persona_id: row
            .get::<_, Option<String>>("source_persona_id")?
            .map(PersonaId::from),
        source_content_hash: row.get("source_content_hash")?,
        source_json: row.get("source_json")?,
        created_at,
        updated_at,
    })
}

pub(crate) fn map_live_slug_unique_error(error: AppError, slug: &str) -> AppError {
    match error {
        AppError::Database(message)
            if message.contains("UNIQUE constraint failed: personas.slug")
                || message.contains(ACTIVE_SLUG_SCOPED_INDEX) =>
        {
            AppError::Validation(format!("Persona slug `{slug}` is already in use"))
        }
        other => other,
    }
}

pub(crate) fn persona_create_sync(
    conn: &rusqlite::Connection,
    persona: Persona,
) -> AppResult<Persona> {
    conn.execute(
        "INSERT INTO personas (
            id, artifact_id, project_id, slug, name, description, content, status, version, content_hash,
            source_session_id, source_persona_id, source_content_hash, source_json,
            created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        rusqlite::params![
            persona.id.as_str(),
            persona.artifact_id.as_ref().map(ArtifactId::as_str),
            persona.project_id.as_ref().map(ProjectId::as_str),
            persona.slug,
            persona.name,
            persona.description,
            persona.content,
            persona.status.to_string(),
            persona.version,
            persona.content_hash,
            persona.source_session_id,
            persona.source_persona_id.as_ref().map(ToString::to_string),
            persona.source_content_hash,
            persona.source_json,
            persona.created_at.to_rfc3339(),
            persona.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(persona)
}

pub(crate) fn persona_set_status_sync(
    conn: &rusqlite::Connection,
    id: &str,
    status: PersonaStatus,
) -> AppResult<()> {
    conn.execute(
        "UPDATE personas SET status = ?1, updated_at = ?2 WHERE id = ?3",
        rusqlite::params![status.to_string(), Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

#[async_trait]
impl PersonaRepository for SqlitePersonaRepository {
    async fn create(&self, persona: Persona) -> AppResult<Persona> {
        let collision_slug = persona.slug.clone();

        self.db
            .run(move |conn| persona_create_sync(conn, persona))
            .await
            .map_err(|error| map_live_slug_unique_error(error, &collision_slug))
    }

    async fn get_by_id(&self, id: &PersonaId) -> AppResult<Option<Persona>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE id = ?1"),
                    [id],
                    persona_from_row,
                )
            })
            .await
    }

    async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Persona>> {
        let slug = slug.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE slug = ?1 ORDER BY created_at DESC LIMIT 1"),
                    [slug],
                    persona_from_row,
                )
            })
            .await
    }

    async fn get_active_by_slug(
        &self,
        slug: &str,
        project_id: Option<&ProjectId>,
    ) -> AppResult<Option<Persona>> {
        let slug = slug.to_string();
        let project_id = project_id.map(ToString::to_string);
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE slug = ?1 AND project_id IS ?2 AND status = 'active' LIMIT 1"),
                    rusqlite::params![slug, project_id],
                    persona_from_row,
                )
            })
            .await
    }

    async fn get_draft_by_source_persona_id(
        &self,
        source_persona_id: &PersonaId,
    ) -> AppResult<Option<Persona>> {
        let source_persona_id = source_persona_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE source_persona_id = ?1 AND status = 'draft' ORDER BY created_at DESC, id DESC LIMIT 1"),
                    [source_persona_id],
                    persona_from_row,
                )
            })
            .await
    }

    async fn list(&self, scope: PersonaScopeFilter) -> AppResult<Vec<Persona>> {
        self.db
            .run(move |conn| {
                let (predicate, project_id) = match scope {
                    PersonaScopeFilter::All => ("1 = 1", None),
                    PersonaScopeFilter::GlobalOnly => ("project_id IS NULL", None),
                    PersonaScopeFilter::GlobalAndProject(project_id) => {
                        ("project_id IS NULL OR project_id = ?1", Some(project_id.to_string()))
                    }
                };
                let mut statement = conn.prepare(&format!(
                    "SELECT {PERSONA_COLUMNS} FROM personas WHERE {predicate} ORDER BY created_at DESC"
                ))?;
                let rows = if let Some(project_id) = project_id {
                    statement.query_map([project_id], persona_from_row)?.collect::<Result<Vec<_>, _>>()?
                } else {
                    statement.query_map([], persona_from_row)?.collect::<Result<Vec<_>, _>>()?
                };
                Ok(rows)
            })
            .await
    }

    async fn list_by_status(&self, status: PersonaStatus) -> AppResult<Vec<Persona>> {
        let status = status.to_string();
        self.db
            .run(move |conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {PERSONA_COLUMNS} FROM personas WHERE status = ?1 ORDER BY created_at DESC"
                ))?;
                let rows = statement
                    .query_map([status], persona_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    async fn set_status(&self, id: &PersonaId, status: PersonaStatus) -> AppResult<()> {
        let collision_slug = self
            .get_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Persona not found: {id}")))?
            .slug;
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| persona_set_status_sync(conn, &id, status))
            .await
            .map_err(|error| map_live_slug_unique_error(error, &collision_slug))
    }

    async fn delete(&self, id: &PersonaId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM personas WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }
}
