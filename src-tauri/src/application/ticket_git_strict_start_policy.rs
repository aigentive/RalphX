use super::*;
use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::domain::entities::AgentConversationWorkspace;

pub async fn resolve_strict_ticket_target_base_ref(
    project: &Project,
    kind: Option<IdeationAnalysisBaseRefKind>,
    selected_ref: Option<&str>,
) -> Result<String, StrictTicketGitBlocker> {
    let selected_ref = selected_ref
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let repo = Path::new(&project.working_directory);
    let resolved = match kind {
        Some(IdeationAnalysisBaseRefKind::LocalBranch) => selected_ref.ok_or_else(|| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::GitProvisioningFailed,
                "Strict ClickUp work requires the selected target branch",
            )
        })?,
        Some(IdeationAnalysisBaseRefKind::CurrentBranch) => match selected_ref {
            Some(branch) => branch,
            None => GitService::get_current_branch(repo)
                .await
                .map_err(|error| git_blocker("unknown", None, error))?,
        },
        Some(IdeationAnalysisBaseRefKind::PullRequest) => {
            return Err(StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::GitProvisioningFailed,
                "Strict ClickUp work selects the pull request target branch, not a PR head",
            ));
        }
        None | Some(IdeationAnalysisBaseRefKind::ProjectDefault) => match selected_ref {
            Some(branch) => branch,
            None => {
                GitService::resolve_project_default_branch(repo, project.base_branch.as_deref())
                    .await
            }
        },
    };
    Ok(resolved)
}

pub async fn rollback_strict_ticket_workspace_activation(
    state: &AppState,
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> Result<(), String> {
    let worktree_path = resolve_valid_agent_conversation_workspace_path(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    GitService::delete_worktree(Path::new(&project.working_directory), &worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    state
        .agent_conversation_workspace_repo
        .delete(&workspace.conversation_id)
        .await
        .map_err(|error| error.to_string())
}

pub async fn strict_clickup_ticket_policy_applies(
    state: &AppState,
    project_id: &ProjectId,
    task: &ClickUpTaskContent,
) -> Result<bool, StrictTicketGitBlocker> {
    let issue_key = clickup_identity_from_task(task).preferred_token();
    if let Some(binding) = load_binding(state, project_id, &issue_key).await? {
        if binding.policy_kind == TicketCanonicalBranchPolicyKind::StrictGitConvention {
            return Ok(true);
        }
    }
    state
        .clickup_integration_service
        .get_settings()
        .await
        .map(|settings| settings.strict_git_naming_enabled)
        .map_err(|error| convention_service_blocker(&issue_key, error))
}

pub async fn activate_strict_ticket_branch_cycle(
    state: &AppState,
    binding: &TicketCanonicalBranch,
    workspace_base_commit: Option<&str>,
) -> Result<TicketCanonicalBranch, StrictTicketGitBlocker> {
    if binding.cycle.state == TicketCanonicalBranchCycleState::Active {
        return Ok(binding.clone());
    }
    if binding.cycle.state != TicketCanonicalBranchCycleState::Preparing {
        return Err(StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::InvalidCycleState,
            format!(
                "Strict ticket branch '{}' cannot activate from {}",
                binding.branch_name, binding.cycle.state
            ),
        )
        .for_task(&binding.issue_key)
        .for_branch(&binding.branch_name));
    }
    let base_commit = workspace_base_commit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| binding.cycle.base_commit.clone())
        .ok_or_else(|| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::InvalidCycleState,
                "Strict ticket workspace did not resolve a cycle base commit",
            )
            .for_task(&binding.issue_key)
            .for_branch(&binding.branch_name)
        })?;
    let replacement = TicketCanonicalBranchCycle {
        generation: binding.cycle.generation,
        state: TicketCanonicalBranchCycleState::Active,
        base_commit: Some(base_commit),
        effective_merge_base: None,
        started_at: binding.cycle.started_at,
        terminal_at: None,
    };
    let swapped = state
        .ticket_canonical_branch_repo
        .compare_and_swap_cycle(
            &binding.project_id,
            &binding.provider,
            &binding.issue_key,
            binding.cycle.generation,
            TicketCanonicalBranchCycleState::Preparing,
            replacement,
        )
        .await
        .map_err(|error| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::InvalidCycleState,
                error.to_string(),
            )
            .for_task(&binding.issue_key)
            .for_branch(&binding.branch_name)
        })?;
    let current = load_binding(state, &binding.project_id, &binding.issue_key)
        .await?
        .ok_or_else(|| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::InvalidCycleState,
                "Strict ticket binding disappeared while activating its workspace",
            )
            .for_task(&binding.issue_key)
            .for_branch(&binding.branch_name)
        })?;
    if swapped || current.cycle.state == TicketCanonicalBranchCycleState::Active {
        return Ok(current);
    }
    Err(StrictTicketGitBlocker::new(
        StrictTicketGitBlockerCode::InvalidCycleState,
        "Strict ticket cycle changed concurrently before workspace activation",
    )
    .for_task(&binding.issue_key)
    .for_branch(&binding.branch_name))
}

pub(super) async fn current_username_if_required(
    state: &AppState,
    settings: &ClickUpIntegrationSettings,
    issue_key: &str,
) -> Result<Option<String>, StrictTicketGitBlocker> {
    let needs_username = [
        settings.branch_name_template.as_str(),
        settings.commit_subject_template.as_str(),
        settings.pr_title_template.as_str(),
    ]
    .into_iter()
    .any(|template| template.contains(":username:"));
    if !needs_username {
        return Ok(None);
    }
    let user = state
        .clickup_integration_service
        .current_user()
        .await
        .map_err(|error| convention_service_blocker(issue_key, error))?;
    user.username
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(Some)
        .ok_or_else(|| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::MissingUsername,
                "The authenticated ClickUp user has no username required by the Git convention",
            )
            .for_task(issue_key)
        })
}

pub(super) fn convention_service_blocker(
    issue_key: &str,
    error: impl std::fmt::Display,
) -> StrictTicketGitBlocker {
    StrictTicketGitBlocker::new(
        StrictTicketGitBlockerCode::InvalidConvention,
        error.to_string(),
    )
    .for_task(issue_key)
}

pub(super) async fn load_binding(
    state: &AppState,
    project_id: &ProjectId,
    issue_key: &str,
) -> Result<Option<TicketCanonicalBranch>, StrictTicketGitBlocker> {
    state
        .ticket_canonical_branch_repo
        .get(project_id, CLICKUP_PROVIDER, issue_key)
        .await
        .map_err(|error| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::BranchBindingConflict,
                error.to_string(),
            )
            .for_task(issue_key)
        })
}

pub(super) fn render_new_preview(
    context: StrictClickUpTicketContext<'_>,
    persisted: bool,
) -> Result<StrictTicketGitPreview, StrictTicketGitBlocker> {
    let identity = clickup_identity_from_task(context.task);
    let task_id = identity.preferred_token();
    let templates = TicketGitConventionTemplates::new(
        &context.settings.branch_name_template,
        &context.settings.commit_subject_template,
        &context.settings.pr_title_template,
    )
    .map_err(|error| {
        StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::InvalidConvention,
            error.to_string(),
        )
        .for_task(&task_id)
    })?;
    let username = context
        .username
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let rendered = templates
        .render(&TicketGitConventionContext {
            task_id: &task_id,
            task_name: &context.task.name,
            username,
            summary: Some(&context.task.name),
        })
        .map_err(|error| {
            let code = if error.to_string().contains(":username:") {
                StrictTicketGitBlockerCode::MissingUsername
            } else {
                StrictTicketGitBlockerCode::InvalidConvention
            };
            StrictTicketGitBlocker::new(code, error.to_string()).for_task(&task_id)
        })?;
    let commit_subject_rule = templates
        .render(&TicketGitConventionContext {
            task_id: &task_id,
            task_name: &context.task.name,
            username,
            summary: Some(":summary:"),
        })
        .map_err(|error| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::InvalidConvention,
                error.to_string(),
            )
            .for_task(&task_id)
        })?
        .commit_subject;
    Ok(StrictTicketGitPreview {
        task_id,
        task_title: context.task.name.clone(),
        username: username.map(str::to_string),
        branch_name: rendered.branch_name,
        target_base_ref: context.target_base_ref.trim().to_string(),
        commit_subject_rule,
        pr_title: rendered.pr_title,
        policy_version: STRICT_TICKET_GIT_POLICY_VERSION,
        persisted,
    })
}

pub(super) fn strict_preview_from_binding(
    binding: TicketCanonicalBranch,
) -> Result<StrictTicketGitPreview, StrictTicketGitBlocker> {
    let issue_key = binding.issue_key.clone();
    let binding = validate_existing_strict_binding(binding, &issue_key)?;
    let policy = binding.strict_policy.as_ref().ok_or_else(|| {
        StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::InvalidConvention,
            "Strict ticket binding is missing its frozen policy",
        )
        .for_task(&binding.issue_key)
        .for_branch(&binding.branch_name)
    })?;
    Ok(StrictTicketGitPreview {
        task_id: binding.issue_key.clone(),
        task_title: policy.task_title.clone(),
        username: policy.username.clone(),
        branch_name: binding.branch_name.clone(),
        target_base_ref: binding.base_branch.clone(),
        commit_subject_rule: policy.commit_subject_rule.clone(),
        pr_title: policy.pr_title.clone(),
        policy_version: policy.policy_version,
        persisted: true,
    })
}

pub(super) fn validate_existing_strict_binding(
    binding: TicketCanonicalBranch,
    issue_key: &str,
) -> Result<TicketCanonicalBranch, StrictTicketGitBlocker> {
    if binding.policy_kind != TicketCanonicalBranchPolicyKind::StrictGitConvention {
        return Err(StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::LegacyBindingConflict,
            "A legacy canonical branch already exists for this ClickUp task",
        )
        .for_task(issue_key)
        .for_branch(&binding.branch_name));
    }
    if !matches!(
        binding.cycle.state,
        TicketCanonicalBranchCycleState::Preparing | TicketCanonicalBranchCycleState::Active
    ) {
        return Err(StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::InvalidCycleState,
            format!(
                "Strict ticket branch '{}' is in {} state",
                binding.branch_name, binding.cycle.state
            ),
        )
        .for_task(issue_key)
        .for_branch(&binding.branch_name));
    }
    Ok(binding)
}

pub(super) fn frozen_clickup_task_for_link(
    binding: &TicketCanonicalBranch,
    external_id: &str,
    external_key: Option<String>,
    external_url: Option<String>,
) -> Result<ClickUpTaskContent, StrictTicketGitBlocker> {
    let binding = validate_existing_strict_binding(binding.clone(), &binding.issue_key)?;
    let policy = binding.strict_policy.ok_or_else(|| {
        StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::InvalidConvention,
            "Strict ticket binding is missing its frozen policy",
        )
        .for_task(&binding.issue_key)
        .for_branch(&binding.branch_name)
    })?;
    Ok(ClickUpTaskContent {
        id: external_id.to_string(),
        custom_id: external_key,
        name: policy.task_title,
        url: external_url,
        description: String::new(),
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
