//! Spawn-free workspace-shell reads for the remote facade.
//!
//! # Why this module exists
//!
//! A paired client could read tasks, conversations, automations, and ideation — but not the
//! two answers its app shell needs before it renders anything at all: *which projects exist*
//! and *is a provider configured*. Both were unregistered, so a connected client fell through
//! to the first-run Welcome screen on a host with a full workspace. The connection was fine;
//! the shell simply had no data.
//!
//! Neither command was blocked by its risk, and neither is blocked by scope. Each is blocked
//! by exactly one authority carrier on an otherwise pure repository read, and this module is
//! the split that removes it — the same shape as `remote_transcript_commands`.
//!
//! ## `list_projects` — the carrier is a filesystem inspection
//!
//! ```text
//! list_projects
//!   -> project_response(project)
//!     -> inspect_repository_capability(project.working_directory)   [FS/git inspection]
//! ```
//!
//! The ledger classes the whole `project_commands` module `Elevated` for "project git/gh and
//! deferred shell authority", and for `list_projects` that verdict is earned by
//! `inspect_repository_capability` alone: `project_repo.get_all()` underneath it is a plain
//! SQLite read. [`list_remote_projects`] drops the inspection and returns the stored row.
//!
//! `repositoryCapability` is therefore **absent by construction**, not merely omitted: it is
//! the field whose computation was the carrier. A client that needs it is asking to run a git
//! inspection on the host, which is a different request than reading the project list.
//!
//! `workingDirectory` IS carried (owner decision, 2026-07-30). It is a stored string, and the
//! authority problem was the act of inspecting that path — never the act of returning it. The
//! paired device is the user's own machine holding `ui:read` on their own host, and the shell
//! displays the path.
//!
//! ## `get_agent_provider_settings` — the carrier is the CLI probe
//!
//! That command is ledgered `Denied` ("configures future provider process authority") because
//! its refresh path runs `refresh_supported_harnesses()`, which probes provider CLIs, and its
//! response carries per-provider model/effort/CLI-version detail.
//!
//! [`get_remote_provider_readiness`] answers neither of those. It reads
//! `agent_provider_settings_repo.list()` and reduces it to two scalars — whether a default
//! provider is configured, and how many providers are enabled. No probe, no CLI path, no model
//! identity, no credential surface. The shell's onboarding gate is a boolean question, so the
//! projection answers a boolean question.
//!
//! # The contract this module keeps
//!
//! Every member is a pure repository read that propagates its errors. No member accepts
//! `tauri::AppHandle`, `ExecutionState`, or a chat service, so no member can carry spawn
//! authority even accidentally — asserted over this file's own source by
//! `the_spawn_free_remote_workspace_module_carries_no_authority_carriers`.

use serde::Serialize;
use tauri::State;

use crate::application::AppState;

/// One project, projected for a remote client's shell.
///
/// Deliberately NOT `ProjectResponse`: that type carries `repository_capability`, whose
/// computation is the spawn carrier this module exists to drop (see the module docs).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProjectView {
    pub id: String,
    pub name: String,
    /// The host's stored path. Display-only here — nothing in this module reads it.
    pub working_directory: String,
    pub git_mode: String,
    pub base_branch: Option<String>,
    pub use_feature_branches: bool,
    pub merge_validation_mode: String,
    pub merge_strategy: String,
    pub github_pr_enabled: bool,
    pub detected_analysis: Option<String>,
    pub custom_analysis: Option<String>,
    pub analyzed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Whether this host is ready to run agents, as two scalars.
///
/// The shell's onboarding gate asks a yes/no question; answering it must not require the
/// `Denied` provider-settings surface (probes, CLI paths, model identities).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProviderReadiness {
    /// True when a provider is both enabled and marked default — the exact condition
    /// `requires_onboarding` negates in `harness_provider_commands`.
    pub onboarding_complete: bool,
    pub enabled_provider_count: u32,
}

/// Lists the host's projects without inspecting any repository.
///
/// # Errors
///
/// Propagates the project repository's error. A read failure must never collapse into an
/// empty list: the client's shell reads emptiness as "first run" and would show onboarding
/// for a populated host.
#[tauri::command]
pub async fn list_remote_projects(
    state: State<'_, AppState>,
) -> Result<Vec<RemoteProjectView>, String> {
    list_remote_projects_for_app_state(state.inner()).await
}

#[doc(hidden)]
pub async fn list_remote_projects_for_app_state(
    state: &AppState,
) -> Result<Vec<RemoteProjectView>, String> {
    let projects = state
        .project_repo
        .get_all()
        .await
        .map_err(|error| error.to_string())?;
    Ok(projects
        .into_iter()
        .map(|project| RemoteProjectView {
            id: project.id.to_string(),
            name: project.name,
            working_directory: project.working_directory,
            git_mode: project.git_mode.to_string(),
            base_branch: project.base_branch,
            use_feature_branches: project.use_feature_branches,
            merge_validation_mode: project.merge_validation_mode.to_string(),
            merge_strategy: project.merge_strategy.to_string(),
            github_pr_enabled: project.github_pr_enabled,
            detected_analysis: project.detected_analysis,
            custom_analysis: project.custom_analysis,
            analyzed_at: project.analyzed_at.map(|at| at.to_rfc3339()),
            created_at: project.created_at.to_rfc3339(),
            updated_at: project.updated_at.to_rfc3339(),
        })
        .collect())
}

/// Reports whether the host has a usable provider, without probing one.
///
/// # Errors
///
/// Propagates the provider-settings repository error. Same fail-closed reason as the project
/// list: "no data" and "could not read" must not both render as "needs onboarding".
#[tauri::command]
pub async fn get_remote_provider_readiness(
    state: State<'_, AppState>,
) -> Result<RemoteProviderReadiness, String> {
    provider_readiness_for_app_state(state.inner()).await
}

#[doc(hidden)]
pub async fn provider_readiness_for_app_state(
    state: &AppState,
) -> Result<RemoteProviderReadiness, String> {
    let stored = state
        .agent_provider_settings_repo
        .list()
        .await
        .map_err(|error| error.to_string())?;
    // Mirrors `read_provider_settings_with_stored_and_probes`: onboarding is complete exactly
    // when some row is BOTH enabled and default. Keeping the predicate identical is what makes
    // the remote shell agree with the local one about whether setup is needed.
    let onboarding_complete = stored.iter().any(|row| row.enabled && row.is_default);
    let enabled_provider_count = stored.iter().filter(|row| row.enabled).count() as u32;
    Ok(RemoteProviderReadiness {
        onboarding_complete,
        enabled_provider_count,
    })
}
