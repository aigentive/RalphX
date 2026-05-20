use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationValidationStatus {
    NotConfigured,
    Pending,
    Valid,
    Invalid,
}

impl IntegrationValidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotConfigured => "not_configured",
            Self::Pending => "pending",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

impl Default for IntegrationValidationStatus {
    fn default() -> Self {
        Self::NotConfigured
    }
}

impl std::str::FromStr for IntegrationValidationStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "not_configured" => Ok(Self::NotConfigured),
            "pending" => Ok(Self::Pending),
            "valid" => Ok(Self::Valid),
            "invalid" => Ok(Self::Invalid),
            other => Err(format!("Unknown integration validation status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtlassianAuthMethod {
    ApiToken,
    OAuth,
}

impl AtlassianAuthMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiToken => "api_token",
            Self::OAuth => "oauth",
        }
    }
}

impl Default for AtlassianAuthMethod {
    fn default() -> Self {
        Self::ApiToken
    }
}

impl std::str::FromStr for AtlassianAuthMethod {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "api_token" => Ok(Self::ApiToken),
            "oauth" => Ok(Self::OAuth),
            other => Err(format!("Unknown Atlassian auth method: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianIntegrationSettings {
    pub enabled: bool,
    pub auth_method: AtlassianAuthMethod,
    pub site_url: Option<String>,
    pub email: Option<String>,
    pub token_secret_ref: Option<String>,
    pub oauth_client_id: Option<String>,
    pub oauth_redirect_uri: Option<String>,
    pub oauth_client_secret_ref: Option<String>,
    pub oauth_access_token_ref: Option<String>,
    pub oauth_refresh_token_ref: Option<String>,
    pub oauth_cloud_id: Option<String>,
    pub oauth_scopes: Option<String>,
    pub oauth_access_token_expires_at: Option<DateTime<Utc>>,
    pub validation_status: IntegrationValidationStatus,
    pub jira_available: bool,
    pub confluence_available: bool,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

impl Default for AtlassianIntegrationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            auth_method: AtlassianAuthMethod::ApiToken,
            site_url: None,
            email: None,
            token_secret_ref: None,
            oauth_client_id: None,
            oauth_redirect_uri: None,
            oauth_client_secret_ref: None,
            oauth_access_token_ref: None,
            oauth_refresh_token_ref: None,
            oauth_cloud_id: None,
            oauth_scopes: None,
            oauth_access_token_expires_at: None,
            validation_status: IntegrationValidationStatus::NotConfigured,
            jira_available: false,
            confluence_available: false,
            last_validated_at: None,
            last_error: None,
            updated_at: Utc::now(),
        }
    }
}

#[async_trait]
pub trait AtlassianIntegrationSettingsRepository: Send + Sync {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>>;

    async fn upsert(
        &self,
        settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>>;
}
