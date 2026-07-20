use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::entities::{ArtifactId, ProjectId};
use crate::error::AppError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PersonaId(pub String);

impl PersonaId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn from_string(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for PersonaId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for PersonaId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl From<String> for PersonaId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for PersonaId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonaDirective {
    #[default]
    Inherit,
    Suppress,
    Explicit(PersonaId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonaStatus {
    Draft,
    Active,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonaScopeFilter {
    All,
    GlobalOnly,
    GlobalAndProject(ProjectId),
}

impl fmt::Display for PersonaStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draft => write!(formatter, "draft"),
            Self::Active => write!(formatter, "active"),
            Self::Archived => write!(formatter, "archived"),
        }
    }
}

impl FromStr for PersonaStatus {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            _ => Err(AppError::Validation(format!(
                "Invalid persona status: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Persona {
    pub id: PersonaId,
    pub artifact_id: Option<ArtifactId>,
    pub project_id: Option<ProjectId>,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub status: PersonaStatus,
    pub version: i64,
    pub content_hash: String,
    pub source_session_id: Option<String>,
    pub source_persona_id: Option<PersonaId>,
    pub source_content_hash: Option<String>,
    pub source_json: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Persona {
    pub fn is_bindable(&self) -> bool {
        matches!(self.status, PersonaStatus::Active)
    }

    pub fn is_bindable_to_project(&self, project_id: &ProjectId) -> bool {
        self.is_bindable()
            && self
                .project_id
                .as_ref()
                .is_none_or(|persona_project_id| persona_project_id == project_id)
    }
}

#[cfg(test)]
#[path = "persona_tests.rs"]
mod tests;
