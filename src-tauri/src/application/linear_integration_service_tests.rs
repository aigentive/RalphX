use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{Mutex, RwLock};

use super::{
    LinearApiClient, LinearAuthContext, LinearIntegrationService, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository, LinearIssueContent, LinearIssueSummary,
};
use crate::domain::integrations::IntegrationValidationStatus;
use crate::domain::services::ComposerIntegrationReference;
use crate::infrastructure::memory::MemorySecretStore;

#[derive(Default)]
struct TestSettingsRepo {
    settings: RwLock<LinearIntegrationSettings>,
}

#[async_trait]
impl LinearIntegrationSettingsRepository for TestSettingsRepo {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        Ok(self.settings.read().await.clone())
    }

    async fn upsert(
        &self,
        settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        *self.settings.write().await = settings.clone();
        Ok(settings.clone())
    }
}

#[derive(Default)]
struct TestLinearClient {
    searches: Mutex<Vec<(String, usize)>>,
    validate_error: Mutex<Option<String>>,
}

#[async_trait]
impl LinearApiClient for TestLinearClient {
    async fn validate(&self, auth: &LinearAuthContext) -> Result<(), String> {
        assert_eq!(auth.api_token, "lin-api-token");
        if let Some(error) = self.validate_error.lock().await.clone() {
            Err(error)
        } else {
            Ok(())
        }
    }

    async fn search_issues(
        &self,
        auth: &LinearAuthContext,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        self.searches.lock().await.push((query.to_string(), limit));
        Ok(vec![LinearIssueSummary {
            id: "issue-id".to_string(),
            key: Some("LIN-123".to_string()),
            title: "Example".to_string(),
            url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
            excerpt: Some("Example body".to_string()),
            state_name: Some("In Progress".to_string()),
        }])
    }

    async fn fetch_issue(
        &self,
        auth: &LinearAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String> {
        assert_eq!(auth.api_token, "lin-api-token");
        Ok(LinearIssueContent {
            id: reference.id.clone(),
            key: reference.key.clone(),
            title: reference
                .title
                .clone()
                .unwrap_or_else(|| "Example".to_string()),
            url: reference.url.clone(),
            body: "Issue body".to_string(),
            state_name: Some("In Progress".to_string()),
        })
    }
}

#[tokio::test]
async fn save_validate_and_search_issues_with_api_token() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client.clone());

    let saved = service
        .save_settings(Some(" lin-api-token ".to_string()))
        .await
        .unwrap();
    assert!(!saved.enabled);
    assert_eq!(
        saved.validation_status,
        IntegrationValidationStatus::Pending
    );

    let validated = service.validate_and_enable().await.unwrap();
    assert!(validated.enabled);
    assert_eq!(
        validated.validation_status,
        IntegrationValidationStatus::Valid
    );
    assert!(validated.issue_search_available);

    let results = service.search_issues("bug", 50).await.unwrap();
    assert_eq!(results[0].key.as_deref(), Some("LIN-123"));
    assert_eq!(
        client.searches.lock().await.as_slice(),
        &[("bug".to_string(), 25)]
    );
}

#[tokio::test]
async fn validation_failure_disables_search() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    *client.validate_error.lock().await = Some("Linear rejected credentials".to_string());
    let service = LinearIntegrationService::new(repo, secrets, client);
    service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();

    let settings = service.validate_and_enable().await.unwrap();

    assert!(!settings.enabled);
    assert_eq!(
        settings.validation_status,
        IntegrationValidationStatus::Invalid
    );
    assert_eq!(
        settings.last_error.as_deref(),
        Some("Linear rejected credentials")
    );
}

#[tokio::test]
async fn expands_linear_issue_references_for_prompt() {
    let repo = Arc::new(TestSettingsRepo::default());
    let secrets = Arc::new(MemorySecretStore::new());
    let client = Arc::new(TestLinearClient::default());
    let service = LinearIntegrationService::new(repo, secrets, client);
    service
        .save_settings(Some("lin-api-token".to_string()))
        .await
        .unwrap();
    service.validate_and_enable().await.unwrap();

    let expanded = service
        .expand_references_for_prompt(
            "Fix this",
            &[ComposerIntegrationReference {
                provider: "linear".to_string(),
                kind: "linear".to_string(),
                id: "issue-id".to_string(),
                key: Some("LIN-123".to_string()),
                title: Some("Example".to_string()),
                url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
            }],
        )
        .await;

    assert!(expanded.contains("<linear_issue"));
    assert!(expanded.contains("LIN-123"));
    assert!(expanded.contains("Issue body"));
}
