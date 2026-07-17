use super::*;

pub(super) async fn resolve_persisted_branch_name(
    state: &AppState,
    project_id: &ProjectId,
    issue_key: &str,
    rendered_branch: &str,
    collision_identity: &str,
) -> Result<String, StrictTicketGitBlocker> {
    let Some(existing) = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(project_id, rendered_branch)
        .await
        .map_err(|error| {
            StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::BranchBindingConflict,
                error.to_string(),
            )
            .for_task(issue_key)
            .for_branch(rendered_branch)
        })?
    else {
        return Ok(rendered_branch.to_string());
    };
    if existing.provider == CLICKUP_PROVIDER && existing.issue_key == issue_key {
        return Ok(existing.branch_name);
    }
    disambiguate_branch_name(rendered_branch, collision_identity).map_err(|error| {
        StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::BranchBindingConflict,
            error.to_string(),
        )
        .for_task(issue_key)
        .for_branch(rendered_branch)
    })
}

pub(super) async fn validate_ticket_git_evidence(
    state: &AppState,
    task: &ClickUpTaskContent,
    binding: &TicketCanonicalBranch,
) -> Result<(), StrictTicketGitBlocker> {
    let project = state
        .project_repo
        .get_by_id(&binding.project_id)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?
        .ok_or_else(|| {
            git_blocker(
                &binding.issue_key,
                Some(&binding.branch_name),
                format!("Project not found: {}", binding.project_id),
            )
        })?;
    let identity = clickup_identity_from_task(task);
    GitService::fetch_origin(Path::new(&project.working_directory))
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?;
    match resolve_clickup_ticket_start(
        &identity,
        Path::new(&project.working_directory),
        state.github_service.as_deref(),
    )
    .await
    .map_err(|error| {
        StrictTicketGitBlocker::new(StrictTicketGitBlockerCode::EvidenceMismatch, error)
            .for_task(&binding.issue_key)
            .for_branch(&binding.branch_name)
    })? {
        ClickUpTicketStartResolution::NoMatch => Ok(()),
        ClickUpTicketStartResolution::Unique(candidate)
            if candidate.branch_name == binding.branch_name
                && candidate.pull_request.as_ref().is_none_or(|pull_request| {
                    pull_request.base_ref_name == binding.base_branch
                }) =>
        {
            Ok(())
        }
        ClickUpTicketStartResolution::Unique(candidate) => Err(StrictTicketGitBlocker::new(
            StrictTicketGitBlockerCode::EvidenceMismatch,
            format!(
                "ClickUp task evidence points to branch '{}' instead of frozen branch '{}'",
                candidate.branch_name, binding.branch_name
            ),
        )
        .for_task(&binding.issue_key)
        .for_branch(&binding.branch_name)),
        ClickUpTicketStartResolution::Ambiguous { branch_names } => {
            Err(StrictTicketGitBlocker::new(
                StrictTicketGitBlockerCode::EvidenceMismatch,
                format!(
                    "ClickUp task evidence matches multiple branches: {}",
                    branch_names.join(", ")
                ),
            )
            .for_task(&binding.issue_key)
            .for_branch(&binding.branch_name))
        }
    }
}

pub(super) async fn ensure_available_owner(
    state: &AppState,
    project_id: &ProjectId,
    binding: &TicketCanonicalBranch,
    allowed_owner: Option<&ChatConversationId>,
) -> Result<(), StrictTicketGitBlocker> {
    let owners = state
        .agent_conversation_workspace_repo
        .find_active_by_project_and_branch_name(project_id, &binding.branch_name)
        .await
        .map_err(|error| {
            StrictTicketGitBlocker::new(StrictTicketGitBlockerCode::ActiveOwner, error.to_string())
                .for_task(&binding.issue_key)
                .for_branch(&binding.branch_name)
        })?;
    if let Some(owner) = owners
        .into_iter()
        .find(|workspace| allowed_owner != Some(&workspace.conversation_id))
    {
        return Err(StrictTicketGitBlocker {
            code: StrictTicketGitBlockerCode::ActiveOwner,
            message: format!(
                "Strict ticket branch '{}' is already owned by conversation {}",
                binding.branch_name, owner.conversation_id
            ),
            task_id: Some(binding.issue_key.clone()),
            expected_branch: Some(binding.branch_name.clone()),
            owner_conversation_id: Some(owner.conversation_id.as_str()),
        });
    }
    Ok(())
}

pub(super) async fn ensure_binding_pushed(
    state: &AppState,
    binding: TicketCanonicalBranch,
) -> Result<TicketCanonicalBranch, StrictTicketGitBlocker> {
    if binding.origin_pushed {
        return Ok(binding);
    }
    let project = state
        .project_repo
        .get_by_id(&binding.project_id)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?
        .ok_or_else(|| {
            git_blocker(
                &binding.issue_key,
                Some(&binding.branch_name),
                format!("Project not found: {}", binding.project_id),
            )
        })?;
    let repo = Path::new(&project.working_directory);
    if !GitService::check_ref_format(repo, &binding.branch_name)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?
    {
        return Err(git_blocker(
            &binding.issue_key,
            Some(&binding.branch_name),
            "Persisted strict ticket branch is not a valid Git ref",
        ));
    }
    if !GitService::branch_exists_strict(repo, &binding.branch_name)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?
    {
        let remote_ref = format!("origin/{}", binding.branch_name);
        let start_ref = if GitService::ref_exists(repo, &remote_ref)
            .await
            .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?
        {
            remote_ref.as_str()
        } else {
            binding.base_branch.as_str()
        };
        GitService::create_branch(repo, &binding.branch_name, start_ref)
            .await
            .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?;
    }
    let github = state.github_service.as_ref().ok_or_else(|| {
        git_blocker(
            &binding.issue_key,
            Some(&binding.branch_name),
            "GitHub integration is not available; cannot push strict ticket branch",
        )
    })?;
    github
        .push_branch(repo, &binding.branch_name)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?;
    state
        .ticket_canonical_branch_repo
        .mark_origin_pushed(&binding.project_id, CLICKUP_PROVIDER, &binding.issue_key)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?;
    state
        .ticket_canonical_branch_repo
        .get(&binding.project_id, CLICKUP_PROVIDER, &binding.issue_key)
        .await
        .map_err(|error| git_blocker(&binding.issue_key, Some(&binding.branch_name), error))?
        .ok_or_else(|| {
            git_blocker(
                &binding.issue_key,
                Some(&binding.branch_name),
                "Strict ticket binding disappeared after push confirmation",
            )
        })
}

pub(super) fn git_blocker(
    task_id: &str,
    branch: Option<&str>,
    error: impl std::fmt::Display,
) -> StrictTicketGitBlocker {
    let mut blocker = StrictTicketGitBlocker::new(
        StrictTicketGitBlockerCode::GitProvisioningFailed,
        error.to_string(),
    )
    .for_task(task_id);
    if let Some(branch) = branch {
        blocker = blocker.for_branch(branch);
    }
    blocker
}
