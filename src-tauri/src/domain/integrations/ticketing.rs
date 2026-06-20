use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTicketOperationKind {
    Transition,
    Assign,
    Comment,
}

impl ProviderTicketOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transition => "transition",
            Self::Assign => "assign",
            Self::Comment => "comment",
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
