use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;

use super::integration_reference_expansion::{
    IntegrationReferenceExpansion, SkippedIntegrationReferenceReason,
};
use crate::application::{
    AtlassianIntegrationService, ClickUpApiClient, ClickUpAuthContext, ClickUpIntegrationService,
    ClickUpTaskContent, ClickUpWorkspace, EmptyAtlassianApiClient, EmptyClickUpApiClient,
    EmptyLinearApiClient, LinearIntegrationService, LinearIntegrationSettings,
    LinearIntegrationSettingsRepository,
};
use crate::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::services::{ComposerIntegrationReference, SecretStore};
use crate::infrastructure::memory::{
    MemoryAtlassianIntegrationSettingsRepository, MemoryClickUpIntegrationSettingsRepository,
    MemoryLinearIntegrationSettingsRepository, MemorySecretStore,
};

const ATLASSIAN_TOKEN_REF: &str = "integrations/atlassian/default/api-token";

struct FailingSettingsRepository;

fn settings_error() -> Box<dyn Error> {
    Box::new(std::io::Error::other("settings unavailable"))
}

#[async_trait]
impl ClickUpIntegrationSettingsRepository for FailingSettingsRepository {
    async fn get(&self) -> Result<ClickUpIntegrationSettings, Box<dyn Error>> {
        Err(settings_error())
    }

    async fn upsert(
        &self,
        _settings: &ClickUpIntegrationSettings,
    ) -> Result<ClickUpIntegrationSettings, Box<dyn Error>> {
        Err(settings_error())
    }
}

#[async_trait]
impl LinearIntegrationSettingsRepository for FailingSettingsRepository {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn Error>> {
        Err(settings_error())
    }

    async fn upsert(
        &self,
        _settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn Error>> {
        Err(settings_error())
    }
}

#[async_trait]
impl AtlassianIntegrationSettingsRepository for FailingSettingsRepository {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn Error>> {
        Err(settings_error())
    }

    async fn upsert(
        &self,
        _settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn Error>> {
        Err(settings_error())
    }
}

struct UnicodeClickUpClient {
    body: String,
}

#[async_trait]
impl ClickUpApiClient for UnicodeClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(Vec::new())
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        Ok(ClickUpTaskContent {
            id: task_id.to_string(),
            custom_id: None,
            name: "Unicode task".to_string(),
            url: None,
            description: self.body.clone(),
            status_name: None,
            status_type: None,
            status_category: None,
            creator: None,
            assignees: Vec::new(),
            watchers: Vec::new(),
            tags: Vec::new(),
            comments: Vec::new(),
            attachments: Vec::new(),
            updated_at: None,
            space_id: None,
            list_name: None,
        })
    }
}

fn reference(provider: &str, kind: &str, id: &str) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: provider.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        key: None,
        title: Some(format!("{provider} reference")),
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }
}

fn assert_single_skip(
    expansion: &IntegrationReferenceExpansion,
    expected: SkippedIntegrationReferenceReason,
) {
    assert_eq!(expansion.rewritten_prompt, "Base");
    assert_eq!(expansion.skipped_references.len(), 1);
    assert_eq!(expansion.skipped_references[0].reason, expected);
}

fn wrapper_and_fixed_lengths(
    expanded_prompt: &str,
    base_prompt: &str,
    opening_tag: &str,
    closing_tag: &str,
    body_len: usize,
) -> (usize, usize) {
    let start = expanded_prompt
        .find(opening_tag)
        .expect("expanded prompt should contain its opening tag");
    let end = expanded_prompt
        .find(closing_tag)
        .map(|index| index + closing_tag.len())
        .expect("expanded prompt should contain its closing tag");
    let wrapper_len = start
        .saturating_sub(base_prompt.len())
        .saturating_add(expanded_prompt.len().saturating_sub(end));
    let fixed_len = end.saturating_sub(start).saturating_sub(body_len);
    (wrapper_len, fixed_len)
}

#[tokio::test]
async fn provider_edge_paths_preserve_the_prompt_and_report_typed_skips() {
    let clickup = ClickUpIntegrationService::new(
        Arc::new(FailingSettingsRepository),
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyClickUpApiClient),
    );
    let unsupported = clickup
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("clickup", "document", "doc-1")],
            4096,
        )
        .await;
    assert_single_skip(
        &unsupported,
        SkippedIntegrationReferenceReason::UnsupportedReference,
    );
    let clickup_settings_error = clickup
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("clickup", "task", "task-1")],
            4096,
        )
        .await;
    assert_single_skip(
        &clickup_settings_error,
        SkippedIntegrationReferenceReason::ApiError,
    );

    let linear = LinearIntegrationService::new(
        Arc::new(FailingSettingsRepository),
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyLinearApiClient),
    );
    let no_linear_references = linear
        .expand_references_for_prompt_with_budget("Base", &[], 4096)
        .await;
    assert_eq!(no_linear_references.rewritten_prompt, "Base");
    assert!(no_linear_references.skipped_references.is_empty());
    let linear_settings_error = linear
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("linear", "linear", "issue-1")],
            4096,
        )
        .await;
    assert_single_skip(
        &linear_settings_error,
        SkippedIntegrationReferenceReason::ApiError,
    );

    let atlassian = AtlassianIntegrationService::new(
        Arc::new(FailingSettingsRepository),
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyAtlassianApiClient),
    );
    let no_atlassian_references = atlassian
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("linear", "linear", "issue-1")],
            4096,
        )
        .await;
    assert_eq!(no_atlassian_references.rewritten_prompt, "Base");
    assert!(no_atlassian_references.skipped_references.is_empty());
    let atlassian_settings_error = atlassian
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("atlassian", "jira", "RX-1")],
            4096,
        )
        .await;
    assert_single_skip(
        &atlassian_settings_error,
        SkippedIntegrationReferenceReason::ApiError,
    );
}

#[tokio::test]
async fn missing_token_references_fail_closed_before_provider_fetches() {
    let clickup_repo = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    clickup_repo
        .upsert(&ClickUpIntegrationSettings {
            enabled: true,
            validation_status: IntegrationValidationStatus::Valid,
            token_secret_ref: None,
            ..ClickUpIntegrationSettings::default()
        })
        .await
        .expect("seed ClickUp settings without a token reference");
    let clickup = ClickUpIntegrationService::new(
        clickup_repo,
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyClickUpApiClient),
    );
    let clickup_missing_token = clickup
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("clickup", "task", "task-1")],
            4096,
        )
        .await;
    assert_single_skip(
        &clickup_missing_token,
        SkippedIntegrationReferenceReason::MissingCredentials,
    );

    let linear_repo = Arc::new(MemoryLinearIntegrationSettingsRepository::new());
    linear_repo
        .upsert(&LinearIntegrationSettings {
            enabled: true,
            validation_status: IntegrationValidationStatus::Valid,
            token_secret_ref: None,
            ..LinearIntegrationSettings::default()
        })
        .await
        .expect("seed Linear settings without a token reference");
    let linear = LinearIntegrationService::new(
        linear_repo,
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyLinearApiClient),
    );
    let linear_missing_token = linear
        .expand_references_for_prompt_with_budget(
            "Base",
            &[reference("linear", "linear", "issue-1")],
            4096,
        )
        .await;
    assert_single_skip(
        &linear_missing_token,
        SkippedIntegrationReferenceReason::MissingCredentials,
    );
}

#[tokio::test]
async fn fixed_render_overhead_is_budgeted_and_clickup_truncation_preserves_utf8() {
    let unicode_body = "é".repeat(200);
    let clickup = ClickUpIntegrationService::new(
        Arc::new(MemoryClickUpIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        Arc::new(UnicodeClickUpClient {
            body: unicode_body.clone(),
        }),
    );
    clickup
        .save_settings(
            Some("clickup-token".to_string()),
            Some("workspace-1".to_string()),
        )
        .await
        .expect("save ClickUp settings");
    clickup
        .validate_and_enable()
        .await
        .expect("validate ClickUp settings");
    let clickup_reference = reference("clickup", "task", "task-1");
    let full_clickup = clickup
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&clickup_reference),
            4096,
        )
        .await;
    let (clickup_wrapper, clickup_fixed) = wrapper_and_fixed_lengths(
        &full_clickup.rewritten_prompt,
        "Base",
        "<clickup_task",
        "</clickup_task>",
        unicode_body.len(),
    );
    let clickup_fixed_only = clickup
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&clickup_reference),
            clickup_wrapper + clickup_fixed,
        )
        .await;
    assert_single_skip(
        &clickup_fixed_only,
        SkippedIntegrationReferenceReason::BudgetExceeded,
    );
    let clickup_one_body_byte = clickup
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&clickup_reference),
            clickup_wrapper + clickup_fixed + 1,
        )
        .await;
    assert!(clickup_one_body_byte
        .rewritten_prompt
        .contains("truncated=\"true\""));
    assert!(!clickup_one_body_byte.rewritten_prompt.contains('é'));

    let linear = LinearIntegrationService::new(
        Arc::new(MemoryLinearIntegrationSettingsRepository::new()),
        Arc::new(MemorySecretStore::new()),
        Arc::new(EmptyLinearApiClient),
    );
    linear
        .save_settings(Some("linear-token".to_string()))
        .await
        .expect("save Linear settings");
    linear
        .validate_and_enable()
        .await
        .expect("validate Linear settings");
    let linear_reference = reference("linear", "linear", "issue-1");
    let full_linear = linear
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&linear_reference),
            4096,
        )
        .await;
    let (linear_wrapper, linear_fixed) = wrapper_and_fixed_lengths(
        &full_linear.rewritten_prompt,
        "Base",
        "<linear_issue",
        "</linear_issue>",
        0,
    );
    let linear_fixed_only = linear
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&linear_reference),
            linear_wrapper + linear_fixed,
        )
        .await;
    assert_single_skip(
        &linear_fixed_only,
        SkippedIntegrationReferenceReason::BudgetExceeded,
    );

    let atlassian_repo = Arc::new(MemoryAtlassianIntegrationSettingsRepository::new());
    atlassian_repo
        .upsert(&AtlassianIntegrationSettings {
            enabled: true,
            auth_method: AtlassianAuthMethod::ApiToken,
            site_url: Some("https://example.atlassian.net".to_string()),
            email: Some("user@example.com".to_string()),
            token_secret_ref: Some(ATLASSIAN_TOKEN_REF.to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            jira_available: true,
            confluence_available: true,
            ..AtlassianIntegrationSettings::default()
        })
        .await
        .expect("seed Atlassian settings");
    let atlassian_secrets = Arc::new(MemorySecretStore::new());
    atlassian_secrets
        .put_secret(ATLASSIAN_TOKEN_REF, "atlassian-token")
        .await
        .expect("seed Atlassian token");
    let atlassian = AtlassianIntegrationService::new(
        atlassian_repo,
        atlassian_secrets,
        Arc::new(EmptyAtlassianApiClient),
    );
    let atlassian_reference = reference("atlassian", "jira", "RX-1");
    let full_atlassian = atlassian
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&atlassian_reference),
            4096,
        )
        .await;
    let (atlassian_wrapper, atlassian_fixed) = wrapper_and_fixed_lengths(
        &full_atlassian.rewritten_prompt,
        "Base",
        "<jira",
        "</jira>",
        0,
    );
    let atlassian_fixed_only = atlassian
        .expand_references_for_prompt_with_budget(
            "Base",
            std::slice::from_ref(&atlassian_reference),
            atlassian_wrapper + atlassian_fixed,
        )
        .await;
    assert_single_skip(
        &atlassian_fixed_only,
        SkippedIntegrationReferenceReason::BudgetExceeded,
    );
}
