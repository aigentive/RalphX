/**
 * GitHub settings hooks — wraps Tauri commands for GitHub integration UI.
 *
 * Hooks:
 * - useGitRemoteUrl: fetch remote URL for a project
 * - useGitAuthDiagnostics: inspect git fetch/push auth modes
 * - useGhAuthStatus: check if `gh` CLI is authenticated
 * - useLoginGhWithBrowser: authenticate `gh` through the app's browser flow
 * - useSwitchGitOriginToSsh: explicitly switch GitHub origin remotes to SSH
 * - useSetupGhGitAuth: configure GitHub CLI HTTPS credentials for git
 * - useResumeDeferredGitStartup: resume startup work paused by Git auth preflight
 * - useUpdateGithubPrEnabled: mutation to toggle PR mode on a project
 */

import { invoke } from "@tauri-apps/api/core";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useProjectStore } from "@/stores/projectStore";

export interface GitAuthDiagnostics {
  fetchUrl: string | null;
  pushUrl: string | null;
  fetchKind: string | null;
  pushKind: string | null;
  mixedAuthModes: boolean;
  canSwitchToSsh: boolean;
  suggestedSshUrl: string | null;
}

// ============================================================================
// useGitRemoteUrl
// ============================================================================

/**
 * Fetch the git remote URL for a project.
 * Returns null when no remote is configured or projectId is null.
 *
 * @param projectId - The project ID to fetch the remote URL for, or null to disable
 */
export function useGitRemoteUrl(projectId: string | null) {
  return useQuery<string | null>({
    queryKey: ["git-remote-url", projectId],
    queryFn: () =>
      invoke<string | null>("get_git_remote_url", { projectId }),
    enabled: projectId !== null,
  });
}

// ============================================================================
// useGitAuthDiagnostics
// ============================================================================

/**
 * Inspect origin fetch/push auth modes and available explicit repair actions.
 */
export function useGitAuthDiagnostics(projectId: string | null) {
  return useQuery<GitAuthDiagnostics>({
    queryKey: ["git-auth-diagnostics", projectId],
    queryFn: () =>
      invoke<GitAuthDiagnostics>("get_git_auth_diagnostics", { projectId }),
    enabled: projectId !== null,
    staleTime: 0,
    refetchOnMount: "always",
    refetchOnWindowFocus: true,
  });
}

// ============================================================================
// useGhAuthStatus
// ============================================================================

/**
 * Check whether the `gh` CLI is authenticated.
 * Returns true when authenticated, false otherwise.
 */
export function useGhAuthStatus() {
  return useQuery<boolean>({
    queryKey: ["gh-auth-status"],
    queryFn: () => invoke<boolean>("check_gh_auth", {}),
    staleTime: 0,
    refetchOnMount: "always",
    refetchOnWindowFocus: true,
  });
}

// ============================================================================
// useSwitchGitOriginToSsh
// ============================================================================

interface ProjectGitAuthMutationArgs {
  projectId: string;
}

/**
 * Explicitly switch a convertible GitHub HTTPS origin remote to SSH.
 */
export function useSwitchGitOriginToSsh() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({ projectId }: ProjectGitAuthMutationArgs) =>
      invoke<GitAuthDiagnostics>("switch_git_origin_to_ssh", { projectId }),
    onSuccess: (_data, { projectId }) => {
      void queryClient.invalidateQueries({
        queryKey: ["git-auth-diagnostics", projectId],
      });
      void queryClient.invalidateQueries({ queryKey: ["git-remote-url", projectId] });
    },
  });
}

// ============================================================================
// useSetupGhGitAuth
// ============================================================================

/**
 * Configure git credential helpers through an already-authenticated GitHub CLI.
 */
export function useSetupGhGitAuth() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => invoke<boolean>("setup_gh_git_auth", {}),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["gh-auth-status"] });
      void queryClient.invalidateQueries({ queryKey: ["git-auth-diagnostics"] });
    },
  });
}

// ============================================================================
// useLoginGhWithBrowser
// ============================================================================

/**
 * Start GitHub CLI's browser login flow from RalphX's app environment.
 */
export function useLoginGhWithBrowser() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => invoke<boolean>("login_gh_with_browser", {}),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["gh-auth-status"] });
      void queryClient.invalidateQueries({ queryKey: ["git-auth-diagnostics"] });
    },
  });
}

// ============================================================================
// useResumeDeferredGitStartup
// ============================================================================

/**
 * Resume Git/GitHub-dependent startup recovery after repository access is repaired.
 */
export function useResumeDeferredGitStartup() {
  return useMutation({
    mutationFn: () => invoke<boolean>("resume_deferred_git_startup", {}),
  });
}

// ============================================================================
// useUpdateGithubPrEnabled
// ============================================================================

interface UpdateGithubPrEnabledArgs {
  projectId: string;
  enabled: boolean;
}

/**
 * Mutation to toggle GitHub PR mode for a project.
 * On success, updates the projectStore and invalidates the projects query.
 */
export function useUpdateGithubPrEnabled() {
  const queryClient = useQueryClient();
  const updateProject = useProjectStore((s) => s.updateProject);

  return useMutation({
    mutationFn: ({ projectId, enabled }: UpdateGithubPrEnabledArgs) =>
      invoke("update_github_pr_enabled", { projectId, enabled }),
    onSuccess: (_data, { projectId, enabled }) => {
      updateProject(projectId, { githubPrEnabled: enabled });
      void queryClient.invalidateQueries({ queryKey: ["projects"] });
    },
  });
}
