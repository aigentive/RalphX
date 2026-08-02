use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;

use crate::application::agent_client_bundle::AgentClientBundle;
use crate::application::agent_workspace_pr_description::{
    escape_xml_text, format_changed_files, format_commit_summaries, run_git_text, truncate_chars,
    validate_agent_workspace_pr_description_body, DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE,
    MAX_NAME_STATUS_CHARS, MAX_PATCH_EXCERPT_CHARS, MAX_STAT_CHARS,
};
use crate::application::app_state::ResolvedBackgroundAgentRuntime;
use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::manual_role_default_service::ManualRoleDefaultService;
use crate::application::GitService;
use crate::domain::agents::{
    default_approval_policy_for_harness, default_sandbox_mode_for_harness, AgentConfig,
    AgentProviderSettings, AgentRole, RoutingRole, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrDescription,
    ChatConversation, ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranch, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentProviderSettingsRepository,
    ChatConversationRepository,
};
use crate::domain::services::{PlanPrDescriptionDrafter, PrReviewState};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;
use tracing::{info, warn};

pub(crate) struct AppStatePlanPrDescriptionDrafter {
    agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    manual_role_default_service: Arc<ManualRoleDefaultService>,
    agent_clients: AgentClientBundle,
}

impl AppStatePlanPrDescriptionDrafter {
    pub(crate) fn new(
        agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
        manual_role_default_service: Arc<ManualRoleDefaultService>,
        agent_clients: AgentClientBundle,
    ) -> Self {
        Self {
            agent_conversation_workspace_repo,
            chat_conversation_repo,
            agent_provider_settings_repo,
            manual_role_default_service,
            agent_clients,
        }
    }
}

pub(crate) fn build_app_state_plan_pr_description_drafter(
    agent_conversation_workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_provider_settings_repo: Arc<dyn AgentProviderSettingsRepository>,
    manual_role_default_service: Arc<ManualRoleDefaultService>,
    agent_clients: AgentClientBundle,
) -> Arc<dyn PlanPrDescriptionDrafter> {
    Arc::new(AppStatePlanPrDescriptionDrafter::new(
        agent_conversation_workspace_repo,
        chat_conversation_repo,
        agent_provider_settings_repo,
        manual_role_default_service,
        agent_clients,
    ))
}

#[async_trait]
impl PlanPrDescriptionDrafter for AppStatePlanPrDescriptionDrafter {
    async fn draft_plan_description(
        &self,
        project: &Project,
        plan_branch: &PlanBranch,
        review_base: &str,
        review_state: PrReviewState,
    ) -> AppResult<AgentWorkspacePrDescription> {
        draft_plan_pr_description(
            &self.agent_conversation_workspace_repo,
            &self.chat_conversation_repo,
            &self.agent_provider_settings_repo,
            &self.manual_role_default_service,
            &self.agent_clients,
            project,
            plan_branch,
            review_base,
            review_state,
        )
        .await
    }
}

async fn draft_plan_pr_description(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    chat_conversation_repo: &Arc<dyn ChatConversationRepository>,
    provider_settings_repo: &Arc<dyn AgentProviderSettingsRepository>,
    manual_role_default_service: &ManualRoleDefaultService,
    agent_clients: &AgentClientBundle,
    project: &Project,
    plan_branch: &PlanBranch,
    review_base: &str,
    review_state: PrReviewState,
) -> AppResult<AgentWorkspacePrDescription> {
    let repo_path = Path::new(&project.working_directory);

    let synthetic_id = ChatConversationId::new();
    create_synthetic_plan_pr_conversation(chat_conversation_repo, &synthetic_id, project).await?;

    let workspace = AgentConversationWorkspace::new(
        synthetic_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        plan_branch.source_branch.clone(),
        None,
        None,
        plan_branch.branch_name.clone(),
        project.working_directory.clone(),
    );
    if let Err(error) = workspace_repo.create_or_update(workspace.clone()).await {
        cleanup_synthetic_plan_pr_conversation(
            workspace_repo,
            chat_conversation_repo,
            &synthetic_id,
        )
        .await;
        return Err(error);
    }

    let result = draft_plan_pr_description_inner(
        workspace_repo,
        provider_settings_repo,
        manual_role_default_service,
        agent_clients,
        project,
        plan_branch,
        repo_path,
        review_base,
        review_state,
        &synthetic_id,
        &workspace,
    )
    .await;

    cleanup_synthetic_plan_pr_conversation(workspace_repo, chat_conversation_repo, &synthetic_id)
        .await;

    result
}

async fn create_synthetic_plan_pr_conversation(
    chat_conversation_repo: &Arc<dyn ChatConversationRepository>,
    synthetic_id: &ChatConversationId,
    project: &Project,
) -> AppResult<()> {
    let mut conversation = ChatConversation::new_project(project.id.clone());
    conversation.id = synthetic_id.clone();
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::Edit));
    conversation.set_title("Plan PR description draft");
    conversation.archive();
    chat_conversation_repo.create(conversation).await?;
    Ok(())
}

async fn cleanup_synthetic_plan_pr_conversation(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    chat_conversation_repo: &Arc<dyn ChatConversationRepository>,
    synthetic_id: &ChatConversationId,
) {
    if let Err(error) = workspace_repo.delete(synthetic_id).await {
        warn!(
            target: "ralphx_lib::application::plan_pr_description",
            conversation_id = %synthetic_id,
            error = %error,
            "Failed to delete synthetic plan PR workspace"
        );
    }
    if let Err(error) = chat_conversation_repo.delete(synthetic_id).await {
        warn!(
            target: "ralphx_lib::application::plan_pr_description",
            conversation_id = %synthetic_id,
            error = %error,
            "Failed to delete synthetic plan PR conversation"
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn draft_plan_pr_description_inner(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    provider_settings_repo: &Arc<dyn AgentProviderSettingsRepository>,
    manual_role_default_service: &ManualRoleDefaultService,
    agent_clients: &AgentClientBundle,
    project: &Project,
    plan_branch: &PlanBranch,
    repo_path: &Path,
    review_base: &str,
    review_state: PrReviewState,
    synthetic_id: &ChatConversationId,
    _workspace: &AgentConversationWorkspace,
) -> AppResult<AgentWorkspacePrDescription> {
    workspace_repo.clear_pr_description(synthetic_id).await?;

    let review_range = format!("{review_base}..{}", plan_branch.branch_name);
    let diff_stats_fut =
        GitService::get_diff_stats_between(repo_path, review_base, &plan_branch.branch_name);
    let commits_fut =
        GitService::get_commits_between(repo_path, review_base, &plan_branch.branch_name);
    let name_status_args = ["diff", "--find-renames", "--name-status"];
    let diff_stat_args = ["diff", "--find-renames", "--stat"];
    let patch_args = ["diff", "--find-renames", "--minimal", "--no-ext-diff"];
    let name_status_fut = async {
        let mut args = name_status_args.to_vec();
        args.push(&review_range);
        run_git_text(repo_path, &args).await
    };
    let diff_stat_fut = async {
        let mut args = diff_stat_args.to_vec();
        args.push(&review_range);
        run_git_text(repo_path, &args).await
    };
    let patch_excerpt_fut = async {
        let mut args = patch_args.to_vec();
        args.push(&review_range);
        run_git_text(repo_path, &args).await
    };
    let (diff_stats, commits, name_status, diff_stat, patch_excerpt) = tokio::try_join!(
        diff_stats_fut,
        commits_fut,
        name_status_fut,
        diff_stat_fut,
        patch_excerpt_fut
    )?;

    let template = read_pr_template(repo_path).await;
    let prompt = build_plan_pr_describer_prompt(
        synthetic_id,
        project,
        plan_branch,
        review_base,
        review_state,
        &template,
        &commits,
        &diff_stats,
        &name_status,
        &diff_stat,
        &patch_excerpt,
    );

    let runtime = resolve_plan_pr_describer_runtime(
        provider_settings_repo,
        manual_role_default_service,
        agent_clients,
        project,
    )
    .await?;
    let agent_client = Arc::clone(&runtime.client);
    let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        helper_harness,
        agent_names::AGENT_PR_DESCRIBER,
        PathBuf::from(&project.working_directory),
    );
    let env = runtime.env_with_overrides(bootstrap.env);

    info!(
        target: "ralphx_lib::application::plan_pr_description",
        plan_branch_id = %plan_branch.id,
        project_id = %project.id,
        branch = %plan_branch.branch_name,
        harness = %helper_harness,
        "Spawning plan PR describer agent"
    );

    let output = agent_client
        .spawn_agent(AgentConfig {
            role: AgentRole::Custom(bootstrap.agent_role.clone()),
            prompt,
            working_directory: bootstrap.working_directory,
            plugin_dir: Some(bootstrap.plugin_dir),
            agent: Some(bootstrap.agent_name),
            model: runtime.model,
            harness: runtime.harness,
            cli_path_override: runtime.cli_path_override,
            logical_effort: runtime.logical_effort,
            approval_policy: runtime.approval_policy,
            sandbox_mode: runtime.sandbox_mode,
            service_tier: runtime.service_tier,
            max_tokens: None,
            timeout_secs: Some(120),
            env,
            mcp_launch_policy: Default::default(),
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("failed to spawn plan PR describer agent: {error}"))
        })?;

    let output = agent_client
        .wait_for_completion(&output)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("plan PR describer agent failed: {error}"))
        })?;

    if !output.success {
        return Err(AppError::Infrastructure(format!(
            "plan PR describer agent exited unsuccessfully: {}",
            output.content.trim()
        )));
    }

    let Some(description) = workspace_repo.get_pr_description(synthetic_id).await? else {
        return Err(AppError::Infrastructure(
            "plan PR describer agent completed but did not submit a description".to_string(),
        ));
    };
    validate_agent_workspace_pr_description_body(&description.body_markdown)?;

    info!(
        target: "ralphx_lib::application::plan_pr_description",
        plan_branch_id = %plan_branch.id,
        project_id = %project.id,
        branch = %plan_branch.branch_name,
        body_chars = description.body_markdown.chars().count(),
        "Drafted plan PR description"
    );

    Ok(description)
}

async fn resolve_plan_pr_describer_runtime(
    provider_settings_repo: &Arc<dyn AgentProviderSettingsRepository>,
    manual_role_default_service: &ManualRoleDefaultService,
    agent_clients: &AgentClientBundle,
    project: &Project,
) -> AppResult<ResolvedBackgroundAgentRuntime> {
    let purpose = "plan PR describer default provider";
    let resolved = crate::application::agent_lane_resolution::resolve_manual_role_spawn_settings(
        agent_names::AGENT_PR_DESCRIBER,
        Some(project.id.as_str()),
        Some(Path::new(&project.working_directory)),
        RoutingRole::UtilityPrDescriber,
        None,
        None,
        None,
        manual_role_default_service,
    )
    .await?;
    let harness = resolved.effective_harness;
    crate::application::ensure_provider_spawn_enabled(provider_settings_repo, harness, purpose)
        .await
        .map_err(AppError::Infrastructure)?;

    let provider_settings = provider_settings_repo
        .get(harness)
        .await
        .map_err(|e| AppError::Infrastructure(e.to_string()))?
        .unwrap_or_else(|| AgentProviderSettings::disabled_defaults(harness));

    let cli_path_override =
        crate::application::app_state::AppState::managed_cli_path_override_for_provider(
            &provider_settings,
            purpose,
        )?;
    let provider_env =
        crate::application::provider_env_file::load_provider_custom_env_file(&provider_settings)
            .map_err(AppError::Infrastructure)?;

    let client = if harness == agent_clients.default_harness {
        Arc::clone(&agent_clients.default_client)
    } else if cli_path_override.is_some() {
        agent_clients
            .explicit_harness_client(harness)
            .ok_or_else(|| {
                AppError::Infrastructure(format!("{purpose} harness unavailable: {harness}"))
            })?
    } else {
        agent_clients
            .explicit_available_harness_client(harness)
            .await
            .ok_or_else(|| {
                AppError::Infrastructure(format!("{purpose} harness unavailable: {harness}"))
            })?
    };

    let runtime = ResolvedBackgroundAgentRuntime {
        client,
        harness: Some(harness),
        model: Some(resolved.model),
        cli_path_override,
        logical_effort: resolved.logical_effort,
        approval_policy: resolved
            .approval_policy
            .or_else(|| default_approval_policy_for_harness(harness).map(str::to_string)),
        sandbox_mode: resolved
            .sandbox_mode
            .or_else(|| default_sandbox_mode_for_harness(harness).map(str::to_string)),
        service_tier: resolved.service_tier.or(provider_settings.service_tier),
        runtime_source: resolved.runtime_source,
        env: provider_env,
    };

    Ok(runtime)
}

pub(crate) async fn read_pr_template(repo_path: &Path) -> String {
    let template_path = repo_path.join(".github").join("PULL_REQUEST_TEMPLATE.md");
    match tokio::fs::read_to_string(template_path).await {
        Ok(content) if !content.trim().is_empty() => content.trim().to_string(),
        _ => DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.trim().to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_plan_pr_describer_prompt(
    synthetic_conversation_id: &ChatConversationId,
    project: &Project,
    plan_branch: &PlanBranch,
    review_base: &str,
    review_state: PrReviewState,
    template: &str,
    commits: &[crate::application::git_service::CommitInfo],
    diff_stats: &crate::application::git_service::DiffStats,
    name_status: &str,
    diff_stat: &str,
    patch_excerpt: &str,
) -> String {
    let commit_summaries = format_commit_summaries(commits);
    let changed_files = format_changed_files(diff_stats);
    let review_state_label = match review_state {
        PrReviewState::Draft => "draft",
        PrReviewState::Ready => "ready",
    };
    let diff_summary = format!(
        "{} files changed, {} insertions, {} deletions",
        diff_stats.files_changed, diff_stats.insertions, diff_stats.deletions
    );

    format!(
        "<instructions>\n\
         Write a reviewer-focused pull request description for this plan branch.\n\
         Follow the supplied pull request template structure exactly. If a section is not applicable, keep the heading and say so briefly.\n\
         Use only the supplied commit and diff context. Do not invent validation, test results, product impact, or user-visible behavior.\n\
         Describe the final net changes shown by the diff context, not the order of commits or fix iterations.\n\
         Treat commit summaries as secondary clues for intent only; do not narrate them as the work itself.\n\
         Do not include command transcripts, local validation logs, or agent progress diaries.\n\
         Do not mention bounded input limits, excerpt truncation, omitted prompt context, or ask reviewers to compensate for missing helper input.\n\
         If the supplied code context is genuinely ambiguous, name only the product or technical risk you can infer.\n\
         If validation evidence is absent, omit validation claims instead of treating absent validation as a risk.\n\
         Call submit_agent_workspace_pr_description exactly once with conversation_id and body_markdown. Do not include a title.\n\
         Do not inspect files, run shell commands, modify files, delegate, or perform any action other than submitting the PR description.\n\
         </instructions>\n\
         <data>\n\
         <conversation_id>{conversation_id}</conversation_id>\n\
         <project_name>{project_name}</project_name>\n\
         <registered_project_cwd>{project_cwd}</registered_project_cwd>\n\
         <base_ref>{base_ref}</base_ref>\n\
         <branch_name>{branch_name}</branch_name>\n\
         <review_base>{review_base}</review_base>\n\
         <review_state>{review_state}</review_state>\n\
         <template source=\"ralphx_fallback\">\n{template}\n</template>\n\
         <diff_summary>{diff_summary}</diff_summary>\n\
         <changed_files>\n{changed_files}\n</changed_files>\n\
         <name_status>\n{name_status}\n</name_status>\n\
         <diff_stat>\n{diff_stat}\n</diff_stat>\n\
         <patch_excerpt>\n{patch_excerpt}\n</patch_excerpt>\n\
         <commit_summaries secondary=\"true\" order=\"oldest_first\" merge_commits=\"omitted\">\n{commit_summaries}\n</commit_summaries>\n\
         </data>",
        conversation_id = synthetic_conversation_id,
        project_name = escape_xml_text(&project.name),
        project_cwd = escape_xml_text(&project.working_directory),
        base_ref = escape_xml_text(&plan_branch.source_branch),
        branch_name = escape_xml_text(&plan_branch.branch_name),
        review_base = escape_xml_text(review_base),
        review_state = review_state_label,
        template = escape_xml_text(template),
        diff_summary = escape_xml_text(&diff_summary),
        commit_summaries = escape_xml_text(&commit_summaries),
        changed_files = escape_xml_text(&changed_files),
        name_status = escape_xml_text(&truncate_chars(name_status, MAX_NAME_STATUS_CHARS)),
        diff_stat = escape_xml_text(&truncate_chars(diff_stat, MAX_STAT_CHARS)),
        patch_excerpt = escape_xml_text(&truncate_chars(patch_excerpt, MAX_PATCH_EXCERPT_CHARS)),
    )
}
