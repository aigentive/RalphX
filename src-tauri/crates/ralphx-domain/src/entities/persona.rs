use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonaStatus {
    Draft,
    Active,
    Archived,
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
    pub slug: String,
    pub name: String,
    pub description: String,
    pub content: String,
    pub status: PersonaStatus,
    pub version: i64,
    pub content_hash: String,
    pub source_session_id: Option<String>,
    pub source_json: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Persona {
    pub fn is_bindable(&self) -> bool {
        matches!(self.status, PersonaStatus::Active)
    }
}
