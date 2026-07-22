use super::*;

pub(crate) fn clickup_task_lookup_key_from_references(
    references: &[ComposerIntegrationReference],
) -> Result<Option<String>, String> {
    let mut lookup_keys = references
        .iter()
        .filter(|reference| {
            reference.provider.trim().eq_ignore_ascii_case("clickup")
                && reference.kind.trim().eq_ignore_ascii_case("clickup")
        })
        .filter_map(|reference| {
            reference
                .key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    let id = reference.id.trim();
                    (!id.is_empty()).then_some(id)
                })
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    lookup_keys.sort_by_key(|key| key.to_ascii_lowercase());
    lookup_keys.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    match lookup_keys.as_slice() {
        [] => Ok(None),
        [lookup_key] => Ok(Some(lookup_key.clone())),
        _ => Err("A conversation can only start from one ClickUp task at a time".to_string()),
    }
}

pub(crate) fn parse_agent_workspace_mode(
    mode: Option<&str>,
) -> Result<AgentConversationWorkspaceMode, String> {
    let mode = mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("edit");
    let mode = mode.parse::<AgentConversationWorkspaceMode>()?;
    if mode == AgentConversationWorkspaceMode::PersonaBuilder
        && !crate::infrastructure::agents::agent_personas_enabled()
    {
        return Err("PersonaBuilder mode requires the agent_personas feature flag".to_string());
    }
    Ok(mode)
}

pub(crate) fn parse_agent_workspace_base_kind(
    kind: Option<&str>,
) -> Result<Option<IdeationAnalysisBaseRefKind>, String> {
    kind.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<IdeationAnalysisBaseRefKind>)
        .transpose()
}

pub(crate) fn parse_agent_workspace_branch_mode(
    branch_mode: Option<&str>,
) -> Result<Option<AgentConversationWorkspaceBranchMode>, String> {
    branch_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::parse::<AgentConversationWorkspaceBranchMode>)
        .transpose()
}

pub(crate) fn trim_optional_input(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl AgentWorkspaceSourcePullRequestInput {
    pub(crate) fn normalize(
        self,
        base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
        base_ref: Option<&str>,
    ) -> Result<AgentWorkspaceSourcePullRequest, String> {
        if self.number <= 0 {
            return Err("Source pull request number must be positive".to_string());
        }
        if base_ref_kind != Some(IdeationAnalysisBaseRefKind::LocalBranch) {
            return Err(
                "Source pull request metadata requires a local_branch base ref".to_string(),
            );
        }

        let head_ref_name = self.head_ref_name.trim().to_string();
        if head_ref_name.is_empty() {
            return Err("Source pull request head branch is required".to_string());
        }
        if let Some(base_ref) = base_ref.map(str::trim).filter(|value| !value.is_empty()) {
            if base_ref != head_ref_name {
                return Err(
                    "Source pull request head branch must match the selected base ref".to_string(),
                );
            }
        }

        Ok(AgentWorkspaceSourcePullRequest {
            number: self.number,
            url: trim_optional_input(self.url),
            title: trim_optional_input(self.title),
            head_ref_name,
            base_ref_name: trim_optional_input(self.base_ref_name),
            head_ref_oid: trim_optional_input(self.head_ref_oid),
        })
    }
}

pub(crate) fn normalize_agent_workspace_source_pull_request(
    input: Option<AgentWorkspaceSourcePullRequestInput>,
    base_ref_kind: Option<IdeationAnalysisBaseRefKind>,
    base_ref: Option<&str>,
) -> Result<Option<AgentWorkspaceSourcePullRequest>, String> {
    input
        .map(|input| input.normalize(base_ref_kind, base_ref))
        .transpose()
}

pub(crate) fn first_ticket_branch_name_hint(
    references: &[ComposerIntegrationReference],
) -> Option<AgentConversationWorkspaceBranchNameHint> {
    references
        .iter()
        .find_map(ticket_branch_name_hint_from_composer_reference)
}

fn ticket_branch_name_hint_from_composer_reference(
    reference: &ComposerIntegrationReference,
) -> Option<AgentConversationWorkspaceBranchNameHint> {
    let provider = match (
        reference.provider.trim().to_ascii_lowercase().as_str(),
        reference.kind.trim().to_ascii_lowercase().as_str(),
    ) {
        ("atlassian", "jira") | ("jira", "jira") => "jira",
        ("linear", "linear") => "linear",
        ("clickup", "clickup") => "clickup",
        _ => return None,
    };
    let ticket_token = reference
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| ticket_id_fallback_token(provider, reference.id.trim()))?;

    Some(AgentConversationWorkspaceBranchNameHint {
        provider: provider.to_string(),
        ticket_token,
    })
}

fn ticket_id_fallback_token(provider: &str, id: &str) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    if provider == "clickup" && !id.to_ascii_uppercase().starts_with("CU-") {
        return Some(format!("CU-{id}"));
    }
    Some(id.to_string())
}
