use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::domain::entities::{Persona, PersonaId, PersonaStatus};
use crate::domain::repositories::PersonaRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

const PERSONA_COLUMNS: &str = "id, slug, name, description, content, status, version, content_hash, source_session_id, source_persona_id, source_content_hash, source_json, created_at, updated_at";

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

fn persona_from_row(row: &rusqlite::Row) -> rusqlite::Result<Persona> {
    let status = row
        .get::<_, String>("status")?
        .parse::<PersonaStatus>()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
    let created_at = DateTime::parse_from_rfc3339(&row.get::<_, String>("created_at")?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                10,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .with_timezone(&Utc);
    let updated_at = DateTime::parse_from_rfc3339(&row.get::<_, String>("updated_at")?)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                11,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?
        .with_timezone(&Utc);

    Ok(Persona {
        id: PersonaId::from(row.get::<_, String>("id")?),
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

fn map_live_slug_unique_error(error: AppError, slug: &str) -> AppError {
    match error {
        AppError::Database(message)
            if message.contains("UNIQUE constraint failed: personas.slug")
                || message.contains("idx_personas_slug_live") =>
        {
            AppError::Validation(format!("Persona slug `{slug}` is already in use"))
        }
        other => other,
    }
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
        let id = persona.id.as_str().to_string();
        let slug = persona.slug.clone();
        let name = persona.name.clone();
        let description = persona.description.clone();
        let content = persona.content.clone();
        let status = persona.status.to_string();
        let version = persona.version;
        let content_hash = persona.content_hash.clone();
        let source_session_id = persona.source_session_id.clone();
        let source_persona_id = persona
            .source_persona_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let source_content_hash = persona.source_content_hash.clone();
        let source_json = persona.source_json.clone();
        let created_at = persona.created_at.to_rfc3339();
        let updated_at = persona.updated_at.to_rfc3339();
        let collision_slug = slug.clone();

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO personas (
                        id, slug, name, description, content, status, version, content_hash,
                        source_session_id, source_persona_id, source_content_hash, source_json,
                        created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        id,
                        slug,
                        name,
                        description,
                        content,
                        status,
                        version,
                        content_hash,
                        source_session_id,
                        source_persona_id,
                        source_content_hash,
                        source_json,
                        created_at,
                        updated_at,
                    ],
                )?;
                Ok(persona)
            })
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

    async fn get_active_by_slug(&self, slug: &str) -> AppResult<Option<Persona>> {
        let slug = slug.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    &format!("SELECT {PERSONA_COLUMNS} FROM personas WHERE slug = ?1 AND status = 'active' LIMIT 1"),
                    [slug],
                    persona_from_row,
                )
            })
            .await
    }

    async fn list(&self) -> AppResult<Vec<Persona>> {
        self.db
            .run(|conn| {
                let mut statement = conn.prepare(&format!(
                    "SELECT {PERSONA_COLUMNS} FROM personas ORDER BY created_at DESC"
                ))?;
                let rows = statement
                    .query_map([], persona_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
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

    async fn update_content(
        &self,
        id: &PersonaId,
        content: &str,
        content_hash: &str,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let content = content.to_string();
        let content_hash = content_hash.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE personas
                     SET content = ?1, content_hash = ?2, version = version + 1, updated_at = ?3
                     WHERE id = ?4",
                    rusqlite::params![content, content_hash, Utc::now().to_rfc3339(), id],
                )?;
                Ok(())
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
