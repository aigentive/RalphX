//! Strict ticket Git-policy resolution for workspace-owning ClickUp starts.

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::application::clickup_git_association::{
    clickup_identity_from_task, preferred_clickup_task_token, resolve_clickup_ticket_start,
    ClickUpTicketStartResolution,
};
use crate::application::clickup_integration_service::ClickUpTaskContent;
use crate::application::git_service::GitService;
use crate::application::ticket_git_convention::{
    disambiguate_branch_name, TicketGitConventionContext, TicketGitConventionTemplates,
};
use crate::application::AppState;
use crate::domain::entities::{
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId, TicketCanonicalBranch,
    TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState, TicketCanonicalBranchPolicyKind,
    TicketGitConventionSnapshot,
};
use crate::domain::integrations::ClickUpIntegrationSettings;

const CLICKUP_PROVIDER: &str = "clickup";
const STRICT_TICKET_GIT_POLICY_VERSION: i64 = 1;

#[path = "ticket_git_strict_start_policy.rs"]
mod policy;
#[path = "ticket_git_strict_start_provision.rs"]
mod provision;

use policy::*;
pub use policy::{
    activate_strict_ticket_branch_cycle, resolve_strict_ticket_target_base_ref,
    rollback_strict_ticket_workspace_activation, strict_clickup_ticket_policy_applies,
};
use provision::*;

#[derive(Debug, Clone, Copy)]
pub struct StrictClickUpTicketContext<'a> {
    pub task: &'a ClickUpTaskContent,
    pub settings: &'a ClickUpIntegrationSettings,
    pub username: Option<&'a str>,
    pub target_base_ref: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrictTicketGitPreview {
    pub task_id: String,
    pub task_title: String,
    pub username: Option<String>,
    pub branch_name: String,
    pub target_base_ref: String,
    pub commit_subject_rule: String,
    pub pr_title: String,
    pub policy_version: i64,
    pub persisted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrictTicketGitResolution {
    pub binding: TicketCanonicalBranch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrictTicketGitBlockerCode {
    InvalidConvention,
    MissingUsername,
    LegacyBindingConflict,
    BranchBindingConflict,
    EvidenceMismatch,
    InvalidCycleState,
    ActiveOwner,
    GitProvisioningFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrictTicketGitBlocker {
    pub code: StrictTicketGitBlockerCode,
    pub message: String,
    pub task_id: Option<String>,
    pub expected_branch: Option<String>,
    pub owner_conversation_id: Option<String>,
}

impl StrictTicketGitBlocker {
    fn new(code: StrictTicketGitBlockerCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            task_id: None,
            expected_branch: None,
            owner_conversation_id: None,
        }
    }

    fn for_task(mut self, task_id: &str) -> Self {
        self.task_id = Some(task_id.to_string());
        self
    }

    fn for_branch(mut self, branch: &str) -> Self {
        self.expected_branch = Some(branch.to_string());
        self
    }
}

impl std::fmt::Display for StrictTicketGitBlocker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let details = serde_json::to_string(self).unwrap_or_else(|_| self.message.clone());
        write!(formatter, "[ralphx:ticket_git_convention] {details}")
    }
}

impl std::error::Error for StrictTicketGitBlocker {}

pub async fn preview_strict_clickup_ticket_branch(
    state: &AppState,
    project_id: &ProjectId,
    context: StrictClickUpTicketContext<'_>,
) -> Result<Option<StrictTicketGitPreview>, StrictTicketGitBlocker> {
    let issue_key = clickup_identity_from_task(context.task).preferred_token();
    if let Some(existing) = load_binding(state, project_id, &issue_key).await? {
        return strict_preview_from_binding(existing).map(Some);
    }
    if !context.settings.strict_git_naming_enabled {
        return Ok(None);
    }
    render_new_preview(context, false).map(Some)
}

/// Resolve a read-only strict preview from live ClickUp settings and identity.
/// Existing bindings do not depend on the current settings or current-user API.
pub async fn preview_strict_clickup_ticket_branch_from_services(
    state: &AppState,
    project_id: &ProjectId,
    task: &ClickUpTaskContent,
    target_base_ref: &str,
) -> Result<Option<StrictTicketGitPreview>, StrictTicketGitBlocker> {
    let issue_key = clickup_identity_from_task(task).preferred_token();
    if load_binding(state, project_id, &issue_key).await?.is_some() {
        let disabled_settings = ClickUpIntegrationSettings::default();
        return preview_strict_clickup_ticket_branch(
            state,
            project_id,
            StrictClickUpTicketContext {
                task,
                settings: &disabled_settings,
                username: None,
                target_base_ref,
            },
        )
        .await;
    }
    let settings = state
        .clickup_integration_service
        .get_settings()
        .await
        .map_err(|error| convention_service_blocker(&issue_key, error))?;
    if !settings.strict_git_naming_enabled {
        return Ok(None);
    }
    let username = current_username_if_required(state, &settings, &issue_key).await?;
    preview_strict_clickup_ticket_branch(
        state,
        project_id,
        StrictClickUpTicketContext {
            task,
            settings: &settings,
            username: username.as_deref(),
            target_base_ref,
        },
    )
    .await
}

pub async fn ensure_strict_clickup_ticket_branch(
    state: &AppState,
    project_id: &ProjectId,
    context: StrictClickUpTicketContext<'_>,
    allowed_owner: Option<&ChatConversationId>,
) -> Result<Option<StrictTicketGitResolution>, StrictTicketGitBlocker> {
    let issue_key = clickup_identity_from_task(context.task).preferred_token();
    if let Some(existing) = load_binding(state, project_id, &issue_key).await? {
        let binding = validate_existing_strict_binding(existing, &issue_key)?;
        validate_ticket_git_evidence(state, context.task, &binding).await?;
        ensure_available_owner(state, project_id, &binding, allowed_owner).await?;
        let binding = ensure_binding_pushed(state, binding).await?;
        return Ok(Some(StrictTicketGitResolution { binding }));
    }
    if !context.settings.strict_git_naming_enabled {
        return Ok(None);
    }

    let preview = render_new_preview(context, false)?;
    let project = state
        .project_repo
        .get_by_id(project_id)
        .await
        .map_err(|error| git_blocker(&issue_key, None, error))?
        .ok_or_else(|| {
            git_blocker(
                &issue_key,
                Some(&preview.branch_name),
                format!("Project not found: {project_id}"),
            )
        })?;
    let repo = Path::new(&project.working_directory);
    let target_base_ref =
        GitService::ensure_local_branch_from_origin_if_missing(repo, &preview.target_base_ref)
            .await
            .map_err(|error| git_blocker(&issue_key, Some(&preview.branch_name), error))?;
    let base_commit = GitService::get_branch_sha(repo, &target_base_ref)
        .await
        .map_err(|error| git_blocker(&issue_key, Some(&preview.branch_name), error))?;
    let branch_name = resolve_persisted_branch_name(
        state,
        project_id,
        &issue_key,
        &preview.branch_name,
        &context.task.id,
    )
    .await?;
    let strict_policy = TicketGitConventionSnapshot {
        policy_version: preview.policy_version,
        task_title: preview.task_title,
        username: preview.username,
        commit_subject_rule: preview.commit_subject_rule,
        pr_title: preview.pr_title,
    };
    let candidate = TicketCanonicalBranch::new_strict(
        project_id.clone(),
        CLICKUP_PROVIDER,
        issue_key.clone(),
        branch_name,
        target_base_ref,
        Some(base_commit),
        strict_policy,
        Utc::now(),
    );
    validate_ticket_git_evidence(state, context.task, &candidate).await?;
    let candidate_branch_name = candidate.branch_name.clone();
    let binding = state
        .ticket_canonical_branch_repo
        .create_if_absent(candidate)
        .await
        .map_err(|error| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::BranchBindingConflict,
                error.to_string(),
            )
            .for_task(&issue_key)
        })?;
    let binding = validate_existing_strict_binding(binding, &issue_key)?;
    if binding.branch_name != candidate_branch_name {
        validate_ticket_git_evidence(state, context.task, &binding).await?;
    }
    ensure_available_owner(state, project_id, &binding, allowed_owner).await?;
    let binding = ensure_binding_pushed(state, binding).await?;
    Ok(Some(StrictTicketGitResolution { binding }))
}

/// Resolve and provision strict ClickUp ownership using authoritative services.
/// A persisted strict binding wins without consulting mutable settings or user data.
pub async fn ensure_strict_clickup_ticket_branch_from_services(
    state: &AppState,
    project_id: &ProjectId,
    task: &ClickUpTaskContent,
    target_base_ref: &str,
    allowed_owner: Option<&ChatConversationId>,
) -> Result<Option<StrictTicketGitResolution>, StrictTicketGitBlocker> {
    let issue_key = clickup_identity_from_task(task).preferred_token();
    if let Some(existing) = load_binding(state, project_id, &issue_key).await? {
        let disabled_settings = ClickUpIntegrationSettings::default();
        return ensure_strict_clickup_ticket_branch(
            state,
            project_id,
            StrictClickUpTicketContext {
                task,
                settings: &disabled_settings,
                username: None,
                target_base_ref: &existing.base_branch,
            },
            allowed_owner,
        )
        .await;
    }
    let settings = state
        .clickup_integration_service
        .get_settings()
        .await
        .map_err(|error| convention_service_blocker(&issue_key, error))?;
    if !settings.strict_git_naming_enabled {
        return Ok(None);
    }
    let username = current_username_if_required(state, &settings, &issue_key).await?;
    ensure_strict_clickup_ticket_branch(
        state,
        project_id,
        StrictClickUpTicketContext {
            task,
            settings: &settings,
            username: username.as_deref(),
            target_base_ref,
        },
        allowed_owner,
    )
    .await
}

pub async fn authoritative_clickup_task_for_conversation(
    state: &AppState,
    project_id: &ProjectId,
    conversation_id: &ChatConversationId,
) -> Result<Option<ClickUpTaskContent>, StrictTicketGitBlocker> {
    let links = state
        .external_issue_link_service
        .list_ticket_links_for_conversation(&conversation_id.as_str())
        .await
        .map_err(|error| convention_service_blocker("unknown", error))?;
    let mut clickup_links = links
        .into_iter()
        .filter(|link| link.provider.eq_ignore_ascii_case(CLICKUP_PROVIDER));
    let Some(link) = clickup_links.next() else {
        return Ok(None);
    };
    if clickup_links.any(|candidate| candidate.external_id != link.external_id) {
        return Err(StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::BranchBindingConflict,
            "Conversation is linked to multiple ClickUp tasks",
        ));
    }
    let issue_key = preferred_clickup_task_token(&link.external_id, link.external_key.as_deref());
    if let Some(binding) = load_binding(state, project_id, &issue_key).await? {
        return frozen_clickup_task_for_link(
            &binding,
            &link.external_id,
            link.external_key,
            link.external_url,
        )
        .map(Some);
    }
    let settings = state
        .clickup_integration_service
        .get_settings()
        .await
        .map_err(|error| convention_service_blocker(&issue_key, error))?;
    if !settings.strict_git_naming_enabled {
        return Ok(None);
    }
    state
        .clickup_integration_service
        .fetch_task(&link.external_id)
        .await
        .map(Some)
        .map_err(|error| convention_service_blocker(&issue_key, error))
}

#[cfg(test)]
#[path = "ticket_git_strict_start_tests.rs"]
mod tests;
