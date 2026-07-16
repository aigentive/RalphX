use std::sync::Arc;

use crate::application::{
    AtlassianIntegrationService, ClickUpIntegrationService, GranolaIntegrationService,
    LinearIntegrationService,
};
use crate::domain::services::ComposerIntegrationReference;

pub(crate) const MAX_TOTAL_INTEGRATION_REFERENCE_BYTES: usize = 192 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationReferenceExpansion {
    pub rewritten_prompt: String,
    pub skipped_references: Vec<SkippedIntegrationReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedIntegrationReference {
    pub provider: String,
    pub kind: String,
    pub id: String,
    pub reason: SkippedIntegrationReferenceReason,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkippedIntegrationReferenceReason {
    ProviderUnavailable,
    IntegrationDisabled,
    MissingCredentials,
    NotFound,
    RateLimited,
    ApiError,
    UnsupportedReference,
    BudgetExceeded,
}

impl SkippedIntegrationReferenceReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "provider_unavailable",
            Self::IntegrationDisabled => "integration_disabled",
            Self::MissingCredentials => "missing_credentials",
            Self::NotFound => "not_found",
            Self::RateLimited => "rate_limited",
            Self::ApiError => "api_error",
            Self::UnsupportedReference => "unsupported_reference",
            Self::BudgetExceeded => "budget_exceeded",
        }
    }
}

impl SkippedIntegrationReference {
    pub(crate) fn new(
        reference: &ComposerIntegrationReference,
        reason: SkippedIntegrationReferenceReason,
        message: impl Into<String>,
    ) -> Self {
        Self {
            provider: reference.provider.clone(),
            kind: reference.kind.clone(),
            id: reference.id.clone(),
            reason,
            message: message.into(),
        }
    }
}

pub async fn expand_integration_references_for_prompt(
    runtime_content: &str,
    integration_references: &[ComposerIntegrationReference],
    atlassian_service: Option<Arc<AtlassianIntegrationService>>,
    linear_service: Option<Arc<LinearIntegrationService>>,
    clickup_service: Option<Arc<ClickUpIntegrationService>>,
    granola_service: Option<Arc<GranolaIntegrationService>>,
) -> IntegrationReferenceExpansion {
    if integration_references.is_empty() {
        return IntegrationReferenceExpansion {
            rewritten_prompt: runtime_content.to_string(),
            skipped_references: Vec::new(),
        };
    }

    let mut rewritten_prompt = runtime_content.to_string();
    let mut skipped_references = Vec::new();

    if integration_references
        .iter()
        .any(|reference| reference.provider == "atlassian")
    {
        if let Some(service) = atlassian_service {
            let atlassian_budget =
                remaining_budget_after_prior_expansions(runtime_content, &rewritten_prompt);
            rewritten_prompt = service
                .expand_references_for_prompt_with_budget(
                    &rewritten_prompt,
                    integration_references,
                    atlassian_budget,
                )
                .await;
        }
    }

    if integration_references
        .iter()
        .any(|reference| reference.provider == "linear")
    {
        if let Some(service) = linear_service {
            let linear_budget =
                remaining_budget_after_prior_expansions(runtime_content, &rewritten_prompt);
            rewritten_prompt = service
                .expand_references_for_prompt_with_budget(
                    &rewritten_prompt,
                    integration_references,
                    linear_budget,
                )
                .await;
        }
    }

    if integration_references
        .iter()
        .any(|reference| reference.provider == "clickup")
    {
        if let Some(service) = clickup_service {
            let clickup_budget =
                remaining_budget_after_prior_expansions(runtime_content, &rewritten_prompt);
            rewritten_prompt = service
                .expand_references_for_prompt_with_budget(
                    &rewritten_prompt,
                    integration_references,
                    clickup_budget,
                )
                .await;
        }
    }

    let granola_budget =
        remaining_budget_after_prior_expansions(runtime_content, &rewritten_prompt);
    if integration_references
        .iter()
        .any(|reference| reference.provider == "granola")
    {
        if let Some(service) = granola_service {
            let granola_expansion = service
                .expand_note_references_for_prompt_with_budget(
                    &rewritten_prompt,
                    integration_references,
                    granola_budget,
                )
                .await;
            rewritten_prompt = granola_expansion.rewritten_prompt;
            skipped_references.extend(granola_expansion.skipped_references);
        } else {
            skipped_references.extend(
                integration_references
                    .iter()
                    .filter(|reference| reference.provider == "granola")
                    .map(|reference| {
                        SkippedIntegrationReference::new(
                            reference,
                            SkippedIntegrationReferenceReason::ProviderUnavailable,
                            "Granola integration service is unavailable",
                        )
                    }),
            );
        }
    }

    skipped_references.extend(
        integration_references
            .iter()
            .filter(|reference| {
                !matches!(
                    reference.provider.as_str(),
                    "atlassian" | "linear" | "clickup" | "granola"
                )
            })
            .map(|reference| {
                SkippedIntegrationReference::new(
                    reference,
                    SkippedIntegrationReferenceReason::UnsupportedReference,
                    "Integration reference provider is not supported",
                )
            }),
    );

    IntegrationReferenceExpansion {
        rewritten_prompt,
        skipped_references,
    }
}

pub(crate) fn log_skipped_integration_references(
    skipped_references: &[SkippedIntegrationReference],
) {
    for skipped in skipped_references {
        tracing::warn!(
            provider = %skipped.provider,
            kind = %skipped.kind,
            id = %skipped.id,
            reason = skipped.reason.as_str(),
            message = %skipped.message,
            "integration reference skipped during prompt expansion"
        );
    }
}

fn remaining_budget_after_prior_expansions(original_prompt: &str, rewritten_prompt: &str) -> usize {
    let original_len = original_prompt.trim_end().len();
    let added_bytes = rewritten_prompt.len().saturating_sub(original_len);
    MAX_TOTAL_INTEGRATION_REFERENCE_BYTES.saturating_sub(added_bytes)
}
