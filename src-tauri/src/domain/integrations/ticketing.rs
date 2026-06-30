use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTicketOperationKind {
    Transition,
    Assign,
    Comment,
    SetLabels,
}

impl ProviderTicketOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transition => "transition",
            Self::Assign => "assign",
            Self::Comment => "comment",
            Self::SetLabels => "set_labels",
        }
    }
}

impl std::str::FromStr for ProviderTicketOperationKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "transition" => Ok(Self::Transition),
            "assign" => Ok(Self::Assign),
            "comment" => Ok(Self::Comment),
            "set_labels" => Ok(Self::SetLabels),
            other => Err(format!("Unknown provider ticket operation kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTicketOperationStatus {
    Pending,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
}

impl ProviderTicketOperationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Canceled => "canceled",
        }
    }

    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

impl std::str::FromStr for ProviderTicketOperationStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            "canceled" => Ok(Self::Canceled),
            other => Err(format!("Unknown provider ticket operation status: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTicketOperation {
    pub id: String,
    pub provider: String,
    pub external_kind: String,
    pub external_id: String,
    pub external_key: Option<String>,
    pub link_id: Option<String>,
    pub local_project_id: Option<String>,
    pub operation: ProviderTicketOperationKind,
    pub client_operation_id: String,
    pub status: ProviderTicketOperationStatus,
    pub provider_operation_id: Option<String>,
    pub error_message: Option<String>,
    pub metadata_json: Option<String>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTicketOperationUpsert {
    pub provider: String,
    pub external_kind: String,
    pub external_id: String,
    pub external_key: Option<String>,
    pub link_id: Option<String>,
    pub local_project_id: Option<String>,
    pub operation: ProviderTicketOperationKind,
    pub client_operation_id: String,
    pub status: ProviderTicketOperationStatus,
    pub provider_operation_id: Option<String>,
    pub error_message: Option<String>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservedTicketingStatus {
    pub provider_status_id: String,
    pub provider_status_name: String,
    pub provider_category: String,
    pub provider_color: Option<String>,
    pub provider_order: Option<i64>,
    pub is_terminal: bool,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingStatusCatalogEntry {
    pub id: String,
    pub provider: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub provider_status_id: String,
    pub provider_status_name: String,
    pub provider_category: String,
    pub provider_color: Option<String>,
    pub provider_order: Option<i64>,
    pub display_order: i64,
    pub color_override: Option<String>,
    pub is_visible: bool,
    pub is_terminal: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub stale_since: Option<DateTime<Utc>>,
    pub metadata_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TicketingStatusCatalogEntry {
    pub fn resolved_color(&self) -> Option<&str> {
        self.color_override
            .as_deref()
            .or(self.provider_color.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingStatusCatalogUpsert {
    pub provider: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub provider_status_id: String,
    pub provider_status_name: String,
    pub provider_category: String,
    pub provider_color: Option<String>,
    pub provider_order: Option<i64>,
    pub display_order: i64,
    pub is_terminal: bool,
    pub last_seen_at: DateTime<Utc>,
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingStatusPresentationPatch {
    pub provider_status_id: String,
    pub display_order: Option<i64>,
    pub color_override: Option<Option<String>>,
    pub is_visible: Option<bool>,
}
